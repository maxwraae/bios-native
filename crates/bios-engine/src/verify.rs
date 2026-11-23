//! What Verify needs from the engine beyond its commands: the read mapper it calls, how a long
//! run reports progress and is cancelled, and reading a verification folder's record and stored
//! alignments.
//!
//! THE MAPPER IS A LIBRARY BIOS CALLS, through [`ReadMapper`]. A build of Bios with a read mapper
//! has it in [`crate::mapping`]; [`FixtureMapper`] replays a finished mapper run from its output
//! folder, for checking the screens; [`Unavailable`] is a mapper that maps nothing.

use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use bios_core::verify::folder::FolderMeta;
use bios_core::verify::mapper::{FinalConsensus, MapperManifest, MapperOutput, MapperReference, MapperRow};
use bios_project::Project;
use flate2::Compression;
use flate2::read::GzDecoder;
use flate2::write::GzEncoder;
use serde_json::{Map, Value};

/// The reads of one sample, as the files hold them (FASTQ, plain or gzipped).
pub enum MapReads {
    /// R1 and R2.
    Pair { r1: Vec<u8>, r2: Vec<u8> },
    /// One file holding both mates, alternating.
    Interlaced(Vec<u8>),
}

impl MapReads {
    /// How many reads the files hold, R1 and R2 together.
    pub fn count(&self) -> usize {
        let records = |bytes: &[u8]| {
            let name = if bytes.starts_with(&[0x1f, 0x8b]) { "reads.fastq.gz" } else { "reads.fastq" };
            bios_core::reads::fastq::parse_bytes(bytes, name).map_or(0, |r| r.len())
        };
        match self {
            MapReads::Pair { r1, r2 } => records(r1) + records(r2),
            MapReads::Interlaced(all) => records(all),
        }
    }
}

/// One mapping to do: these reads against this reference.
pub struct MapJob {
    pub reference: MapperReference,
    pub reads: MapReads,
}

/// Where a long command's progress goes.
type Report = Arc<dyn Fn(&str) + Send + Sync>;

/// How a long command says how far it has got and learns it should stop.
#[derive(Clone, Default)]
pub struct Control {
    cancel: Arc<AtomicBool>,
    progress: Option<Report>,
}

impl Control {
    /// A control that reports to `progress` and stops when `cancel` is set.
    pub fn new(cancel: Arc<AtomicBool>, progress: impl Fn(&str) + Send + Sync + 'static) -> Self {
        Control { cancel, progress: Some(Arc::new(progress)) }
    }

    pub fn cancelled(&self) -> bool {
        self.cancel.load(Ordering::Relaxed)
    }

    pub fn say(&self, what: &str) {
        if let Some(p) = &self.progress {
            p(what);
        }
    }
}

/// A read mapper. It owns every alignment calculation; Bios only hands it reads and a reference
/// and keeps what comes back exactly.
pub trait ReadMapper: Send + Sync {
    /// Maps one sample. Returns `Err` with a sentence for a person when nothing could be mapped or
    /// the run was cancelled (`control.cancelled()`).
    fn map(&self, job: &MapJob, control: &Control) -> Result<MapperOutput, String>;
}

/// What read mapping answers in a build of Bios that has no read mapper.
pub const NO_MAPPER: &str = "Read mapping is not in this build of Bios yet.";

/// The mapper of a build that has none.
pub struct Unavailable;

impl ReadMapper for Unavailable {
    fn map(&self, _: &MapJob, _: &Control) -> Result<MapperOutput, String> {
        Err(NO_MAPPER.into())
    }
}

/// Replays one finished mapper run from its output folder (`reference-aligned.seq`,
/// `sam-rows.jsonl`, `final-consensus.json`, `manifest.json` and the four exports), whatever
/// reads it is given. The reference must be the one that run was made against. For checking the
/// screens on real results until the mapper crate is wired in.
pub struct FixtureMapper {
    pub dir: PathBuf,
}

fn read_text(dir: &Path, name: &str) -> Result<String, String> {
    fs::read_to_string(dir.join(name)).map_err(|e| format!("{name}: {e}"))
}

impl FixtureMapper {
    /// The output folder as a mapper result against `reference`.
    pub fn load(dir: &Path, reference: &MapperReference) -> Result<MapperOutput, String> {
        let padded: String = read_text(dir, "reference-aligned.seq")?.chars().filter(|c| !c.is_whitespace()).collect();
        let rows = read_text(dir, "sam-rows.jsonl")?
            .lines()
            .filter(|l| !l.trim().is_empty())
            .map(|l| serde_json::from_str::<MapperRow>(l).map_err(|e| format!("sam-rows.jsonl: {e}")))
            .collect::<Result<Vec<_>, _>>()?;
        let final_consensus: FinalConsensus =
            serde_json::from_str(&read_text(dir, "final-consensus.json")?).map_err(|e| format!("final-consensus.json: {e}"))?;
        let manifest: MapperManifest = serde_json::from_str(&read_text(dir, "manifest.json")?).map_err(|e| format!("manifest.json: {e}"))?;
        Ok(MapperOutput {
            schema: 1,
            reference: reference.clone(),
            padded_reference: padded,
            rows,
            final_consensus,
            manifest,
            source: Default::default(),
            sam: read_text(dir, "alignment.sam").unwrap_or_default(),
            consensus_fasta: read_text(dir, "consensus.fasta").unwrap_or_default(),
            coverage_tsv: read_text(dir, "coverage.tsv").unwrap_or_default(),
            differences_tsv: read_text(dir, "column-differences.tsv").unwrap_or_default(),
        })
    }
}

impl ReadMapper for FixtureMapper {
    fn map(&self, job: &MapJob, control: &Control) -> Result<MapperOutput, String> {
        control.say("Reading the mapper's result");
        if control.cancelled() {
            return Err("cancelled".into());
        }
        FixtureMapper::load(&self.dir, &job.reference)
    }
}

/// The mapper a session starts with: this build's own ([`crate::mapping::own_mapper`]), or the
/// fixture named by `BIOS_ALIGNMENT_FIXTURE` when that is set (for checking the screens on a finished run).
pub fn default_mapper() -> Arc<dyn ReadMapper> {
    match std::env::var_os("BIOS_ALIGNMENT_FIXTURE") {
        Some(dir) => Arc::new(FixtureMapper { dir: PathBuf::from(dir) }),
        None => crate::mapping::own_mapper(),
    }
}

/// A verification folder's record, or `None` when the folder is not one.
pub fn load_meta(project: &Project, folder: &str) -> Result<Option<FolderMeta>, String> {
    let Some(meta) = project.folder_meta(folder).map_err(|e| e.to_string())? else {
        return Ok(None);
    };
    if meta.kind != "verification" {
        return Ok(None);
    }
    let mut object = meta.data.clone();
    object.insert("v".into(), Value::from(meta.v));
    object.insert("type".into(), Value::from(meta.kind.clone()));
    object.insert("createdAt".into(), Value::from(meta.created_at.clone()));
    serde_json::from_value(Value::Object(object)).map(Some).map_err(|e| format!("{folder}'s record cannot be read: {e}"))
}

/// Writes a verification folder's record.
pub fn save_meta(project: &Project, folder: &str, meta: &FolderMeta) -> Result<(), String> {
    let Value::Object(mut data) = serde_json::to_value(meta).map_err(|e| e.to_string())? else {
        return Err("a folder record is an object".into());
    };
    for key in ["v", "type", "createdAt"] {
        data.remove(key);
    }
    let record = bios_project::FolderMeta { v: meta.v, kind: "verification".into(), created_at: meta.created_at.clone(), data: Map::from_iter(data) };
    project.write_folder_meta(folder, &record).map_err(|e| e.to_string())
}

/// Every verification folder in the project, with its record, by name.
pub fn verifications(project: &Project) -> Vec<(String, FolderMeta)> {
    let mut out: Vec<(String, FolderMeta)> = project
        .folder_metas()
        .unwrap_or_default()
        .into_iter()
        .filter(|(_, m)| m.kind == "verification")
        .filter_map(|(path, _)| {
            let folder = path.components().map(|c| c.as_os_str().to_string_lossy()).collect::<Vec<_>>().join("/");
            load_meta(project, &folder).ok().flatten().map(|m| (folder, m))
        })
        .collect();
    out.sort_by(|a, b| bios_core::verify::natural_cmp(&a.0, &b.0));
    out
}

/// A short fingerprint for a drawer name.
fn digest(text: &str) -> String {
    bios_core::verify::folder::ref_hash_of(text).chars().take(8).collect()
}

/// A name kept as is when it is a drawer name already, otherwise slugged with a fingerprint of
/// the original so two names never share a file.
fn drawer_safe(name: &str) -> String {
    let plain = !name.is_empty()
        && name.len() <= 150
        && name.starts_with(|c: char| c.is_ascii_alphanumeric())
        && name.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'));
    if plain {
        return name.to_string();
    }
    let slug: String = name.chars().map(|c| if c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-') { c } else { '_' }).collect();
    let slug: String = slug.trim_start_matches(|c: char| !c.is_ascii_alphanumeric()).chars().take(100).collect();
    let slug = if slug.is_empty() { "reads".to_string() } else { slug };
    format!("{slug}-{}", digest(name))
}

/// Where a stored read file lives in its folder's drawer.
pub fn reads_file(name: &str) -> String {
    format!("reads/{}", drawer_safe(name))
}

/// Where an alignment of `sample` against `map` lives in its folder's drawer.
pub fn run_file(sample: &str, map: Option<&str>) -> String {
    let key = format!("{}\u{0}{sample}", map.unwrap_or_default());
    let slug: String = sample.chars().map(|c| if c.is_ascii_alphanumeric() || matches!(c, '_' | '-') { c } else { '_' }).take(80).collect();
    let slug = slug.trim_start_matches(|c: char| !c.is_ascii_alphanumeric()).to_string();
    let slug = if slug.is_empty() { "run".to_string() } else { slug };
    format!("runs/{slug}-{}.json.gz", digest(&key))
}

/// Keeps one alignment, gzipped.
pub fn save_run(project: &Project, folder: &str, sample: &str, map: Option<&str>, output: &MapperOutput) -> Result<(), String> {
    let json = serde_json::to_vec(output).map_err(|e| e.to_string())?;
    let mut gz = GzEncoder::new(Vec::new(), Compression::fast());
    gz.write_all(&json).map_err(|e| e.to_string())?;
    let bytes = gz.finish().map_err(|e| e.to_string())?;
    project.write_folder_bytes(folder, &run_file(sample, map), &bytes).map_err(|e| e.to_string())
}

/// One stored alignment, read back whole.
pub fn load_run(project: &Project, folder: &str, sample: &str, map: Option<&str>) -> Result<MapperOutput, String> {
    let Some(bytes) = project.read_folder_bytes(folder, &run_file(sample, map)).map_err(|e| e.to_string())? else {
        return Err(format!("the alignment of {sample} is not in {folder} any more"));
    };
    let mut json = Vec::new();
    GzDecoder::new(bytes.as_slice()).read_to_end(&mut json).map_err(|e| format!("the alignment of {sample} cannot be read: {e}"))?;
    serde_json::from_slice(&json).map_err(|e| format!("the alignment of {sample} cannot be read: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn drawer_names_keep_a_sequencers_name_and_slug_anything_else() {
        assert_eq!(reads_file("A7_S7_R1_001.fastq.gz"), "reads/A7_S7_R1_001.fastq.gz");
        let odd = reads_file("my reads (1).fastq.gz");
        assert!(odd.starts_with("reads/my_reads__1_.fastq.gz-"), "{odd}");
        assert_ne!(run_file("A1", Some("m1")), run_file("A1", Some("m2")));
    }
}
