//! A VERIFICATION FOLDER IS ITS MAPS. What a verification folder knows about itself, in the web's
//! `<folder>/.bios/folder.json` shape (`packages/shared/src/folders.ts`), plus the samples waiting
//! on the board, which the web kept in the browser and Bios keeps beside the rest.
//!
//! Every alignment in one folder is comparable to every other because they were all measured
//! against the same bases. So the fingerprint of those bases rides along with each run
//! ([`ref_hash_of`]), and the moment the folder's copy of a map changes, the runs measured against
//! the old bases say so ([`run_stale`]). The test is on the SEQUENCE, never on the id or the name.

use serde::{Deserialize, Serialize};

use super::names::match_names;

/// One reference map held by a verification folder: the copy kept in it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VerificationMap {
    pub construct_id: String,
    pub added_at: String,
}

/// What a person decided about one alignment, kept apart from what the app found.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Call {
    Good,
    Bad,
    Unsure,
}

impl Call {
    /// The word for a call, as the keys on the alignment say it.
    pub fn word(self) -> &'static str {
        match self {
            Call::Good => "good",
            Call::Bad => "not good",
            Call::Unsure => "cannot tell",
        }
    }

    pub fn parse(text: &str) -> Option<Call> {
        match text {
            "good" => Some(Call::Good),
            "bad" => Some(Call::Bad),
            "unsure" => Some(Call::Unsure),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Call::Good => "good",
            Call::Bad => "bad",
            Call::Unsure => "unsure",
        }
    }
}

/// One aligned read file, as the panel and the board draw it. Small on purpose: it rides along
/// with the folder's record, and the alignment behind it is read only when someone opens it.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RunSummary {
    /// The sample's name: a single file's own name, or a pair's shared stem.
    pub name: String,
    /// The folder's map these reads were aligned against. Absent on runs written when a folder held
    /// one map; read through [`FolderMeta::map_id_of`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub map_id: Option<String>,
    /// The stored read files this run was aligned from, by their original names.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub read_files: Option<Vec<String>>,
    pub added_at: String,
    pub total_reads: u64,
    pub mapped_reads: u64,
    pub mean_depth: f64,
    /// Fraction of the map with any coverage at all, 0..1.
    pub breadth: f64,
    /// How many differences were called.
    pub changes: u64,
    /// The one-line verdict the dot is coloured by.
    pub verdict: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub median_read_length: Option<u64>,
    pub ref_name: String,
    pub ref_length: u64,
    /// FNV-1a of the map's bases at alignment time, 8 hex digits.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ref_hash: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub call: Option<Call>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub called_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub called_by: Option<String>,
}

/// One read file kept in the folder, for as long as the folder is kept.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StoredReads {
    pub name: String,
    pub bytes: u64,
    pub added_at: String,
}

/// A sample on the board that has not run yet: its files, and where it sits.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StagedSample {
    pub name: String,
    /// R1 first.
    pub files: Vec<String>,
    /// The map it is placed on, or `None` for the tray.
    #[serde(default)]
    pub map_id: Option<String>,
    /// A person put it here (including "in the tray, on purpose"). The matcher never moves it.
    #[serde(default)]
    pub hand: bool,
}

/// `<folder>/.bios/folder.json` of a verification folder.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FolderMeta {
    /// Schema version, 1.
    pub v: u32,
    /// Always `verification` here.
    #[serde(rename = "type")]
    pub kind: String,
    pub created_at: String,
    /// The one map, before folders could hold several. Read through [`FolderMeta::maps`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub map_construct_id: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub maps: Vec<VerificationMap>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub runs: Vec<RunSummary>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub reads: Vec<StoredReads>,
    /// Bios's addition: the board's samples that have not run.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub staged: Vec<StagedSample>,
}

impl FolderMeta {
    pub fn new(created_at: &str) -> Self {
        FolderMeta {
            v: 1,
            kind: "verification".into(),
            created_at: created_at.into(),
            map_construct_id: None,
            maps: Vec::new(),
            runs: Vec::new(),
            reads: Vec::new(),
            staged: Vec::new(),
        }
    }

    /// The folder's maps, however the record was written (`mapsOf`).
    pub fn maps(&self) -> Vec<VerificationMap> {
        if !self.maps.is_empty() {
            return self.maps.clone();
        }
        self.map_construct_id.iter().map(|id| VerificationMap { construct_id: id.clone(), added_at: self.created_at.clone() }).collect()
    }

    /// Which map a run was measured against (`mapIdOf`): its own, or the folder's first.
    pub fn map_id_of(&self, run: &RunSummary) -> Option<String> {
        run.map_id.clone().or_else(|| self.maps().first().map(|m| m.construct_id.clone()))
    }

    /// The run of `name` against `map`.
    pub fn run(&self, name: &str, map: Option<&str>) -> Option<&RunSummary> {
        self.runs.iter().find(|r| r.name == name && self.map_id_of(r).as_deref() == map)
    }

    /// Decides where the loose samples go, by the matcher, for those nobody has placed by hand.
    ///
    /// All samples at once: the plate token is found by looking across every name. A sample the
    /// matcher already placed is left alone, so a new map does not re-deal the board.
    pub fn place_loose(&mut self, maps: &[(String, String)]) {
        let loose: Vec<usize> = self.staged.iter().enumerate().filter(|(_, s)| !s.hand && s.map_id.is_none()).map(|(i, _)| i).collect();
        if loose.is_empty() || maps.is_empty() {
            return;
        }
        let names: Vec<String> = self.staged.iter().map(|s| s.name.clone()).collect();
        let guess = match_names(&names, maps);
        for i in loose {
            if let Some(s) = self.staged.get_mut(i) {
                s.map_id = guess.get(&s.name).and_then(|g| g.map_id.clone());
            }
        }
    }

    /// The tray's button (`tryMatch`): another go at everything loose, including chips taken off a
    /// row by hand. What it places counts as placed by hand.
    pub fn try_match(&mut self, maps: &[(String, String)]) -> usize {
        let loose: Vec<String> = self.staged.iter().filter(|s| s.map_id.is_none()).map(|s| s.name.clone()).collect();
        if loose.is_empty() || maps.is_empty() {
            return 0;
        }
        let guess = match_names(&loose, maps);
        let mut placed = 0;
        for s in self.staged.iter_mut().filter(|s| s.map_id.is_none()) {
            if let Some(to) = guess.get(&s.name).and_then(|g| g.map_id.clone()) {
                s.map_id = Some(to);
                s.hand = true;
                placed += 1;
            }
        }
        placed
    }
}

/// FNV-1a over the uppercased bases, 32-bit, as 8 hex digits: a fingerprint, not a security hash.
pub fn ref_hash_of(bases: &str) -> String {
    let mut h: u32 = 0x811c_9dc5;
    for unit in bases.to_uppercase().encode_utf16() {
        h ^= u32::from(unit);
        h = h.wrapping_mul(0x0100_0193);
    }
    format!("{h:08x}")
}

/// Was this run aligned against the bases its map holds now? Hash first when both sides have
/// one; the length otherwise. No map at all is not "stale", it is a different problem.
pub fn run_stale(ref_length: u64, ref_hash: Option<&str>, map: Option<&str>) -> bool {
    let Some(bases) = map else { return false };
    match ref_hash {
        Some(hash) => hash != ref_hash_of(bases),
        None => ref_length != bases.chars().count() as u64,
    }
}

/// A name, as a folder name: illegal characters become spaces, control characters go, leading and
/// trailing dots and spaces go, at most 64 characters, `fallback` when nothing is left.
pub fn folder_name_for(name: &str, fallback: &str) -> String {
    let replaced: String = name
        .chars()
        .filter(|c| !(('\u{0}'..='\u{1f}').contains(c) || *c == '\u{7f}'))
        .map(|c| if "/\\:*?\"<>|".contains(c) { ' ' } else { c })
        .collect();
    let collapsed = replaced.split_whitespace().collect::<Vec<_>>().join(" ");
    let trim =
        |s: &str| s.trim_start_matches(|c: char| c == '.' || c.is_whitespace()).trim_end_matches(|c: char| c == '.' || c.is_whitespace()).to_string();
    let cut: String = trim(&collapsed).chars().take(64).collect();
    let out = cut.trim_end_matches(|c: char| c == '.' || c.is_whitespace()).to_string();
    if out.is_empty() { fallback.to_string() } else { out }
}

const MONTHS: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];

/// `2 Sep`: a run's default name. Not localised: a folder name is an address, not a label.
pub fn folder_stamp(day: u32, month0: u32) -> String {
    format!("{day} {}", MONTHS.get(month0 as usize).copied().unwrap_or("Jan"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_ordinary_plasmid_name_comes_through_untouched() {
        assert_eq!(folder_name_for("pUC19", "verification"), "pUC19");
        assert_eq!(folder_name_for("Round 2 (AAV)", "verification"), "Round 2 (AAV)");
    }

    #[test]
    fn the_characters_a_folder_cannot_hold_are_replaced_not_refused() {
        assert_eq!(folder_name_for("pUC19 (v2): round 3", "verification"), "pUC19 (v2) round 3");
        assert_eq!(folder_name_for("a/b", "verification"), "a b");
        assert_eq!(folder_name_for("a\\b", "verification"), "a b");
        assert_eq!(folder_name_for("a*b?c\"d<e>f|g", "verification"), "a b c d e f g");
    }

    #[test]
    fn leading_and_trailing_dots_and_spaces_go() {
        assert_eq!(folder_name_for("  .hidden.  ", "verification"), "hidden");
        assert_eq!(folder_name_for("trailing.", "verification"), "trailing");
    }

    #[test]
    fn a_name_of_nothing_usable_still_yields_a_folder_name() {
        for bad in ["", "   ", "...", "///", ":*?"] {
            assert_eq!(folder_name_for(bad, "verification"), "verification", "{bad:?}");
        }
    }

    #[test]
    fn it_never_exceeds_64_and_never_ends_in_a_space() {
        assert_eq!(folder_name_for(&"x".repeat(200), "verification").chars().count(), 64);
        let trimmed = folder_name_for(&format!("{} tail", "y".repeat(63)), "verification");
        assert!(!trimmed.ends_with(' '));
    }

    #[test]
    fn the_stamp_is_short_and_not_localised() {
        assert_eq!(folder_stamp(2, 8), "2 Sep");
        assert_eq!(folder_stamp(25, 11), "25 Dec");
    }

    #[test]
    fn ref_hash_is_deterministic_and_case_insensitive() {
        assert_eq!(ref_hash_of("ACGTACGT"), ref_hash_of("acgtACGT"));
        assert_eq!(ref_hash_of("ACGT").len(), 8);
        assert!(ref_hash_of("ACGT").chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn one_changed_base_is_a_different_hash() {
        assert_ne!(ref_hash_of("ACGTACGTAC"), ref_hash_of("ACGTACGTAG"));
    }

    #[test]
    fn ref_hash_matches_the_web_fnv1a() {
        // FNV-1a 32 of "A" (0x41) is 0xc40bf6cc.
        assert_eq!(ref_hash_of("a"), "c40bf6cc");
    }

    #[test]
    fn run_stale_follows_the_hash_then_the_length() {
        let map = "ACGTACGTAC";
        let hash = ref_hash_of(map);
        assert!(!run_stale(10, Some(&hash), Some(map)));
        assert!(run_stale(10, Some(&ref_hash_of("ACGTACGTAG")), Some(map)));
        assert!(!run_stale(10, None, Some(map)));
        assert!(run_stale(11, None, Some(map)));
        assert!(!run_stale(11, Some("deadbeef"), None));
    }

    #[test]
    fn a_folder_written_with_one_map_reads_as_one_map() {
        let mut meta = FolderMeta::new("2026-09-06T00:00:00Z");
        meta.map_construct_id = Some("old".into());
        assert_eq!(meta.maps()[0].construct_id, "old");
        let run = RunSummary { name: "A1".into(), ..Default::default() };
        assert_eq!(meta.map_id_of(&run).as_deref(), Some("old"));
    }

    #[test]
    fn the_web_record_reads_back_unchanged() {
        let text = r#"{"v":1,"type":"verification","createdAt":"2026-09-06T00:00:00Z","maps":[{"constructId":"a","addedAt":"x"}],"runs":[{"name":"A1","mapId":"a","addedAt":"y","totalReads":10,"mappedReads":9,"meanDepth":3.5,"breadth":1.0,"changes":0,"verdict":"consensus matches","refName":"p","refLength":100,"refHash":"0011aabb","call":"good"}]}"#;
        let meta: FolderMeta = serde_json::from_str(text).unwrap();
        assert_eq!(meta.runs[0].call, Some(Call::Good));
        let back: serde_json::Value = serde_json::to_value(&meta).unwrap();
        assert_eq!(back, serde_json::from_str::<serde_json::Value>(text).unwrap());
    }

    #[test]
    fn loose_samples_slide_under_the_map_their_name_carries_and_hand_placed_ones_stay() {
        let mut meta = FolderMeta::new("t");
        let s = |name: &str, map: Option<&str>, hand: bool| StagedSample { name: name.into(), files: vec![], map_id: map.map(String::from), hand };
        meta.staged = vec![s("QRS_OLG7_aZQ1_A1_S1", None, false), s("QRS_OLG8_aZQ1_A2_S2", None, true), s("QRS_OLG8_aZQ1_A3_S3", None, false)];
        let maps = vec![("m7".to_string(), "QRS_OLG7".to_string()), ("m8".to_string(), "QRS_OLG8".to_string())];
        meta.place_loose(&maps);
        assert_eq!(meta.staged[0].map_id.as_deref(), Some("m7"));
        assert_eq!(meta.staged[1].map_id, None, "a chip put in the tray by hand stays there");
        assert_eq!(meta.staged[2].map_id.as_deref(), Some("m8"));
        assert_eq!(meta.try_match(&maps), 1);
        assert_eq!(meta.staged[1].map_id.as_deref(), Some("m8"));
    }
}
