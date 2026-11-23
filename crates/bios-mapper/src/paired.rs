//! `initial_mapper.py` and `paired_mapper.py`: candidates for one read, and the paired core that
//! scores mate combinations and breaks ties with the pair's own generator.
//!
//! The original keeps a bounded cache of pre-ranking candidates; it never changes a result. Here
//! [`CandidateCache`] does the same for every pair of one pass, and a pair ranks its two reads'
//! candidates as often as the original asks.

use crate::MapError;
use crate::chaining::chain_candidates;
use crate::dna::reverse_complement;
use crate::kernel::{Kernel, RawForRefine};
use crate::primitives::{INT_MAX, JavaRandom, Location, float32, nucleotide_score, pair_distance_penalty, read_pair_seed};
use crate::ranking::{RankOptions, RankedAlignment, Refined, rank_candidates};
use crate::search::CandidateSearch;

/// The chained and refined candidates of one normalized query, in native order (no RNG used).
pub fn refine_candidates(query: &[u8], search: &CandidateSearch, kernel: &Kernel, special_search: bool) -> Result<Vec<Refined>, MapError> {
    let reversed = reverse_complement(query);
    let mut refined = Vec::new();
    for raw in chain_candidates(query, search, kernel, special_search)? {
        let oriented = if raw.reverse { &reversed } else { query };
        let result = kernel.refine_alignment(
            oriented,
            &search.reference,
            search.circular,
            &RawForRefine {
                score: raw.score,
                query_start: raw.query_start,
                reference_start: raw.reference_start,
                alignment_length: raw.alignment_length,
                query_gap_positions: &raw.query_gap_positions,
                ref_gap_positions: &raw.ref_gap_positions,
                query_gap_sizes: &raw.query_gap_sizes,
                ref_gap_sizes: &raw.ref_gap_sizes,
                mismatch_count: raw.mismatch_count,
            },
        )?;
        refined.push(Refined {
            reference_id: raw.reference_id,
            query_start: result.query_start,
            reference_start: result.reference_start,
            alignment_length: result.alignment_length,
            reverse: raw.reverse,
            score: result.score,
            mismatch_count: result.mismatch_count,
            query_gap_positions: result.query_gap_positions,
            ref_gap_positions: result.ref_gap_positions,
            query_gap_sizes: result.query_gap_sizes,
            ref_gap_sizes: result.ref_gap_sizes,
        });
    }
    Ok(refined)
}

/// The result for one pair, in normalized mate orientation (`PairedAlignment`).
#[derive(Clone, Debug, PartialEq)]
pub struct PairedAlignment {
    pub read: Option<RankedAlignment>,
    pub mate: Option<RankedAlignment>,
    pub score: f64,
    pub mapping_quality: i64,
}

fn orphan_score(read: Option<&RankedAlignment>, mate: Option<&RankedAlignment>) -> f64 {
    let mut score = 0.0;
    if let Some(r) = read {
        score = float32(score + r.score);
    }
    if let Some(m) = mate {
        score = float32(score + m.score);
    }
    1.0f64.max(float32(score - 60.0))
}

fn tie_quality(other_count: i64) -> i64 {
    match other_count {
        1 => 3,
        2 => 2,
        n if n <= 8 => 1,
        _ => 0,
    }
}

fn error_quality(best_errors: i64, second_errors: i64) -> i64 {
    if second_errors == INT_MAX {
        return 254;
    }
    let difference = second_errors - best_errors;
    if difference <= 0 { 3 } else { (difference * 20).min(254) }
}

fn location(hit: &RankedAlignment) -> Location {
    Location { reference: hit.reference_id, start: hit.reference_start, reference_span: hit.reference_span(), reverse: hit.reverse }
}

fn index(s: &[u8], i: i64) -> Result<u8, MapError> {
    let n = s.len() as i64;
    let j = if i < 0 { i + n } else { i };
    if j < 0 || j >= n {
        return Err(MapError::new("Index out of range in overhang scoring"));
    }
    Ok(s[j as usize])
}

/// The exact paired-overhang interval score, with affine gap costs.
fn alignment_interval_score(hit: &RankedAlignment, region_start: i64, region_end: i64, query: &[u8], reference: &[u8]) -> Result<f64, MapError> {
    let (gap_open, gap_extend) = (5.0, 6.0);
    let reversed;
    let oriented: &[u8] = if hit.reverse {
        reversed = reverse_complement(query);
        &reversed
    } else {
        query
    };
    let (mut qi, mut ri) = (hit.query_start, hit.reference_start);
    let (mut qgap_index, mut rgap_index, mut qgap_remaining, mut rgap_remaining) = (0usize, 0usize, 0i64, 0i64);
    let mut score = 0.0;
    let (mut previous_in_region, mut previous_gap) = (false, false);
    for column in 0..hit.alignment_length {
        let mut in_region = region_start <= ri && ri <= region_end;
        let mut is_gap = false;
        let mut query_base = b'-';
        let mut reference_base = b'-';
        if qgap_remaining > 0 {
            is_gap = true;
            qgap_remaining -= 1;
        } else if qgap_index < hit.query_gap_positions.len() && column == hit.query_gap_positions[qgap_index] {
            qgap_remaining = hit.query_gap_sizes.get(qgap_index).copied().unwrap_or(0) - 1;
            qgap_index += 1;
            if in_region {
                score = float32(score - gap_open);
            }
            is_gap = true;
        } else {
            query_base = index(oriented, qi)?;
            qi += 1;
        }
        if rgap_remaining > 0 {
            if ri == region_end + 1 {
                in_region = true;
            }
            is_gap = true;
            rgap_remaining -= 1;
        } else if rgap_index < hit.ref_gap_positions.len() && column == hit.ref_gap_positions[rgap_index] {
            if ri == region_end + 1 {
                in_region = true;
            }
            rgap_remaining = hit.ref_gap_sizes.get(rgap_index).copied().unwrap_or(0) - 1;
            rgap_index += 1;
            if in_region {
                score = float32(score - gap_open);
            }
            is_gap = true;
        } else {
            reference_base = index(reference, ri)?;
            ri += 1;
            if ri == reference.len() as i64 {
                ri = 0;
            }
        }
        if in_region {
            score = if is_gap { float32(score - gap_extend) } else { float32(score + nucleotide_score(query_base, reference_base)?) };
        }
        if in_region && !previous_in_region && previous_gap {
            score = float32(score - gap_open);
        }
        previous_in_region = in_region;
        previous_gap = is_gap;
    }
    Ok(score)
}

fn pair_overlap_score(
    read: &RankedAlignment,
    mate: &RankedAlignment,
    read_query: &[u8],
    mate_query: &[u8],
    reference: &[u8],
    extension: i64,
) -> Result<f64, MapError> {
    let (read, mate, read_query, mate_query) = if read.reverse { (mate, read, mate_query, read_query) } else { (read, mate, read_query, mate_query) };
    let read_end = read.reference_start + read.reference_span() - 1;
    let mate_end = mate.reference_start + mate.reference_span() - 1;
    let mut score = 0.0;
    if mate.reference_start < read.reference_start && mate_end >= read.reference_start + extension {
        score = float32(score + alignment_interval_score(mate, mate.reference_start, read.reference_start - 1, mate_query, reference)?);
    }
    if mate_end < read_end && read.reference_start <= mate_end - extension {
        score = float32(score + alignment_interval_score(read, mate_end + 1, read_end, read_query, reference)?);
    }
    Ok(score)
}

struct Combination<'a> {
    read: &'a RankedAlignment,
    mate: &'a RankedAlignment,
    penalty: f64,
    score: f64,
}

/// Native Cartesian pair scoring and tie resolution for two nonempty lists (MapAll, one result).
fn choose_paired_candidates(
    reads: &[RankedAlignment],
    mates: &[RankedAlignment],
    expected_distance: i64,
    circular_length: Option<i64>,
    rng: &mut JavaRandom,
    overlap_score_extension: i64,
    read_query: &[u8],
    mate_query: &[u8],
    reference: &[u8],
) -> Result<PairedAlignment, MapError> {
    if reads.is_empty() || mates.is_empty() {
        return Err(MapError::new("Pair scoring requires nonempty candidate lists"));
    }
    let mut combinations = Vec::with_capacity(reads.len() * mates.len());
    for read in reads {
        for mate in mates {
            let mut penalty = pair_distance_penalty(&location(read), &location(mate), expected_distance, circular_length)?;
            if penalty != crate::primitives::FLOAT_MAX {
                if overlap_score_extension > 0 && read.reverse == mate.reverse && read.reference_id == mate.reference_id {
                    penalty =
                        60.0f64.min(float32(penalty + pair_overlap_score(read, mate, read_query, mate_query, reference, overlap_score_extension)?));
                }
                let score = float32(float32(read.score + mate.score) - penalty);
                combinations.push(Combination { read, mate, penalty, score });
            }
        }
    }
    let mut best: Option<PairedAlignment> = None;
    let (mut best_score, mut second_score) = (-999999.0f64, -999999.0f64);
    let (mut ties, mut capped_ties) = (0i64, 0i64);
    let (mut best_errors, mut second_errors) = (INT_MAX, INT_MAX);
    for c in &combinations {
        let errors = c.read.errors() + c.mate.errors();
        if c.score < best_score && errors < second_errors {
            second_errors = errors;
        }
        if c.score < best_score && c.score > second_score {
            second_score = c.score;
        }
        if best.is_none() || c.score > best_score {
            second_score = best_score;
            best_score = c.score;
            best = Some(PairedAlignment { read: Some(c.read.clone()), mate: Some(c.mate.clone()), score: c.score, mapping_quality: 0 });
            second_errors = best_errors;
            ties = 0;
            capped_ties = 0;
            best_errors = errors;
        }
        if c.score == best_score {
            ties += 1;
            if c.penalty == 60.0 {
                capped_ties += 1;
            }
        }
    }
    let Some(mut best) = best else {
        return Ok(PairedAlignment { read: None, mate: None, score: 0.0, mapping_quality: 0 });
    };
    if ties > 1 {
        best.mapping_quality = tie_quality(ties - 1);
    } else {
        best.mapping_quality = error_quality(best_errors, second_errors);
        if best_errors >= second_errors {
            if second_score >= float32(best_score - 10.0) {
                best.mapping_quality += float32(float32(1.0 + best_score) - second_score) as i64;
            } else {
                best.mapping_quality += 20;
            }
        }
    }
    let exclude_capped = 0 < capped_ties && capped_ties < ties;
    if exclude_capped {
        ties -= capped_ties;
    }
    if ties != 1 {
        let mut selected = rng.next_int(ties)?;
        for c in &combinations {
            if c.score == best_score && (c.penalty != 60.0 || !exclude_capped) {
                if selected == 0 {
                    return Ok(PairedAlignment {
                        read: Some(c.read.clone()),
                        mate: Some(c.mate.clone()),
                        score: c.score,
                        mapping_quality: best.mapping_quality,
                    });
                }
                selected -= 1;
            }
        }
        return Err(MapError::new("Native random pair invariant failed"));
    }
    Ok(best)
}

/// The paired core against one reference with the web's fixed settings.
pub struct PairedMapper<'a> {
    pub search: &'a CandidateSearch,
    pub kernel: &'a Kernel,
    pub circular_length: Option<i64>,
    pub expected_distance: i64,
    pub overlap_score_extension: i64,
    /// Pre-ranking candidates of queries already seen against this reference. Candidates do not
    /// depend on the pair or its generator, so reusing them never changes a result.
    pub cache: CandidateCache,
}

/// A bounded, thread-shared store of [`refine_candidates`] results by query.
#[derive(Default)]
pub struct CandidateCache {
    entries: std::sync::Mutex<std::collections::HashMap<Vec<u8>, std::sync::Arc<Vec<Refined>>>>,
}

impl CandidateCache {
    const MAX_ENTRIES: usize = 100_000;

    fn get_or_compute(
        &self,
        query: &[u8],
        compute: impl FnOnce() -> Result<Vec<Refined>, MapError>,
    ) -> Result<std::sync::Arc<Vec<Refined>>, MapError> {
        if let Ok(entries) = self.entries.lock()
            && let Some(found) = entries.get(query)
        {
            return Ok(found.clone());
        }
        let value = std::sync::Arc::new(compute()?);
        if let Ok(mut entries) = self.entries.lock()
            && entries.len() < Self::MAX_ENTRIES
        {
            entries.insert(query.to_vec(), value.clone());
        }
        Ok(value)
    }
}

/// One query's refined candidates, ranked as often as asked.
struct Query<'a> {
    sequence: &'a [u8],
    refined: std::sync::Arc<Vec<Refined>>,
}

impl PairedMapper<'_> {
    fn candidates(&self, query: &Query, rng: &mut JavaRandom, limit: i64, window: i64, minimum: f64) -> Result<Vec<RankedAlignment>, MapError> {
        rank_candidates(
            &query.refined,
            query.sequence.len() as i64,
            self.search.reference.len() as i64,
            self.search.circular,
            &RankOptions { max_candidates: limit, score_window: window, minimum_score: minimum },
            rng,
        )
    }

    fn choose_single(&self, query: &Query, candidates: &[RankedAlignment], rng: &mut JavaRandom) -> Result<Option<RankedAlignment>, MapError> {
        if candidates.len() == 10 && candidates.first().map(|c| c.score) == candidates.last().map(|c| c.score) {
            let selected = self.candidates(query, rng, 1, 20, 0.0)?;
            return Ok(selected.into_iter().next());
        }
        let best_score = candidates.iter().map(|h| h.score).fold(f64::NEG_INFINITY, f64::max);
        let choices: Vec<&RankedAlignment> = candidates.iter().filter(|h| h.score == best_score).collect();
        let pick = rng.next_int(choices.len() as i64)?;
        Ok(choices.get(pick as usize).map(|h| (*h).clone()))
    }

    /// `map_normalized` for one pair of normalized mates (MapAll, one result, the pair's own RNG).
    pub fn map_normalized(&self, read: &[u8], mate: &[u8], pair_index: i64) -> Result<PairedAlignment, MapError> {
        let mut rng = JavaRandom::new(read_pair_seed(pair_index));
        let read_q = Query { sequence: read, refined: self.cache.get_or_compute(read, || refine_candidates(read, self.search, self.kernel, true))? };
        let mate_q = Query { sequence: mate, refined: self.cache.get_or_compute(mate, || refine_candidates(mate, self.search, self.kernel, true))? };
        let rng = &mut rng;
        let mut reads = self.candidates(&read_q, rng, 10, 200, 0.0)?;
        let mut mates = self.candidates(&mate_q, rng, 10, 200, 0.0)?;
        if reads.len() >= 10 && mates.len() >= 10 {
            let narrower_reads = self.candidates(&read_q, rng, 10, 20, 0.0)?;
            let narrower_mates = self.candidates(&mate_q, rng, 10, 20, 0.0)?;
            if narrower_reads.len() < 10 {
                reads = narrower_reads;
            }
            if narrower_mates.len() < 10 {
                mates = narrower_mates;
            }
        }
        if !reads.is_empty() && !mates.is_empty() {
            if reads.len() < 10 && mates.len() >= 10 {
                mates = self.candidates(&mate_q, rng, INT_MAX, 20, 0.0)?;
            } else if mates.len() < 10 && reads.len() >= 10 {
                reads = self.candidates(&read_q, rng, INT_MAX, 20, 0.0)?;
            } else if reads.len() >= 10 && mates.len() >= 10 {
                let (read_minimum, mate_minimum) = (reads[0].score, mates[0].score);
                reads = self.candidates(&read_q, rng, INT_MAX, 20, read_minimum)?;
                mates = self.candidates(&mate_q, rng, INT_MAX, 20, mate_minimum)?;
            }
            return choose_paired_candidates(
                &reads,
                &mates,
                self.expected_distance,
                self.circular_length,
                rng,
                self.overlap_score_extension,
                read,
                mate,
                &self.search.reference,
            );
        }
        let (mut chosen_read, mut chosen_mate) = (None, None);
        let mut quality = 0;
        if let Some(first) = reads.first() {
            quality = first.mapping_quality();
            chosen_read = self.choose_single(&read_q, &reads, rng)?;
        }
        if let Some(first) = mates.first() {
            quality = first.mapping_quality();
            chosen_mate = self.choose_single(&mate_q, &mates, rng)?;
        }
        // Native empty/singleton fallback clamps the score to 1, even when both are missing.
        let score = orphan_score(chosen_read.as_ref(), chosen_mate.as_ref());
        Ok(PairedAlignment { read: chosen_read, mate: chosen_mate, score, mapping_quality: quality })
    }
}
