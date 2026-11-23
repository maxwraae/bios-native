//! Reading a stored run: its dot, its word, its line on the board, and the order a person calls a
//! plate in. Ported from `layout/sessionConstructs.ts` (`runTone`, `runNote`), `verify/boxOrder.ts`
//! and `verify/callOrder.ts`.
//!
//! A PERSON'S CALL WINS on the screen, and both records are kept: what the app found (`verdict`)
//! and what somebody decided (`call`). Nothing derives one from the other.

use std::cmp::Ordering;

use super::folder::{Call, FolderMeta, RunSummary};
use super::{grouped, natural_cmp};

/// What the app found, as the dot's colour.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tone {
    /// The reads agree with the map.
    Ok,
    /// They differ from it.
    Changed,
    /// Too little of it covered to say.
    Gaps,
    /// Not this map at all.
    Unmapped,
}

impl Tone {
    pub fn as_str(self) -> &'static str {
        match self {
            Tone::Ok => "ok",
            Tone::Changed => "changed",
            Tone::Gaps => "gaps",
            Tone::Unmapped => "unmapped",
        }
    }
}

fn is_count_of(verdict: &str, words: &[&str]) -> bool {
    words.iter().any(|w| verdict.strip_suffix(w).is_some_and(|n| !n.is_empty() && n.chars().all(|c| c.is_ascii_digit())))
}

/// Median read length below which the aligner has nothing to seed with (`seeding.ts`).
pub const SEEDABLE_BP: u64 = 60;

fn seedable(s: &RunSummary) -> bool {
    let median = s.median_read_length.unwrap_or(0);
    !(median > 0 && median < SEEDABLE_BP)
}

/// The dot (`runTone`).
pub fn run_tone(s: &RunSummary) -> Tone {
    match s.verdict.as_str() {
        "partial coverage" | "incomplete coverage" => return Tone::Gaps,
        "consensus matches" | "no variants called" => return Tone::Ok,
        "no reads aligned" => return Tone::Unmapped,
        v if is_count_of(v, &[" consensus differences"]) => return Tone::Changed,
        _ => {}
    }
    if s.total_reads == 0 || s.mapped_reads == 0 || s.breadth < 0.5 {
        return Tone::Unmapped;
    }
    if s.changes > 0 {
        return Tone::Changed;
    }
    if s.breadth < 0.95 { Tone::Gaps } else { Tone::Ok }
}

/// The word beside a run in a narrow column (`runNote`). Shares the dot's floors, so the dot and
/// the words never disagree.
pub fn run_note(s: &RunSummary) -> String {
    let v = s.verdict.as_str();
    if matches!(v, "partial coverage" | "consensus matches" | "incomplete coverage" | "no variants called" | "no reads aligned")
        || is_count_of(v, &[" consensus differences", " differences", " difference"])
    {
        return v.to_string();
    }
    let mapped = if s.total_reads > 0 { s.mapped_reads as f64 / s.total_reads as f64 } else { 0.0 };
    if s.total_reads == 0 {
        return "no reads".into();
    }
    if mapped < 0.2 && !seedable(s) {
        return "reads too short".into();
    }
    if s.mapped_reads == 0 || s.breadth < 0.5 {
        return "not this map".into();
    }
    if s.changes > 0 {
        return format!("{} change{}", s.changes, if s.changes == 1 { "" } else { "s" });
    }
    if s.breadth < 0.95 { "gaps".into() } else { "matches".into() }
}

/// What a line on the board says.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum LineState {
    Good,
    Thin,
    Bad,
    Elsewhere,
    Waiting,
}

/// A person's call wins; otherwise what the app found (`stateOf`).
pub fn state_of(run: &RunSummary) -> LineState {
    if let Some(call) = run.call {
        return match call {
            Call::Good => LineState::Good,
            Call::Bad => LineState::Bad,
            Call::Unsure => LineState::Thin,
        };
    }
    match run_tone(run) {
        Tone::Ok => LineState::Good,
        Tone::Gaps => LineState::Thin,
        Tone::Changed => LineState::Bad,
        Tone::Unmapped => LineState::Elsewhere,
    }
}

/// The order of one box's lines: a stable partition, good first. Equal lines keep their order.
pub fn order_lines<T: Clone>(lines: &[(T, LineState)]) -> Vec<(T, LineState)> {
    let mut out = lines.to_vec();
    out.sort_by_key(|(_, s)| *s);
    out
}

/// Every answered sample under a map differing is a claim about the map, not the clones. Needs two.
pub fn box_fails_as_one(states: &[LineState]) -> bool {
    let answered: Vec<&LineState> = states.iter().filter(|s| !matches!(s, LineState::Waiting | LineState::Elsewhere)).collect();
    answered.len() >= 2 && answered.iter().all(|s| **s == LineState::Bad)
}

/// Did a person overrule the app? Only where the app reached an opinion.
pub fn disagrees(run: &RunSummary) -> bool {
    let Some(call) = run.call else { return false };
    let found = state_of(&RunSummary { call: None, ..run.clone() });
    if found == LineState::Elsewhere {
        return false;
    }
    if call == Call::Unsure { found != LineState::Thin } else { state_of(run) != found }
}

/// One line of numbers for a run, from the summary alone, floored (`summaryLine`).
pub fn summary_line(s: &RunSummary) -> String {
    if s.total_reads == 0 {
        return "no reads".into();
    }
    [
        format!("{} of {} reads", grouped(s.mapped_reads as i64), grouped(s.total_reads as i64)),
        format!("{}% mapped", (100 * s.mapped_reads) / s.total_reads),
        format!("{:.1}% covered", (1000.0 * s.breadth).floor() / 10.0),
        format!("{:.0}×", s.mean_depth),
        run_note(s),
    ]
    .join(" · ")
}

fn by_name(a: &RunSummary, b: &RunSummary) -> Ordering {
    natural_cmp(&a.name, &b.name)
}

/// Every run in the folder, map by map in the board's order, each map's files in chip order. A run
/// whose map has left the folder comes after every map that is here.
pub fn call_order(meta: &FolderMeta) -> Vec<RunSummary> {
    let maps = meta.maps();
    let rank = |r: &RunSummary| meta.map_id_of(r).and_then(|id| maps.iter().position(|m| m.construct_id == id)).unwrap_or(maps.len());
    let mut runs = meta.runs.clone();
    runs.sort_by(|a, b| rank(a).cmp(&rank(b)).then_with(|| by_name(a, b)));
    runs
}

/// Where clicking a map starts: its first file nobody has called, or its first file.
pub fn first_of_map(meta: &FolderMeta, map_id: &str) -> Option<RunSummary> {
    let here: Vec<RunSummary> = call_order(meta).into_iter().filter(|r| meta.map_id_of(r).as_deref() == Some(map_id)).collect();
    here.iter().find(|r| r.call.is_none()).or(here.first()).cloned()
}

/// Where a call takes you: the next uncalled file after `name` on `map`, on into the next map and
/// round to the top. `None` when every other file has a call. `called_here` names calls the
/// record has not caught up with yet.
pub fn next_to_call(meta: &FolderMeta, name: &str, map: Option<&str>, called_here: impl Fn(&RunSummary) -> bool) -> Option<RunSummary> {
    let order = call_order(meta);
    let map = map.map(str::to_string).or_else(|| meta.maps().first().map(|m| m.construct_id.clone()));
    let at = order.iter().position(|r| r.name == name && meta.map_id_of(r) == map);
    let rest: Vec<RunSummary> = match at {
        Some(i) => order[i + 1..].iter().chain(order[..i].iter()).cloned().collect(),
        None => order,
    };
    rest.into_iter().find(|r| r.call.is_none() && !called_here(r))
}

/// The list you take to the freezer: one line per file called good, its first file's name and its
/// map's name, tab-separated.
pub fn good_wells(meta: &FolderMeta, map_name: impl Fn(&str) -> Option<String>) -> String {
    meta.runs
        .iter()
        .filter(|r| r.call == Some(Call::Good))
        .map(|r| {
            let file = r.read_files.as_ref().and_then(|f| f.first()).cloned().unwrap_or_else(|| r.name.clone());
            let map = meta.map_id_of(r).and_then(|id| map_name(&id)).unwrap_or_else(|| r.ref_name.clone());
            format!("{file}\t{map}")
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod tests {
    use super::super::folder::VerificationMap;
    use super::*;

    fn run(name: &str) -> RunSummary {
        RunSummary {
            name: name.into(),
            added_at: "2026-09-05T00:00:00.000Z".into(),
            total_reads: 4000,
            mapped_reads: 3800,
            mean_depth: 80.0,
            breadth: 0.999,
            changes: 0,
            verdict: String::new(),
            ref_name: "pUC19".into(),
            ref_length: 2686,
            ..Default::default()
        }
    }

    fn with(name: &str, f: impl FnOnce(&mut RunSummary)) -> RunSummary {
        let mut r = run(name);
        f(&mut r);
        r
    }

    #[test]
    fn reads_that_agree_across_the_whole_plasmid_are_good() {
        assert_eq!(state_of(&run("A1")), LineState::Good);
    }

    #[test]
    fn a_difference_makes_it_bad_however_well_covered() {
        assert_eq!(state_of(&with("A1", |r| r.changes = 1)), LineState::Bad);
    }

    #[test]
    fn too_little_of_the_plasmid_covered_is_thin_not_bad() {
        assert_eq!(state_of(&with("A1", |r| r.breadth = 0.62)), LineState::Thin);
    }

    #[test]
    fn reads_that_cover_almost_none_of_the_map_are_not_about_it_at_all() {
        assert_eq!(
            state_of(&with("A1", |r| {
                r.mapped_reads = 200;
                r.breadth = 0.16;
                r.changes = 3;
            })),
            LineState::Elsewhere
        );
    }

    #[test]
    fn a_clone_that_covers_its_map_is_judged_on_its_changes_not_on_its_miniprep() {
        let d7 = with("D7", |r| {
            r.total_reads = 20414;
            r.mapped_reads = 3548;
            r.breadth = 0.895;
            r.changes = 16;
        });
        assert_eq!(state_of(&d7), LineState::Bad);
    }

    #[test]
    fn the_first_line_is_the_one_to_take() {
        let out = order_lines(&[("A1", LineState::Bad), ("B1", LineState::Thin), ("C1", LineState::Good), ("D1", LineState::Bad)]);
        assert_eq!(out[0].0, "C1");
    }

    #[test]
    fn all_five_states_come_out_in_their_order_of_usefulness() {
        let out = order_lines(&[
            ("e", LineState::Waiting),
            ("d", LineState::Elsewhere),
            ("c", LineState::Bad),
            ("b", LineState::Thin),
            ("a", LineState::Good),
        ]);
        assert_eq!(out.iter().map(|l| l.0).collect::<Vec<_>>(), vec!["a", "b", "c", "d", "e"]);
    }

    #[test]
    fn equal_lines_keep_the_order_they_arrived_in() {
        let out = order_lines(&[("A1", LineState::Bad), ("B1", LineState::Bad), ("C1", LineState::Bad), ("D1", LineState::Bad)]);
        assert_eq!(out.iter().map(|l| l.0).collect::<Vec<_>>(), vec!["A1", "B1", "C1", "D1"]);
        assert!(order_lines::<&str>(&[]).is_empty());
    }

    #[test]
    fn box_fails_as_one_is_about_the_map_not_the_clones() {
        use LineState::*;
        assert!(box_fails_as_one(&[Bad, Bad, Bad, Bad]));
        assert!(!box_fails_as_one(&[Bad]));
        assert!(!box_fails_as_one(&[Bad, Bad, Good]));
        assert!(!box_fails_as_one(&[Bad, Bad, Thin]));
        assert!(box_fails_as_one(&[Bad, Bad, Waiting, Waiting]));
        assert!(box_fails_as_one(&[Bad, Bad, Elsewhere]));
        assert!(!box_fails_as_one(&[Waiting, Waiting]));
    }

    #[test]
    fn a_call_wins_over_what_the_app_found() {
        assert_eq!(
            state_of(&with("A1", |r| {
                r.changes = 1;
                r.call = Some(Call::Good);
            })),
            LineState::Good
        );
        assert_eq!(state_of(&with("A1", |r| r.call = Some(Call::Bad))), LineState::Bad);
        assert_eq!(state_of(&with("A1", |r| r.call = Some(Call::Unsure))), LineState::Thin);
        assert_eq!(state_of(&with("A1", |r| r.changes = 1)), LineState::Bad);
    }

    #[test]
    fn the_overrule_is_findable_and_agreement_is_not_one() {
        assert!(disagrees(&with("A1", |r| {
            r.changes = 1;
            r.call = Some(Call::Good);
        })));
        assert!(!disagrees(&with("A1", |r| r.call = Some(Call::Good))));
        assert!(!disagrees(&with("A1", |r| {
            r.changes = 1;
            r.call = Some(Call::Bad);
        })));
        assert!(!disagrees(&with("A1", |r| {
            r.breadth = 0.6;
            r.call = Some(Call::Unsure);
        })));
        assert!(disagrees(&with("A1", |r| r.call = Some(Call::Unsure))));
        assert!(!disagrees(&with("A1", |r| {
            r.mapped_reads = 100;
            r.breadth = 0.12;
            r.call = Some(Call::Bad);
        })));
        assert!(!disagrees(&with("A1", |r| r.changes = 3)));
    }

    #[test]
    fn summary_line_says_the_numbers_the_header_says_floored() {
        assert_eq!(summary_line(&run("a")), "3,800 of 4,000 reads · 95% mapped · 99.9% covered · 80× · matches");
        assert_eq!(summary_line(&with("b", |r| r.changes = 2)), "3,800 of 4,000 reads · 95% mapped · 99.9% covered · 80× · 2 changes");
        assert_eq!(
            summary_line(&with("c", |r| {
                r.mapped_reads = 3999;
                r.breadth = 0.9996;
            })),
            "3,999 of 4,000 reads · 99% mapped · 99.9% covered · 80× · matches"
        );
        assert_eq!(
            summary_line(&with("d", |r| {
                r.total_reads = 0;
                r.mapped_reads = 0;
                r.breadth = 0.0;
            })),
            "no reads"
        );
    }

    #[test]
    fn the_mappers_three_verdicts_colour_the_dot() {
        assert_eq!(run_tone(&with("a", |r| r.verdict = "consensus matches".into())), Tone::Ok);
        assert_eq!(run_tone(&with("a", |r| r.verdict = "3 consensus differences".into())), Tone::Changed);
        assert_eq!(run_tone(&with("a", |r| r.verdict = "partial coverage".into())), Tone::Gaps);
        assert_eq!(run_note(&with("a", |r| r.verdict = "3 consensus differences".into())), "3 consensus differences");
    }

    fn plate_run(name: &str, map: Option<&str>, call: Option<Call>) -> RunSummary {
        RunSummary { name: name.into(), map_id: map.map(String::from), call, ..Default::default() }
    }

    fn folder(runs: Vec<RunSummary>) -> FolderMeta {
        let mut meta = FolderMeta::new("2026-09-06T00:00:00Z");
        meta.maps = ["tpi", "ef1a"].iter().map(|id| VerificationMap { construct_id: id.to_string(), added_at: "x".into() }).collect();
        meta.runs = runs;
        meta
    }

    fn plate() -> FolderMeta {
        folder(vec![
            plate_run("F1_S61", Some("ef1a"), None),
            plate_run("A10_S10", Some("tpi"), None),
            plate_run("E3_S51", Some("ef1a"), None),
            plate_run("A7_S7", Some("tpi"), Some(Call::Good)),
            plate_run("A8_S8", Some("tpi"), None),
            plate_run("E4_S52", Some("ef1a"), None),
        ])
    }

    fn names(runs: &[RunSummary]) -> Vec<&str> {
        runs.iter().map(|r| r.name.as_str()).collect()
    }

    #[test]
    fn call_order_is_the_board() {
        assert_eq!(names(&call_order(&plate())), vec!["A7_S7", "A8_S8", "A10_S10", "E3_S51", "E4_S52", "F1_S61"]);
        let meta = folder(vec![plate_run("B1", Some("gone"), None), plate_run("A1", Some("ef1a"), None)]);
        assert_eq!(names(&call_order(&meta)), vec!["A1", "B1"]);
    }

    #[test]
    fn clicking_a_map_opens_its_first_uncalled_file() {
        assert_eq!(first_of_map(&plate(), "tpi").map(|r| r.name), Some("A8_S8".into()));
        assert_eq!(first_of_map(&plate(), "ef1a").map(|r| r.name), Some("E3_S51".into()));
        let meta = folder(vec![plate_run("A2", Some("tpi"), Some(Call::Bad)), plate_run("A1", Some("tpi"), Some(Call::Good))]);
        assert_eq!(first_of_map(&meta, "tpi").map(|r| r.name), Some("A1".into()));
        assert_eq!(first_of_map(&meta, "ef1a"), None);
    }

    #[test]
    fn yes_no_yes_walks_the_map_then_the_next_then_says_the_plate_is_done() {
        let mut meta = plate();
        let mut walked = Vec::new();
        let mut at = first_of_map(&meta, "tpi");
        while let Some(here) = at {
            walked.push(here.name.clone());
            for r in meta.runs.iter_mut().filter(|r| r.name == here.name) {
                r.call = Some(Call::Good);
            }
            at = next_to_call(&meta, &here.name, here.map_id.as_deref(), |_| false);
        }
        assert_eq!(walked, vec!["A8_S8", "A10_S10", "E3_S51", "E4_S52", "F1_S61"]);
    }

    #[test]
    fn the_last_file_goes_round_to_one_you_skipped() {
        assert_eq!(next_to_call(&plate(), "F1_S61", Some("ef1a"), |_| false).map(|r| r.name), Some("A8_S8".into()));
        assert_eq!(next_to_call(&plate(), "F1_S61", Some("ef1a"), |r| r.name == "A8_S8").map(|r| r.name), Some("A10_S10".into()));
    }

    #[test]
    fn never_the_file_you_are_on_and_none_when_nothing_else_is_uncalled() {
        let meta = folder(vec![plate_run("A1", Some("tpi"), None), plate_run("A2", Some("tpi"), Some(Call::Good))]);
        assert_eq!(next_to_call(&meta, "A1", Some("tpi"), |_| false), None);
    }

    #[test]
    fn a_run_stored_without_a_map_id_is_the_folders_first_maps() {
        let meta = folder(vec![plate_run("A1", None, None), plate_run("E1", Some("ef1a"), None)]);
        assert_eq!(next_to_call(&meta, "A1", None, |_| false).map(|r| r.name), Some("E1".into()));
    }

    #[test]
    fn the_picks_are_one_tab_separated_line_per_good_file() {
        let mut meta = plate();
        meta.runs[3].read_files = Some(vec!["A7_S7_R1_001.fastq.gz".into(), "A7_S7_R2_001.fastq.gz".into()]);
        let text = good_wells(&meta, |id| Some(format!("map {id}")));
        assert_eq!(text, "A7_S7_R1_001.fastq.gz\tmap tpi");
    }
}
