//! `seed_extension.py` and the Java kernel it runs (`standalone/*.java`).
//!
//! The original runs the kernel as a separate Java process and talks to it in text: Python's
//! `str(float)` goes in and is read by `Float.parseFloat`, Java's `Float.toString` comes out and
//! is read by Python's `float`. [`Kernel`] keeps that boundary exactly, so every number the Python
//! side sees is the same double it saw in the original. The three submodules are the Java
//! classes, ported statement by statement in Java's own int and float arithmetic.

mod extension;
mod refiner;
mod support;

use crate::MapError;
use crate::py::{java_float_to_python, python_float_to_java};

/// What one seed extension returns, as the Python side sees it.
#[derive(Clone, Debug)]
pub struct ExtensionResult {
    pub score: f64,
    pub score_gain: f64,
    pub ref_left: i64,
    pub query_left: i64,
    pub alignment_length: i64,
    pub ref_gap_positions: Vec<i64>,
    pub query_gap_positions: Vec<i64>,
    pub ref_gap_sizes: Vec<i64>,
    pub query_gap_sizes: Vec<i64>,
    pub mismatch_count: i64,
    pub reference_id: i64,
    pub reference_start: i64,
}

/// One alignment after final gap placement, as the Python side sees it.
#[derive(Clone, Debug)]
pub struct RefinedAlignment {
    pub score: f64,
    pub query_start: i64,
    pub reference_start: i64,
    pub alignment_length: i64,
    pub query_gap_positions: Vec<i64>,
    pub ref_gap_positions: Vec<i64>,
    pub query_gap_sizes: Vec<i64>,
    pub ref_gap_sizes: Vec<i64>,
    pub mismatch_count: i64,
}

/// An alignment handed to [`Kernel::refine_alignment`].
pub struct RawForRefine<'a> {
    pub score: f64,
    pub query_start: i64,
    pub reference_start: i64,
    pub alignment_length: i64,
    pub query_gap_positions: &'a [i64],
    pub ref_gap_positions: &'a [i64],
    pub query_gap_sizes: &'a [i64],
    pub ref_gap_sizes: &'a [i64],
    pub mismatch_count: i64,
}

/// The kernel with the mapper's options. It holds no state between requests, so one value may be
/// shared by every thread.
pub struct Kernel {
    scores: support::Scores,
    fraction: f32,
    gap_lookahead: i32,
    minimum_gap_remaining: i32,
}

fn int(value: i64) -> Result<i32, MapError> {
    i32::try_from(value).map_err(|_| MapError::new("Kernel request number does not fit a Java int"))
}

fn ints(values: &[i32]) -> Vec<i64> {
    values.iter().map(|&v| v as i64).collect()
}

fn java_ints(values: &[i64]) -> Result<Vec<i32>, MapError> {
    values.iter().map(|&v| int(v)).collect()
}

impl Kernel {
    /// `SeedExtender(minimum_extension_score_fraction, gap_lookahead, minimum_gap_lookahead_remaining)`.
    pub fn new(fraction: f64, gap_lookahead: i64, minimum_gap_remaining: i64) -> Result<Self, MapError> {
        if !fraction.is_finite() || !(0.0..=1.0).contains(&fraction) {
            return Err(MapError::new("Extension score fraction must be between 0 and 1"));
        }
        if gap_lookahead < 0 || minimum_gap_remaining < 0 {
            return Err(MapError::new("Gap lookahead values must be nonnegative integers"));
        }
        Ok(Kernel {
            scores: support::Scores::new(),
            fraction: python_float_to_java(fraction),
            gap_lookahead: int(gap_lookahead)?,
            minimum_gap_remaining: int(minimum_gap_remaining)?,
        })
    }

    /// Extends an already oriented query from a seed (`SeedExtender.extend_seed`, index length 12).
    pub fn extend_seed(
        &self,
        query: &[u8],
        reference: &[u8],
        circular: bool,
        reference_start: i64,
        query_start: i64,
        seed_length: i64,
        direction: i64,
        stop_match_run: i64,
        initial_score: f64,
        max_gap: i64,
    ) -> Result<ExtensionResult, MapError> {
        let (ref_seed, query_seed, seed_length) = (int(reference_start)?, int(query_start)?, int(seed_length)?);
        let (direction, max_gap) = (int(direction)?, int(max_gap)?);
        let (rl, ql) = (reference.len() as i64, query.len() as i64);
        if rl == 0
            || ql == 0
            || ref_seed < 0
            || ref_seed as i64 >= rl
            || query_seed < 0
            || seed_length < 1
            || query_seed as i64 + seed_length as i64 > ql
            || (!circular && ref_seed as i64 + seed_length as i64 > rl)
            || !(-1..=1).contains(&direction)
            || max_gap < 0
        {
            return Err(MapError::new("Invalid seed extension input"));
        }
        let options =
            support::Options { n: self.fraction, q: self.gap_lookahead, b_remaining: self.minimum_gap_remaining, ..support::Options::default() };
        let r = extension::extend(
            reference,
            circular,
            query,
            ref_seed,
            query_seed,
            seed_length,
            12,
            2,
            &self.scores,
            &options,
            direction,
            int(stop_match_run)?,
            python_float_to_java(initial_score),
            max_gap,
        )
        .map_err(|e| MapError::new(format!("Seed extension kernel: {e}")))?;
        Ok(ExtensionResult {
            score: java_float_to_python(r.a),
            score_gain: java_float_to_python(r.b),
            ref_left: r.c as i64,
            query_left: r.d as i64,
            alignment_length: r.g as i64,
            ref_gap_positions: ints(&r.h),
            query_gap_positions: ints(&r.i),
            ref_gap_sizes: ints(&r.j),
            query_gap_sizes: ints(&r.k),
            mismatch_count: r.l as i64,
            reference_id: r.n as i64,
            reference_start: r.o as i64,
        })
    }

    /// Final gap placement for one alignment (`SeedExtender.refine_alignment`).
    pub fn refine_alignment(&self, query: &[u8], reference: &[u8], circular: bool, raw: &RawForRefine) -> Result<RefinedAlignment, MapError> {
        let mut hit = refiner::Hit {
            c: python_float_to_java(raw.score),
            f: int(raw.query_start)?,
            g: int(raw.reference_start)?,
            h: int(raw.alignment_length)?,
            o: int(raw.mismatch_count)?,
            k: java_ints(raw.query_gap_positions)?,
            l: java_ints(raw.ref_gap_positions)?,
            m: java_ints(raw.query_gap_sizes)?,
            n: java_ints(raw.ref_gap_sizes)?,
            ..refiner::Hit::default()
        };
        refiner::refine(&mut hit, reference, query, circular, &self.scores)
            .map_err(|e| MapError::new(format!("Alignment refinement kernel: {e}")))?;
        Ok(RefinedAlignment {
            score: java_float_to_python(hit.c),
            query_start: hit.f as i64,
            reference_start: hit.g as i64,
            alignment_length: hit.h as i64,
            query_gap_positions: ints(&hit.k),
            ref_gap_positions: ints(&hit.l),
            query_gap_sizes: ints(&hit.m),
            ref_gap_sizes: ints(&hit.n),
            mismatch_count: hit.o as i64,
        })
    }
}
