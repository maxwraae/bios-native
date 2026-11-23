//! `candidate_ranking.py`: acceptance, deduplication, ranking and mapping quality of refined
//! candidates. The original links tied candidates through a `_next_tie` attribute and shares
//! objects between its best pointer and its candidate list; here they live in one arena.

use crate::MapError;
use crate::primitives::{FLOAT_MAX, INT_MAX, JavaRandom, float32};
use crate::py::py_mod;

/// A candidate before ranking: the refined alignment and its strand.
#[derive(Clone, Debug, PartialEq)]
pub struct Refined {
    pub reference_id: i64,
    pub query_start: i64,
    pub reference_start: i64,
    pub alignment_length: i64,
    pub reverse: bool,
    pub score: f64,
    pub mismatch_count: i64,
    pub query_gap_positions: Vec<i64>,
    pub ref_gap_positions: Vec<i64>,
    pub query_gap_sizes: Vec<i64>,
    pub ref_gap_sizes: Vec<i64>,
}

/// One read hit (`RankedAlignment`).
#[derive(Clone, Debug, PartialEq)]
pub struct RankedAlignment {
    pub reference_id: i64,
    pub query_start: i64,
    pub reference_start: i64,
    pub alignment_length: i64,
    pub reverse: bool,
    pub score: f64,
    pub mismatch_count: i64,
    pub query_gap_positions: Vec<i64>,
    pub ref_gap_positions: Vec<i64>,
    pub query_gap_sizes: Vec<i64>,
    pub ref_gap_sizes: Vec<i64>,
    pub second_best_errors: i64,
    pub other_best_count: i64,
}

impl RankedAlignment {
    fn from_raw(raw: &Refined) -> Self {
        RankedAlignment {
            reference_id: raw.reference_id,
            query_start: raw.query_start,
            reference_start: raw.reference_start,
            alignment_length: raw.alignment_length,
            reverse: raw.reverse,
            score: float32(raw.score),
            mismatch_count: raw.mismatch_count,
            query_gap_positions: raw.query_gap_positions.clone(),
            ref_gap_positions: raw.ref_gap_positions.clone(),
            query_gap_sizes: raw.query_gap_sizes.clone(),
            ref_gap_sizes: raw.ref_gap_sizes.clone(),
            second_best_errors: INT_MAX,
            other_best_count: 0,
        }
    }

    pub fn query_span(&self) -> i64 {
        self.alignment_length - self.query_gap_sizes.iter().sum::<i64>()
    }

    pub fn reference_span(&self) -> i64 {
        self.alignment_length - self.ref_gap_sizes.iter().sum::<i64>()
    }

    pub fn errors(&self) -> i64 {
        self.mismatch_count + self.query_gap_sizes.iter().sum::<i64>() + self.ref_gap_sizes.iter().sum::<i64>()
    }

    fn errors_with_unaligned(&self, query_length: i64) -> i64 {
        self.errors() + query_length - self.query_span()
    }

    pub fn mapping_quality(&self) -> i64 {
        if self.other_best_count != 0 {
            if self.other_best_count == 1 {
                return 3;
            }
            if self.other_best_count == 2 {
                return 2;
            }
            return if self.other_best_count <= 8 { 1 } else { 0 };
        }
        if self.second_best_errors == INT_MAX {
            return 254;
        }
        let difference = self.second_best_errors - self.errors();
        if difference <= 0 { 3 } else { (difference * 20).min(254) }
    }

    /// Native `c.equals` deliberately leaves out the second-best errors.
    fn same_key(&self, other: &RankedAlignment) -> bool {
        self.reference_id == other.reference_id
            && self.query_start == other.query_start
            && self.reference_start == other.reference_start
            && self.alignment_length == other.alignment_length
            && self.reverse == other.reverse
            && self.score == other.score
            && self.mismatch_count == other.mismatch_count
            && self.query_gap_positions == other.query_gap_positions
            && self.ref_gap_positions == other.ref_gap_positions
            && self.query_gap_sizes == other.query_gap_sizes
            && self.ref_gap_sizes == other.ref_gap_sizes
            && self.other_best_count == other.other_best_count
    }

    fn gap_position_weight(&self) -> i64 {
        self.query_gap_positions.iter().zip(&self.query_gap_sizes).map(|(p, n)| p * n).sum::<i64>()
            + self.ref_gap_positions.iter().zip(&self.ref_gap_sizes).map(|(p, n)| p * n).sum::<i64>()
    }
}

/// Native equivalence: either common query end with both reference ends, or the reverse.
fn same_boundaries(first: &RankedAlignment, second: &RankedAlignment) -> bool {
    if first.reverse != second.reverse {
        return false;
    }
    let query_start_equal = first.query_start == second.query_start;
    let query_end_equal = first.query_start + first.query_span() == second.query_start + second.query_span();
    let ref_start_equal = first.reference_id == second.reference_id && first.reference_start == second.reference_start;
    let ref_end_equal = first.reference_id == second.reference_id
        && first.reference_start + first.reference_span() == second.reference_start + second.reference_span();
    if query_start_equal && query_end_equal && (ref_start_equal || ref_end_equal) {
        return true;
    }
    ref_start_equal && ref_end_equal && (query_start_equal || query_end_equal)
}

fn percent_allowance(length: i64, percent: i64) -> i64 {
    if percent > 0 { (percent * length / 100).max(1) } else { 0 }
}

/// Every candidate object of one `rank_candidates` call, with its tie link.
struct Arena {
    items: Vec<RankedAlignment>,
    next_tie: Vec<Option<usize>>,
}

impl Arena {
    fn push(&mut self, item: RankedAlignment) -> usize {
        self.items.push(item);
        self.next_tie.push(None);
        self.items.len() - 1
    }
}

pub struct RankOptions {
    pub max_candidates: i64,
    pub score_window: i64,
    pub minimum_score: f64,
}

struct Ranker<'a> {
    query_length: i64,
    reference_length: i64,
    circular: bool,
    options: &'a RankOptions,
    minimum_score: f64,
    use_rng: bool,
    candidates: Vec<usize>,
    best: Option<usize>,
    worst_score: f64,
    best_score: f64,
}

impl Ranker<'_> {
    fn accept(&mut self, arena: &mut Arena, raw: &Refined, rng: &mut JavaRandom) -> Result<bool, MapError> {
        let mut candidate = RankedAlignment::from_raw(raw);
        if candidate.reference_id != 0 {
            return Err(MapError::new("This ranker currently models a single reference"));
        }
        if self.circular {
            candidate.reference_start = py_mod(candidate.reference_start, self.reference_length);
            if candidate.reference_span() > self.reference_length {
                return Err(MapError::new("Circular multi-turn postprocessing requires structural jump records"));
            }
        }
        let gap_events = (candidate.query_gap_sizes.len() + candidate.ref_gap_sizes.len()) as i64;
        if candidate.mismatch_count + gap_events > percent_allowance(candidate.alignment_length, 30)
            || gap_events > percent_allowance(candidate.alignment_length, 15)
        {
            return Ok(false);
        }
        if candidate.score <= 0.0
            || candidate.score == FLOAT_MAX
            || !candidate.score.is_finite()
            || candidate.score < self.minimum_score
            || self.best_score - candidate.score > self.options.score_window as f64
        {
            return Ok(false);
        }
        let different_boundaries = match self.best {
            Some(b) => !same_boundaries(&arena.items[b], &candidate),
            None => false,
        };
        if let Some(best) = self.best {
            candidate.second_best_errors =
                if different_boundaries { arena.items[best].errors_with_unaligned(self.query_length) } else { arena.items[best].second_best_errors };
            let mut other = Some(best);
            while let Some(o) = other {
                if candidate.same_key(&arena.items[o]) {
                    return Ok(false);
                }
                other = arena.next_tie[o];
            }
            let mut previous: Option<usize> = None;
            let mut other = Some(best);
            while let Some(o) = other {
                if candidate.score == arena.items[o].score && same_boundaries(&candidate, &arena.items[o]) {
                    if candidate.gap_position_weight() > arena.items[o].gap_position_weight() {
                        // A copy resets its tie link, then this branch restores the tail.
                        let updated = arena.push(candidate.clone());
                        arena.next_tie[updated] = arena.next_tie[o];
                        match previous {
                            None => self.best = Some(updated),
                            Some(p) => arena.next_tie[p] = Some(updated),
                        }
                    }
                    return Ok(false);
                }
                previous = Some(o);
                other = arena.next_tie[o];
            }
        }
        if let Some(best) = self.best
            && different_boundaries
        {
            let e = candidate.errors_with_unaligned(self.query_length);
            if e < arena.items[best].second_best_errors {
                arena.items[best].second_best_errors = e;
            }
        }
        let stored = arena.push(candidate);
        let mut becomes_best = match self.best {
            None => true,
            Some(b) => arena.items[stored].score > arena.items[b].score,
        };
        if let Some(best) = self.best
            && !becomes_best
            && self.use_rng
            && arena.items[stored].score == arena.items[best].score
        {
            arena.items[best].other_best_count += 1;
            arena.items[stored].other_best_count = arena.items[best].other_best_count;
            if rng.next_int(arena.items[stored].other_best_count + 1)? == 0 {
                becomes_best = true;
                arena.next_tie[stored] = Some(best);
            } else {
                arena.next_tie[stored] = arena.next_tie[best];
                arena.next_tie[best] = Some(stored);
            }
        }
        if becomes_best {
            self.best = Some(stored);
        }
        if self.options.max_candidates == 1 {
            if let Some(b) = self.best {
                self.worst_score = arena.items[b].score;
                self.best_score = arena.items[b].score;
            }
            return Ok(true);
        }

        let mut matched_index = None;
        let mut changed = false;
        for (index, &existing) in self.candidates.iter().enumerate() {
            if same_boundaries(&arena.items[stored], &arena.items[existing]) {
                matched_index = Some(index);
                if arena.items[stored].score > arena.items[existing].score {
                    self.candidates[index] = stored;
                    changed = true;
                }
                break;
            }
        }
        if matched_index.is_none() && self.candidates.len() as i64 == self.options.max_candidates {
            let mut worst = 0;
            for index in 1..self.candidates.len() {
                if arena.items[self.candidates[index]].score < arena.items[self.candidates[worst]].score {
                    worst = index;
                }
            }
            if worst + 1 < self.candidates.len() {
                let last = self.candidates[self.candidates.len() - 1];
                self.candidates[worst] = last;
            }
            self.candidates.pop();
            changed = true;
        }
        if changed {
            self.worst_score = self
                .candidates
                .iter()
                .map(|&c| arena.items[c].score)
                .fold(None, |m: Option<f64>, s| Some(m.map_or(s, |m| m.min(s))))
                .unwrap_or(FLOAT_MAX);
            self.best_score = self
                .candidates
                .iter()
                .map(|&c| arena.items[c].score)
                .fold(None, |m: Option<f64>, s| Some(m.map_or(s, |m| m.max(s))))
                .unwrap_or(-999999.0);
        }
        if matched_index.is_none() {
            self.worst_score = self.worst_score.min(arena.items[stored].score);
            self.best_score = self.best_score.max(arena.items[stored].score);
            self.candidates.push(stored);
        }
        Ok(matched_index.is_none() || changed)
    }

    fn finish(mut self, arena: &Arena) -> Vec<usize> {
        if self.options.max_candidates == 1 {
            return self.best.into_iter().collect();
        }
        // Java Collections.sort is stable, and c.compareTo compares score only.
        self.candidates.sort_by(|&a, &b| arena.items[b].score.partial_cmp(&arena.items[a].score).unwrap_or(std::cmp::Ordering::Equal));
        self.candidates
    }
}

/// Ranks refined candidates in forward-then-reverse order. `rng` is only drawn on when a single
/// candidate is asked for.
pub fn rank_candidates(
    candidates: &[Refined],
    query_length: i64,
    reference_length: i64,
    circular: bool,
    options: &RankOptions,
    rng: &mut JavaRandom,
) -> Result<Vec<RankedAlignment>, MapError> {
    if !(1..=INT_MAX).contains(&options.max_candidates) {
        return Err(MapError::new("max_candidates must be a positive Java int"));
    }
    if options.score_window < 0 {
        return Err(MapError::new("score_window must be a nonnegative integer"));
    }
    let mut arena = Arena { items: Vec::new(), next_tie: Vec::new() };
    let mut ranked: Vec<usize> = Vec::new();
    let mut previous_strand = false;
    let mut start = 0;
    while start < candidates.len() {
        let strand = candidates[start].reverse;
        if previous_strand && !strand {
            return Err(MapError::new("Candidates must be ordered by forward then reverse strand"));
        }
        previous_strand = strand;
        let mut end = start;
        while end < candidates.len() && candidates[end].reverse == strand {
            end += 1;
        }
        let best = ranked.first().copied();
        let worst_score =
            ranked.iter().map(|&c| arena.items[c].score).fold(None, |m: Option<f64>, s| Some(m.map_or(s, |m| m.min(s)))).unwrap_or(FLOAT_MAX);
        let best_score =
            ranked.iter().map(|&c| arena.items[c].score).fold(None, |m: Option<f64>, s| Some(m.map_or(s, |m| m.max(s)))).unwrap_or(-9999999.0);
        let mut ranker = Ranker {
            query_length,
            reference_length,
            circular,
            options,
            minimum_score: float32(options.minimum_score),
            use_rng: options.max_candidates == 1,
            candidates: ranked.clone(),
            best,
            worst_score,
            best_score,
        };
        for candidate in &candidates[start..end] {
            ranker.accept(&mut arena, candidate, rng)?;
        }
        ranked = ranker.finish(&arena);
        start = end;
    }
    let Some(&first) = ranked.first() else {
        return Ok(Vec::new());
    };
    let best_score = arena.items[first].score;
    let mut output: Vec<RankedAlignment> = Vec::new();
    for &c in &ranked {
        let candidate = &arena.items[c];
        if !output.iter().any(|o| o.same_key(candidate)) && best_score - candidate.score <= options.score_window as f64 {
            output.push(candidate.clone());
        }
    }
    Ok(output)
}
