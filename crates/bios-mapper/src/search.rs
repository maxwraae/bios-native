//! `seed_index.py` and `candidate_search.py`: the sparse word index of the reference and the
//! two-pass seed collection, with ambiguity expansion, that feeds chaining.

use std::collections::HashMap;

use crate::MapError;
use crate::dna::reverse_complement;
use crate::kernel::{ExtensionResult, Kernel};
use crate::primitives::iupac;
use crate::py::py_mod;

/// Every base-4 key a word stands for, in the Java word mapper's expansion order; empty when the
/// word stands for more than `maximum_ambiguity` words.
pub fn word_keys(word: &[u8], maximum_ambiguity: usize) -> Result<Vec<u64>, MapError> {
    let mut keys: Vec<u64> = vec![0];
    for &raw in word {
        let symbol = match raw.to_ascii_uppercase() {
            b'U' => b'T',
            s => s,
        };
        let Some(bases) = iupac(symbol) else {
            return Err(MapError::new(format!("No word key for {:?}", symbol as char)));
        };
        if keys.len() * bases.len() > maximum_ambiguity {
            return Ok(Vec::new());
        }
        let mut next = Vec::with_capacity(keys.len() * bases.len());
        for &base in bases {
            let value = match base {
                b'A' => 0,
                b'C' => 1,
                b'G' => 2,
                _ => 3,
            };
            for &key in &keys {
                next.push(key * 4 + value);
            }
        }
        keys = next;
    }
    Ok(keys)
}

pub struct SeedIndex {
    pub table: HashMap<u64, Vec<i64>>,
}

impl SeedIndex {
    pub fn new(reference: &[u8], circular: bool, word_length: usize, expansion_length: usize, repeat_limit: usize) -> Result<Self, MapError> {
        if reference.is_empty() {
            return Err(MapError::new("Reference sequence is empty"));
        }
        if expansion_length < word_length {
            return Err(MapError::new("Expansion word must be at least as long as the index word"));
        }
        let stride = expansion_length - word_length + 1;
        let n = reference.len();
        let limit = if circular { n as i64 } else { n as i64 - word_length as i64 + 1 };
        let mut padded = reference.to_vec();
        if circular {
            let repeats = word_length.div_ceil(n);
            let mut padding: Vec<u8> = reference.repeat(repeats);
            padding.truncate(word_length.saturating_sub(1));
            padded.extend(padding);
        }
        let mut table: HashMap<u64, Vec<i64>> = HashMap::new();
        let mut start = stride as i64 - 1;
        while start < limit {
            let s = start as usize;
            let word = padded.get(s..(s + word_length).min(padded.len())).unwrap_or(&[]);
            for key in word_keys(word, 4)? {
                table.entry(key).or_default().push(start);
            }
            start += stride as i64;
        }
        table.retain(|_, positions| positions.len() <= repeat_limit);
        Ok(SeedIndex { table })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SeedHit {
    pub reference_id: i64,
    pub diagonal: i64,
    pub query_start: i64,
    pub length: i64,
    pub reverse: bool,
}

impl SeedHit {
    pub fn reference_start(&self) -> i64 {
        self.query_start - self.diagonal
    }
}

struct Threshold {
    word_length: i64,
    extension: i64,
    limit: i64,
    counts: Vec<i64>,
    maximum: i64,
    maximum_capped: i64,
    skip_threshold: i64,
}

impl Threshold {
    fn new(word_length: i64, extension: i64) -> Self {
        let limit = 20.max(word_length + 2 * extension);
        Threshold {
            word_length,
            extension,
            limit,
            counts: vec![0; (4 * limit + 1) as usize],
            maximum: 0,
            maximum_capped: 0,
            skip_threshold: extension,
        }
    }

    fn observe(&mut self, length: i64, special: bool, sensitivity: i64) {
        if sensitivity == 2 {
            return;
        }
        if length > self.maximum {
            self.maximum = length;
            if special || sensitivity == 0 {
                self.skip_threshold = self.extension.max((self.word_length + length) / 50 - self.word_length);
            }
        }
        let length = length.min(self.counts.len() as i64 - 1);
        if let Some(c) = self.counts.get_mut(length as usize) {
            *c += 1;
        }
        self.maximum_capped = self.maximum_capped.max(length);
    }

    fn second_pass(&self, special: bool, sensitivity: i64) -> i64 {
        if sensitivity == 2 {
            return 0;
        }
        let mut threshold = self.extension;
        if special {
            threshold = (self.word_length + self.maximum_capped) / 4 - self.word_length;
        }
        if sensitivity == 0 {
            threshold = (self.word_length + self.maximum_capped) / 2 - self.word_length;
        }
        threshold = threshold.min(self.limit);
        let mut remaining = 10;
        let mut length = self.counts.len() as i64 - 1;
        while length > self.extension - 1 {
            remaining -= self.counts.get(length as usize).copied().unwrap_or(0);
            if remaining <= 0 {
                break;
            }
            threshold = threshold.min(length);
            length -= 1;
        }
        threshold.max(self.extension)
    }
}

struct MissWindow {
    extension: i64,
    size: i64,
    distance: [Vec<i64>; 2],
    all_hit: [bool; 2],
    phase: i64,
}

impl MissWindow {
    fn new(extension: i64, query_length: i64) -> Self {
        let size = extension + 1;
        MissWindow {
            extension,
            size,
            distance: [vec![query_length; size as usize], vec![query_length; size as usize]],
            all_hit: [true, true],
            phase: 0,
        }
    }

    fn allowed_left(&self, strand: usize) -> i64 {
        self.distance[strand].get(self.phase as usize).copied().unwrap_or(0)
    }

    fn advance(&mut self, distance: i64) {
        for strand in 0..2 {
            if self.all_hit[strand]
                && let Some(d) = self.distance[strand].get_mut(self.phase as usize)
            {
                *d = self.extension - 1;
            }
            for x in self.distance[strand].iter_mut() {
                *x += distance;
            }
        }
        self.phase = py_mod(self.phase + distance, self.size);
        self.all_hit = [true, true];
    }
}

fn byte_at(s: &[u8], i: i64) -> u8 {
    if i < 0 { 0 } else { s.get(i as usize).copied().unwrap_or(0) }
}

fn exact_run(
    query: &[u8],
    reference: &[u8],
    circular: bool,
    mut query_start: i64,
    mut reference_start: i64,
    direction: i64,
    limit: Option<i64>,
) -> i64 {
    let maximum = limit.unwrap_or(query.len() as i64);
    let n = reference.len() as i64;
    let mut matched = 0;
    while matched < maximum && 0 <= query_start && query_start < query.len() as i64 {
        if circular {
            reference_start = py_mod(reference_start, n);
        } else if !(0 <= reference_start && reference_start < n) {
            break;
        }
        let q = byte_at(query, query_start);
        if q != byte_at(reference, reference_start) || q == b'N' {
            break;
        }
        matched += 1;
        query_start += direction;
        reference_start += direction;
    }
    matched
}

/// The single-reference default-DNA seed search.
pub struct CandidateSearch {
    pub reference: Vec<u8>,
    pub circular: bool,
    pub word_length: i64,
    pub extension: i64,
    pub index: SeedIndex,
}

/// One seed with its two directional extensions, as `initial_extensions` yields it.
pub struct InitialExtension {
    pub seed: SeedHit,
    pub right: ExtensionResult,
    pub left: ExtensionResult,
    pub right_support: f64,
    pub left_support: f64,
}

impl CandidateSearch {
    /// `reference` must already be normalized.
    pub fn new(reference: Vec<u8>, circular: bool) -> Result<Self, MapError> {
        let (word_length, expansion_length, repeat_limit) = (12, 14, 20);
        let index = SeedIndex::new(&reference, circular, word_length, expansion_length, repeat_limit)?;
        Ok(CandidateSearch { reference, circular, word_length: word_length as i64, extension: (expansion_length - word_length) as i64, index })
    }

    /// The two strands' seeds for a normalized query (sensitivity 1, no minimum overlap).
    #[allow(clippy::cognitive_complexity)] // a line-for-line port; see the module notes
    pub fn collect(&self, forward: &[u8], special_search: bool) -> Result<[Vec<SeedHit>; 2], MapError> {
        let sensitivity = 1;
        let reverse = reverse_complement(forward);
        let queries: [&[u8]; 2] = [forward, &reverse];
        let query_length = forward.len() as i64;
        let reference = &self.reference;
        let n = reference.len() as i64;
        let word_length = self.word_length;
        let extension = self.extension;
        let mut threshold = Threshold::new(word_length, extension);
        let mut capacity: usize = 10;
        let mut output: [Vec<SeedHit>; 2] = [Vec::new(), Vec::new()];

        let add = |output: &mut [Vec<SeedHit>; 2], strand: usize, query_start: i64, diagonal: i64, length: i64| {
            if length < 2 {
                return;
            }
            let seed = SeedHit { reference_id: 0, diagonal, query_start, length, reverse: strand == 1 };
            let list = &mut output[strand];
            let tail = &list[list.len().saturating_sub(10)..];
            if !tail.contains(&seed) {
                list.push(seed);
            }
        };

        for pass_number in 0..2 {
            let first_pass = pass_number == 0;
            let required_extension = if first_pass { extension } else { threshold.second_pass(special_search, sensitivity) };
            // (diagonal, expiry query position, strand), in swap-delete order.
            let mut active: Vec<(i64, i64, usize)> = Vec::new();
            let mut overflow: usize = 0;
            let mut raw_hit_count: usize = 0;
            let mut query_start: i64 = 0;
            let mut skip_phase = extension;
            let mut misses = MissWindow::new(extension, query_length);
            while query_start <= query_length - word_length {
                let mut i = 0;
                while i < active.len() {
                    if active[i].1 - extension <= query_start {
                        active.swap_remove(i);
                    } else {
                        i += 1;
                    }
                }
                for (strand, query) in queries.iter().enumerate() {
                    let qs = query_start as usize;
                    let word = query.get(qs..qs + word_length as usize).unwrap_or(&[]);
                    for key in word_keys(word, 4)? {
                        let positions: &[i64] = self.index.table.get(&key).map(Vec::as_slice).unwrap_or(&[]);
                        if positions.is_empty() {
                            misses.all_hit[strand] = false;
                        }
                        for &reference_start in positions {
                            raw_hit_count += 1;
                            let mut diagonal = query_start - reference_start;
                            if active.iter().any(|&(a, _, s)| a == diagonal && s == strand) {
                                continue;
                            }
                            let right = exact_run(query, reference, self.circular, query_start + word_length, reference_start + word_length, 1, None);
                            let left = exact_run(query, reference, self.circular, query_start - 1, reference_start - 1, -1, None);
                            let combined = left + right;
                            if first_pass {
                                threshold.observe(combined, special_search, sensitivity);
                            }
                            if right > extension + 1 {
                                let expiry = query_start + right + 1;
                                let entry = (diagonal, expiry, strand);
                                if active.len() < capacity {
                                    active.push(entry);
                                } else {
                                    overflow += 1;
                                    let mut victim = 0;
                                    for j in 1..active.len() {
                                        if active[j].1 < active[victim].1 {
                                            victim = j;
                                        }
                                    }
                                    if let Some(v) = active.get_mut(victim)
                                        && v.1 < expiry
                                    {
                                        *v = entry;
                                    }
                                }
                            }
                            if first_pass || combined < required_extension {
                                continue;
                            }
                            while left > 0 && self.circular && query_start - left - diagonal < 0 {
                                diagonal -= n;
                            }
                            if left > misses.allowed_left(strand) {
                                continue;
                            }
                            let mut remaining = word_length + left + right;
                            let mut run_query = query_start - left;
                            let mut run_reference = reference_start - left;
                            if self.circular {
                                run_reference = py_mod(run_reference, n);
                            }
                            while remaining > 0 {
                                if self.circular {
                                    while run_query - diagonal < 0 {
                                        diagonal -= n;
                                    }
                                    while run_query - diagonal >= n {
                                        diagonal += n;
                                    }
                                }
                                let exact = exact_run(query, reference, self.circular, run_query, run_reference, 1, Some(remaining));
                                add(&mut output, strand, run_query, diagonal, exact);
                                remaining -= exact + 1;
                                run_query += exact + 1;
                                run_reference += exact + 1;
                            }
                        }
                    }
                }
                let mut step = 1;
                let skip_threshold = threshold.skip_threshold.max(required_extension);
                if skip_threshold - extension > 0 {
                    if skip_phase == 0 {
                        let groups = (skip_threshold + 1) / (extension + 1);
                        step = (groups - 1) * (extension + 1) + 1;
                        skip_phase = extension;
                    } else {
                        skip_phase -= 1;
                    }
                }
                query_start += step;
                misses.advance(step);
            }
            if first_pass && overflow > 0 {
                if overflow >= raw_hit_count / 2 {
                    capacity *= 4;
                } else if overflow >= raw_hit_count / 4 {
                    capacity *= 2;
                } else {
                    capacity = capacity * 3 / 2;
                }
            }
        }
        for hits in output.iter_mut() {
            hits.sort_by_key(|s| (s.reference_id, s.diagonal, s.query_start, s.length));
        }
        Ok(output)
    }

    /// Every seed with its two native directional extension requests, forward strand first.
    pub fn initial_extensions(&self, query: &[u8], kernel: &Kernel, special_search: bool) -> Result<Vec<InitialExtension>, MapError> {
        let reverse = reverse_complement(query);
        let queries: [&[u8]; 2] = [query, &reverse];
        let mut out = Vec::new();
        for (strand, seeds) in self.collect(query, special_search)?.iter().enumerate() {
            let mut previous: Option<(i64, i64, i64)> = None;
            for seed in seeds {
                let identity = (seed.reference_id, seed.query_start, seed.diagonal);
                if previous == Some(identity) {
                    continue;
                }
                previous = Some(identity);
                let q = queries[strand];
                let extend = |direction: i64, initial: f64| {
                    kernel.extend_seed(
                        q,
                        &self.reference,
                        self.circular,
                        seed.reference_start(),
                        seed.query_start,
                        seed.length,
                        direction,
                        self.word_length,
                        initial,
                        50,
                    )
                };
                let mut right = extend(1, 0.0)?;
                let mut left = extend(-1, 0.0)?;
                let right_support = if right.score <= 0.0 && left.score_gain > 0.0 { left.score_gain } else { 0.0 };
                let left_support = if left.score <= 0.0 && right.score_gain > 0.0 { right.score_gain } else { 0.0 };
                if right_support > 0.0 {
                    right = extend(1, right_support)?;
                }
                if left_support > 0.0 {
                    left = extend(-1, left_support)?;
                }
                out.push(InitialExtension { seed: *seed, right, left, right_support, left_support });
            }
        }
        Ok(out)
    }
}
