//! Which files are the two halves of one sample?
//!
//! Demultiplexing writes two files per sample and names them so a machine can tell:
//! `Exon4_A7_S7_R1_001.fastq.gz` and `..._R2_001.fastq.gz`. bcl2fastq's shape is
//! `<sample>_S<n>[_L<lane>]_R<1|2>_001.fastq.gz`; other tools drop the `_001`, write `.R1.`, or use a
//! bare `_1` / `_2`. All of those pair here. A file whose partner is not in the drop is mapped on its
//! own.
//!
//! Names first, because they are free. But a name is a convention, not a fact, so the reads are the
//! second witness: every Illumina record carries its cluster id and which read it is, and two files
//! whose FIRST records name the same cluster with different read numbers are the two halves of one
//! sample whatever they are called. [`group_reads`] uses both; [`group_pairs`] is the name-only half.

use std::collections::{HashMap, HashSet};
use std::io::Read;

use flate2::read::MultiGzDecoder;

/// One unit of input: a file on its own, or the two writes for one sample. `name` is the file's
/// name for a single, the shared stem for a pair.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ReadsInput<F> {
    Single { name: String, file: F },
    Pair { name: String, r1: F, r2: F },
}

impl<F> ReadsInput<F> {
    pub fn name(&self) -> &str {
        match self {
            ReadsInput::Single { name, .. } | ReadsInput::Pair { name, .. } => name,
        }
    }

    pub fn is_pair(&self) -> bool {
        matches!(self, ReadsInput::Pair { .. })
    }

    /// The files behind it, R1 first.
    pub fn files(&self) -> Vec<&F> {
        match self {
            ReadsInput::Single { file, .. } => vec![file],
            ReadsInput::Pair { r1, r2, .. } => vec![r1, r2],
        }
    }
}

/// Where a file would sit in a pair, from its name alone.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Mate {
    /// What a file's two mates share.
    pub key: String,
    pub sample: String,
    pub mate: u8,
}

/// The name without `.fastq`, `.fq`, `.fastq.gz` or `.fq.gz` (any case), or `None` without one.
fn strip_reads_ext(name: &str) -> Option<&str> {
    let lower = name.to_ascii_lowercase();
    [".fastq.gz", ".fq.gz", ".fastq", ".fq"].iter().find(|ext| lower.ends_with(*ext)).and_then(|ext| name.get(..name.len() - ext.len()))
}

fn is_sep(c: u8) -> bool {
    matches!(c, b'.' | b'_' | b'-')
}

/// `^(.*?)([._-])(R?)([12])((?:[._-]\d{3})?)\.(?:fastq|fq)(?:\.gz)?$`, as the web writes it: the
/// shortest stem for which the rest fits. The separator must sit immediately before the mate
/// number: `pUC19_A1.fastq.gz` is a well, not read 1 of `pUC19_A`, and stays single.
pub fn mate_of(name: &str) -> Option<Mate> {
    let body = strip_reads_ext(name)?;
    let b = body.as_bytes();
    let n = b.len();
    for s in 0..n {
        if !is_sep(b[s]) {
            continue;
        }
        // Try with the optional R taken, then without, as a regex engine would.
        for take_r in [true, false] {
            let mut i = s + 1;
            if take_r {
                if i < n && (b[i] == b'R' || b[i] == b'r') {
                    i += 1;
                } else {
                    continue;
                }
            }
            let Some(&digit) = b.get(i) else { continue };
            if digit != b'1' && digit != b'2' {
                continue;
            }
            let rest = &b[i + 1..];
            let suffix_ok = rest.is_empty() || (rest.len() == 4 && is_sep(rest[0]) && rest[1..].iter().all(u8::is_ascii_digit));
            if !suffix_ok {
                continue;
            }
            let stem = body.get(..s)?;
            let sep = body.get(s..s + 1)?;
            let r = if take_r { body.get(s + 1..s + 2)? } else { "" };
            let suffix = body.get(i + 1..)?;
            return Some(Mate { key: format!("{stem}{sep}{r}|{suffix}"), sample: stem.to_string(), mate: digit - b'0' });
        }
    }
    None
}

/// Groups a drop into samples, in the order the files were given, by name alone.
pub fn group_pairs<F: Clone>(files: &[(String, F)]) -> Vec<ReadsInput<F>> {
    struct Slot<F> {
        index: usize,
        r1: Option<(String, F)>,
        r2: Option<(String, F)>,
        sample: String,
    }
    let mut out: Vec<ReadsInput<F>> = Vec::new();
    let mut open: HashMap<String, Slot<F>> = HashMap::new();
    for (name, file) in files {
        let Some(m) = mate_of(name) else {
            out.push(ReadsInput::Single { name: name.clone(), file: file.clone() });
            continue;
        };
        let slot = open.entry(m.key.clone()).or_insert_with(|| {
            // Placeholder, filled in once we know whether the partner turns up.
            out.push(ReadsInput::Single { name: name.clone(), file: file.clone() });
            Slot { index: out.len() - 1, r1: None, r2: None, sample: m.sample.clone() }
        });
        if m.mate == 1 && slot.r1.is_none() {
            slot.r1 = Some((name.clone(), file.clone()));
        } else if m.mate == 2 && slot.r2.is_none() {
            slot.r2 = Some((name.clone(), file.clone()));
        } else {
            // A second R1 for the same stem is not a mate of anything here. Keep it, on its own.
            out.push(ReadsInput::Single { name: name.clone(), file: file.clone() });
            continue;
        }
        let entry = match (&slot.r1, &slot.r2) {
            (Some((_, r1)), Some((_, r2))) => ReadsInput::Pair { name: slot.sample.clone(), r1: r1.clone(), r2: r2.clone() },
            (Some((n, f)), None) | (None, Some((n, f))) => ReadsInput::Single { name: n.clone(), file: f.clone() },
            (None, None) => continue,
        };
        if let Some(at) = out.get_mut(slot.index) {
            *at = entry;
        }
    }
    out
}

/// The cluster a record came from and which read of the pair it is, from its id line.
///
/// Illumina: `<cluster> <read>:<filtered>:<control>:<index>`. Older files and the SRA: `<cluster>/1`
/// or `<cluster>/2`. Anything else has a cluster and no mate.
pub fn mate_from_record_id(id: &str) -> (String, Option<u8>) {
    let sp = id.find(char::is_whitespace);
    let head = sp.map_or(id, |s| &id[..s]);
    let hb = head.as_bytes();
    if hb.len() >= 2 && hb[hb.len() - 2] == b'/' && matches!(hb[hb.len() - 1], b'1' | b'2') {
        return (head[..head.len() - 2].to_string(), Some(hb[hb.len() - 1] - b'0'));
    }
    let token = sp.map_or("", |s| id[s..].trim_start());
    let t = token.as_bytes();
    let mate = (t.len() >= 4 && matches!(t[0], b'1' | b'2') && t[1] == b':' && matches!(t[2], b'Y' | b'N') && t[3] == b':').then(|| t[0] - b'0');
    (head.to_string(), mate)
}

/// What to call a pair whose files were not named as one: the part of the two names that agrees,
/// or R1's name without its extension when nothing does.
pub fn sample_name_of(a: &str, b: &str) -> String {
    let n: usize = a.chars().zip(b.chars()).take_while(|(x, y)| x == y).map(|(x, _)| x.len_utf8()).sum();
    let common = a.get(..n).unwrap_or_default().trim_end_matches(['.', '_', '-']);
    let common = strip_reads_ext(common).unwrap_or(common);
    if common.chars().count() >= 3 { common.to_string() } else { strip_reads_ext(a).unwrap_or(a).to_string() }
}

/// How much of a file is read to find its first record. Enough for any id line, gzipped or not.
pub const SNIFF_BYTES: usize = 64 * 1024;

/// The id of the first record in the opening bytes of a FASTQ, decompressing a gzip head if asked.
/// A gzip stream cut off mid-member still decodes its opening bytes, which is what makes peeking
/// at a 400 MB file cost 64 kB.
pub fn first_record_id(bytes: &[u8], gzipped: bool) -> Option<String> {
    let text: Vec<u8> = if gzipped {
        let mut out = Vec::new();
        let mut dec = MultiGzDecoder::new(bytes);
        let mut buf = [0u8; 4096];
        while out.len() < SNIFF_BYTES {
            match dec.read(&mut buf) {
                // A truncated last block fails once the input runs out; what came before is good.
                Ok(0) | Err(_) => break,
                Ok(k) => {
                    out.extend_from_slice(&buf[..k]);
                    if out.contains(&b'\n') {
                        break;
                    }
                }
            }
        }
        out
    } else {
        bytes.to_vec()
    };
    let end = text.iter().position(|&c| c == b'\n').unwrap_or(text.len());
    let mut line: String = text[..end].iter().map(|&c| c as char).collect();
    if line.ends_with('\r') {
        line.pop();
    }
    (line.starts_with('@') && line.chars().count() >= 2).then(|| line.chars().skip(1).collect())
}

/// Groups a drop by name and then by what the reads say. `head` gives a file's first record id
/// (see [`first_record_id`]). Files the names could not pair are paired when their first records
/// share a cluster with different read numbers; anything unreadable or alone stays single.
pub fn group_reads<F: Clone>(files: &[(String, F)], head: impl Fn(&str, &F) -> Option<String>) -> Vec<ReadsInput<F>> {
    let by_name = group_pairs(files);
    let singles: Vec<(usize, String, F)> = by_name
        .iter()
        .enumerate()
        .filter_map(|(i, input)| match input {
            ReadsInput::Single { name, file } => Some((i, name.clone(), file.clone())),
            ReadsInput::Pair { .. } => None,
        })
        .collect();
    if singles.len() < 2 {
        return by_name;
    }
    // cluster -> the singles that start there, by read number, in first-seen order.
    let mut order: Vec<String> = Vec::new();
    let mut by_cluster: HashMap<String, HashMap<u8, usize>> = HashMap::new();
    for (k, (_, name, file)) in singles.iter().enumerate() {
        let Some(id) = head(name, file) else { continue };
        let (cluster, Some(mate)) = mate_from_record_id(&id) else { continue };
        if !by_cluster.contains_key(&cluster) {
            order.push(cluster.clone());
        }
        by_cluster.entry(cluster).or_default().entry(mate).or_insert(k);
    }
    let mut joined: HashMap<usize, ReadsInput<F>> = HashMap::new();
    let mut dropped: HashSet<usize> = HashSet::new();
    for cluster in &order {
        let Some(slot) = by_cluster.get(cluster) else { continue };
        let (Some(a), Some(b)) = (slot.get(&1).and_then(|&k| singles.get(k)), slot.get(&2).and_then(|&k| singles.get(k))) else { continue };
        joined.insert(a.0, ReadsInput::Pair { name: sample_name_of(&a.1, &b.1), r1: a.2.clone(), r2: b.2.clone() });
        dropped.insert(b.0);
    }
    if joined.is_empty() {
        return by_name;
    }
    by_name.into_iter().enumerate().filter(|(i, _)| !dropped.contains(i)).map(|(i, input)| joined.remove(&i).unwrap_or(input)).collect()
}

/// The part of a read's id that both mates share: everything before the first space, without a
/// trailing `/1`, `/2` (Casava) or `.1`, `.2` (fastq-dump's `<accession>.<spot>.<read>`).
pub fn cluster_of(id: &str) -> String {
    let head = id.split(char::is_whitespace).next().unwrap_or(id);
    let b = head.as_bytes();
    if b.len() >= 2 && matches!(b[b.len() - 2], b'/' | b'.') && matches!(b[b.len() - 1], b'1' | b'2') {
        return head[..head.len() - 2].to_string();
    }
    head.to_string()
}

#[cfg(test)]
mod tests {
    use std::io::Write;

    use flate2::Compression;
    use flate2::write::GzEncoder;

    use super::*;

    fn files(names: &[&str]) -> Vec<(String, String)> {
        names.iter().map(|n| (n.to_string(), n.to_string())).collect()
    }

    fn kinds<F>(got: &[ReadsInput<F>]) -> Vec<&'static str> {
        got.iter().map(|g| if g.is_pair() { "pair" } else { "single" }).collect()
    }

    #[test]
    fn bcl2fastqs_two_files_become_one_sample_named_by_the_stem() {
        let got = group_pairs(&files(&["Part4_with_EF1A_aZQ4102_A7_S7_R1_001.fastq.gz", "Part4_with_EF1A_aZQ4102_A7_S7_R2_001.fastq.gz"]));
        assert_eq!(got.len(), 1);
        assert!(got[0].is_pair());
        assert_eq!(got[0].name(), "Part4_with_EF1A_aZQ4102_A7_S7");
    }

    #[test]
    fn order_in_the_drop_does_not_decide_which_is_r1() {
        let got = group_pairs(&files(&["s_R2_001.fastq.gz", "s_R1_001.fastq.gz"]));
        assert_eq!(got[0], ReadsInput::Pair { name: "s".into(), r1: "s_R1_001.fastq.gz".into(), r2: "s_R2_001.fastq.gz".into() });
    }

    #[test]
    fn a_plate_keeps_its_order_and_its_wells_apart() {
        let got = group_pairs(&files(&["A7_S7_R1_001.fastq.gz", "B7_S19_R1_001.fastq.gz", "A7_S7_R2_001.fastq.gz", "B7_S19_R2_001.fastq.gz"]));
        let named: Vec<(&str, &str)> = got.iter().map(|g| (if g.is_pair() { "pair" } else { "single" }, g.name())).collect();
        assert_eq!(named, vec![("pair", "A7_S7"), ("pair", "B7_S19")]);
    }

    #[test]
    fn a_mate_with_no_partner_in_the_drop_is_mapped_on_its_own() {
        let got = group_pairs(&files(&["s_R1_001.fastq.gz"]));
        assert_eq!(got, vec![ReadsInput::Single { name: "s_R1_001.fastq.gz".into(), file: "s_R1_001.fastq.gz".into() }]);
    }

    #[test]
    fn a_well_named_with_a_trailing_digit_is_not_a_mate() {
        let got = group_pairs(&files(&["pUC19_A1.fastq.gz", "pUC19_A2.fastq.gz"]));
        assert_eq!(kinds(&got), vec!["single", "single"]);
    }

    #[test]
    fn the_other_spellings_pair_too() {
        for (a, b) in [
            ("x_R1.fastq.gz", "x_R2.fastq.gz"),
            ("x.R1.fq", "x.R2.fq"),
            ("x_1.fastq", "x_2.fastq"),
            ("x_S3_L001_R1_001.fastq.gz", "x_S3_L001_R2_001.fastq.gz"),
        ] {
            let got = group_pairs(&files(&[a, b]));
            assert_eq!(got.len(), 1, "{a}");
            assert!(got[0].is_pair(), "{a}");
        }
    }

    #[test]
    fn r1_with_001_does_not_pair_with_r2_without_it() {
        assert_eq!(group_pairs(&files(&["x_R1_001.fastq.gz", "x_R2.fastq.gz"])).len(), 2);
    }

    #[test]
    fn a_second_r1_for_the_same_stem_stays_single_rather_than_stealing_the_pair() {
        assert_eq!(kinds(&group_pairs(&files(&["x_R1.fq", "x_R2.fq", "x_R1.fq"]))), vec!["pair", "single"]);
    }

    #[test]
    fn mate_of_reads_the_mate_number() {
        assert_eq!(mate_of("s_R2_001.fastq.gz").map(|m| m.mate), Some(2));
        assert_eq!(mate_of("s.fastq.gz"), None);
    }

    const ILLUMINA_R1: &str = "M08845:459:000000000-GYD2N:1:1101:16485:1360 1:N:0:TGGTAACG+CCAGTTAG";
    const ILLUMINA_R2: &str = "M08845:459:000000000-GYD2N:1:1101:16485:1360 2:N:0:TGGTAACG+CCAGTTAG";

    #[test]
    fn cluster_of_strips_the_read_token_and_the_slash_suffix() {
        assert_eq!(cluster_of(ILLUMINA_R1), "M08845:459:000000000-GYD2N:1:1101:16485:1360");
        assert_eq!(cluster_of(ILLUMINA_R2), "M08845:459:000000000-GYD2N:1:1101:16485:1360");
        assert_eq!(cluster_of("SRR123.1/1"), "SRR123.1");
        assert_eq!(cluster_of("SRR123.1/2"), "SRR123.1");
    }

    fn fastq_bytes(id: &str, n: usize) -> Vec<u8> {
        let mut out = String::new();
        for i in 0..n {
            out += &format!("@{}\nACGTACGTACGTACGTACGT\n+\nIIIIIIIIIIIIIIIIIIII\n", id.replace("1360", &(1360 + i).to_string()));
        }
        out.into_bytes()
    }

    fn gzip(bytes: &[u8]) -> Vec<u8> {
        let mut gz = GzEncoder::new(Vec::new(), Compression::default());
        gz.write_all(bytes).unwrap();
        gz.finish().unwrap()
    }

    #[test]
    fn mate_from_record_id_reads_illuminas_read_token() {
        assert_eq!(mate_from_record_id(ILLUMINA_R1), ("M08845:459:000000000-GYD2N:1:1101:16485:1360".to_string(), Some(1)));
        assert_eq!(mate_from_record_id(ILLUMINA_R2), ("M08845:459:000000000-GYD2N:1:1101:16485:1360".to_string(), Some(2)));
    }

    #[test]
    fn mate_from_record_id_reads_the_slash_suffix() {
        assert_eq!(mate_from_record_id("SRR123.1/2"), ("SRR123.1".to_string(), Some(2)));
    }

    #[test]
    fn a_bare_id_has_a_cluster_and_no_mate() {
        assert_eq!(mate_from_record_id("fixture_0 pos=4553 strand=+"), ("fixture_0".to_string(), None));
    }

    #[test]
    fn first_record_id_of_plain_fastq() {
        assert_eq!(first_record_id(&fastq_bytes(ILLUMINA_R1, 300), false).as_deref(), Some(ILLUMINA_R1));
    }

    #[test]
    fn a_gzip_head_cut_off_mid_stream_still_yields_the_first_id() {
        // Varied text so the compressed stream is longer than the slice.
        let mut text = Vec::new();
        for i in 0..5000 {
            text.extend(
                format!("@{}\n{}\n+\n{}\n", ILLUMINA_R2.replace("1360", &i.to_string()), "ACGT".repeat(i % 7 + 3), "I".repeat((i % 7 + 3) * 4))
                    .bytes(),
            );
        }
        let first = ILLUMINA_R2.replace("1360", "0");
        let gz = gzip(&text);
        assert!(gz.len() > 4096);
        assert_eq!(first_record_id(&gz[..4096], true), Some(first));
    }

    #[test]
    fn not_a_fastq_is_none() {
        assert_eq!(first_record_id(b"LOCUS pUC19", false), None);
    }

    /// Files with the given first record, as `group_reads` sees them through `head`.
    fn by_heads(list: &[(&str, &str, bool)]) -> Vec<ReadsInput<String>> {
        let bytes: HashMap<String, (Vec<u8>, bool)> = list
            .iter()
            .map(|(name, id, gz)| {
                let raw = fastq_bytes(id, 300);
                (name.to_string(), (if *gz { gzip(&raw) } else { raw }, *gz))
            })
            .collect();
        let names: Vec<(String, String)> = list.iter().map(|(n, _, _)| (n.to_string(), n.to_string())).collect();
        group_reads(&names, |_, f| bytes.get(f).and_then(|(b, gz)| first_record_id(&b[..b.len().min(SNIFF_BYTES)], *gz)))
    }

    #[test]
    fn two_files_with_the_same_first_cluster_and_different_read_numbers_are_a_pair() {
        let got = by_heads(&[("wellA.fastq.gz", ILLUMINA_R1, true), ("wellA_other.fastq.gz", ILLUMINA_R2, true)]);
        assert_eq!(got, vec![ReadsInput::Pair { name: "wellA".into(), r1: "wellA.fastq.gz".into(), r2: "wellA_other.fastq.gz".into() }]);
    }

    #[test]
    fn reads_order_in_the_drop_does_not_decide_which_is_r1() {
        let got = by_heads(&[("b.fastq", ILLUMINA_R2, false), ("a.fastq", ILLUMINA_R1, false)]);
        assert!(matches!(&got[0], ReadsInput::Pair { r1, .. } if r1 == "a.fastq"));
    }

    #[test]
    fn two_files_from_different_clusters_stay_single() {
        let other = ILLUMINA_R1.replace("1360", "9999");
        assert_eq!(kinds(&by_heads(&[("a.fastq", ILLUMINA_R1, false), ("b.fastq", &other, false)])), vec!["single", "single"]);
    }

    #[test]
    fn two_r1s_of_the_same_cluster_are_not_a_pair() {
        assert_eq!(kinds(&by_heads(&[("a.fastq", ILLUMINA_R1, false), ("b.fastq", ILLUMINA_R1, false)])), vec!["single", "single"]);
    }

    #[test]
    fn names_win_where_they_can_and_the_reads_fill_in_the_rest() {
        let (o1, o2) = (ILLUMINA_R1.replace("1360", "5555"), ILLUMINA_R2.replace("1360", "5555"));
        let got = by_heads(&[
            ("s_R1_001.fastq.gz", ILLUMINA_R1, true),
            ("s_R2_001.fastq.gz", ILLUMINA_R2, true),
            ("odd_one.fastq", &o1, false),
            ("odd_two.fastq", &o2, false),
        ]);
        let named: Vec<(bool, &str)> = got.iter().map(|g| (g.is_pair(), g.name())).collect();
        assert_eq!(named, vec![(true, "s"), (true, "odd")]);
    }

    #[test]
    fn a_single_end_run_with_bare_ids_is_left_alone() {
        assert_eq!(kinds(&by_heads(&[("A1.fastq", "fixture_0 pos=1", false), ("B1.fastq", "fixture_0 pos=1", false)])), vec!["single", "single"]);
    }

    #[test]
    fn sample_name_of_is_the_common_prefix_trimmed_of_separators_and_extension() {
        assert_eq!(sample_name_of("well7.fastq.gz", "well7.mate.fastq.gz"), "well7");
        assert_eq!(sample_name_of("plate_A7_fwd.fq", "plate_A7_rev.fq"), "plate_A7");
    }

    #[test]
    fn sample_name_of_is_r1s_own_name_when_nothing_agrees() {
        assert_eq!(sample_name_of("forward.fastq", "reverse.fastq"), "forward");
    }

    #[test]
    fn fastq_dumps_accession_spot_read_clusters_agree_between_mates() {
        assert_eq!(cluster_of("SRR000001.1.1 length=100"), cluster_of("SRR000001.1.2 length=100"));
        assert_eq!(cluster_of("SRR000001.1.1 length=100"), "SRR000001.1");
    }

    #[test]
    fn a_spot_with_no_read_number_is_left_alone_by_comparison() {
        assert_eq!(cluster_of("SRR000001.7"), cluster_of("SRR000001.7"));
    }

    #[test]
    fn illumina_and_casava_still_agree_between_mates() {
        assert_eq!(cluster_of(ILLUMINA_R1), cluster_of(ILLUMINA_R2));
        assert_eq!(cluster_of("CLUSTER:1:2:3/1"), cluster_of("CLUSTER:1:2:3/2"));
    }

    #[test]
    fn two_genuinely_different_clusters_do_not_collapse() {
        assert_ne!(cluster_of("M0:1:1:100:200 1:N:0:A"), cluster_of("M0:1:1:100:201 1:N:0:A"));
    }
}
