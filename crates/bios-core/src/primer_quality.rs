//! Advisory primer-quality signals for an oligo someone already chose: the checks a cloner looks
//! at before ordering. Not a primer designer; it only flags well-known liabilities, so the worst
//! case is a caution that can be ignored.
//!
//! Ported from the web app's `editor/primerQuality.ts`:
//! - 3' GC clamp: a G or C at the 3' end anchors extension; without one the primer is weak.
//! - GC-rich 3' end: four or more of the last five being G/C over-stabilises the end and misprimes.
//! - homopolymer: a run of five or more identical bases slips in synthesis and annealing.
//! - self-complementarity: the longest stretch matching the oligo's own reverse complement, a
//!   proxy for hairpins and self-dimers; six or more is worth a look.

/// What the checks found. `warnings` is empty for a clean oligo.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PrimerFlags {
    pub gc_clamp: bool,
    pub long_run: usize,
    pub self_comp: usize,
    pub warnings: Vec<String>,
}

const RUN_LIMIT: usize = 5;
const SELF_COMP_LIMIT: usize = 6;

fn complement(b: u8) -> u8 {
    match b {
        b'a' => b't',
        b't' => b'a',
        b'c' => b'g',
        b'g' => b'c',
        b'u' => b'a',
        other => other,
    }
}

/// The longest run of one base, and where it ends.
fn longest_run(s: &[u8]) -> (usize, usize) {
    let (mut best, mut at, mut run) = (0, 0, 0);
    for (i, b) in s.iter().enumerate() {
        run = if i > 0 && s.get(i - 1) == Some(b) { run + 1 } else { 1 };
        if run > best {
            best = run;
            at = i;
        }
    }
    (best, at)
}

/// The longest common substring of `a` and `b`, O(n·m): fine at oligo length.
fn longest_common_substring(a: &[u8], b: &[u8]) -> usize {
    let mut best = 0;
    let mut prev = vec![0usize; b.len() + 1];
    for x in a {
        let mut cur = vec![0usize; b.len() + 1];
        for (j, y) in b.iter().enumerate() {
            if x == y {
                let run = prev.get(j).copied().unwrap_or(0) + 1;
                if let Some(slot) = cur.get_mut(j + 1) {
                    *slot = run;
                }
                best = best.max(run);
            }
        }
        prev = cur;
    }
    best
}

/// The flags for `seq`. Letters other than A, C, G, T, U and N are ignored, in either case.
pub fn primer_flags(seq: &str) -> PrimerFlags {
    let seq: Vec<u8> = seq.bytes().map(|b| b.to_ascii_lowercase()).filter(|b| b"acgtun".contains(b)).collect();
    let n = seq.len();
    let mut warnings = Vec::new();
    let gc_clamp = matches!(seq.last(), Some(b'g' | b'c'));
    if n >= 3 && !gc_clamp {
        warnings.push("no 3' G/C clamp".to_string());
    }
    if n >= 5 {
        let tail = seq.get(n - 5..).unwrap_or_default();
        if tail.iter().filter(|b| matches!(b, b'g' | b'c')).count() >= 4 {
            warnings.push("GC-rich 3' end".to_string());
        }
    }
    let (long_run, at) = longest_run(&seq);
    if long_run >= RUN_LIMIT {
        let base = seq.get(at).map_or('N', |b| char::from(b.to_ascii_uppercase()));
        warnings.push(format!("{long_run}× {base} run"));
    }
    let reverse: Vec<u8> = seq.iter().rev().map(|b| complement(*b)).collect();
    let self_comp = longest_common_substring(&seq, &reverse);
    if self_comp >= SELF_COMP_LIMIT {
        warnings.push(format!("{self_comp}-bp self-complementarity"));
    }
    PrimerFlags { gc_clamp, long_run, self_comp, warnings }
}
