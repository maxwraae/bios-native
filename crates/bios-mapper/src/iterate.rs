//! `iterative_mapper.py` and `iteration_selection.py`: map every pair, lay the hits out, build the
//! next reference from their consensus, and repeat until the preset or convergence.

use crate::consensus::{ConsensusResult, build_consensus};
use crate::dna::{normalize, reverse_complement};
use crate::fastq::FastqRead;
use crate::kernel::Kernel;
use crate::layout::{AlignmentLayout, LayoutRead, layout_read_from_hit};
use crate::paired::{PairedAlignment, PairedMapper};
use crate::ranking::RankedAlignment;
use crate::search::CandidateSearch;
use crate::{MapError, Progress};

/// One accepted read record of an iteration (`ContigRead`).
#[derive(Clone, Debug)]
pub struct ContigRead {
    pub index: i64,
    /// The pair this record came from, and which mate (1 or 2).
    pub pair: usize,
    pub mate_number: i64,
    pub hit: RankedAlignment,
    pub layout_read: LayoutRead,
    pub mate_index: i64,
    pub expected_distance: i64,
    pub mapping_quality: i64,
}

/// One complete iteration (`MappingIteration`).
pub struct MappingIteration {
    pub index: usize,
    pub mapping_reference: Vec<u8>,
    pub raw_pairs: Vec<PairedAlignment>,
    pub layout: AlignmentLayout,
    pub reads: Vec<ContigRead>,
    pub consensus: Option<ConsensusResult>,
}

struct SelectedPair {
    result: PairedAlignment,
    original_coordinates: bool,
    read_mapping_quality: Option<i64>,
    mate_mapping_quality: Option<i64>,
}

fn shift_original(hit: &Option<RankedAlignment>, origin_shift: i64, reference_length: i64) -> Option<RankedAlignment> {
    let hit = hit.as_ref()?;
    if origin_shift <= 0 {
        return Some(hit.clone());
    }
    let mut start = hit.reference_start - origin_shift;
    if start < 0 {
        start += reference_length;
    }
    Some(RankedAlignment { reference_start: start, ..hit.clone() })
}

fn select_iteration_pair(
    first: &PairedAlignment,
    current: &PairedAlignment,
    first_iteration: bool,
    origin_shift: i64,
    original_reference_length: i64,
) -> SelectedPair {
    let (mut read_limit, mut mate_limit) = (254, 254);
    let mut retained = false;
    let mut selected = current.clone();
    if !first_iteration {
        // Java promotes the native float scores to double before multiplying.
        if first.score > current.score * 1.1 {
            retained = true;
            selected = PairedAlignment {
                read: shift_original(&first.read, origin_shift, original_reference_length),
                mate: shift_original(&first.mate, origin_shift, original_reference_length),
                ..first.clone()
            };
        } else {
            if let Some(r) = &first.read {
                read_limit = r.mapping_quality();
            }
            if let Some(m) = &first.mate {
                mate_limit = m.mapping_quality();
            }
            selected = PairedAlignment { mapping_quality: first.mapping_quality.min(current.mapping_quality), ..current.clone() };
        }
    }
    let mut read_quality = selected.read.as_ref().map(|r| read_limit.min(r.mapping_quality()));
    let mut mate_quality = selected.mate.as_ref().map(|m| mate_limit.min(m.mapping_quality()));
    if let (Some(r), Some(m)) = (&selected.read, &selected.mate)
        && r.reference_id == m.reference_id
    {
        read_quality = read_quality.map(|q| q.max(selected.mapping_quality));
        mate_quality = mate_quality.map(|q| q.max(selected.mapping_quality));
    }
    SelectedPair { result: selected, original_coordinates: retained, read_mapping_quality: read_quality, mate_mapping_quality: mate_quality }
}

/// Maps every normalized pair against one reference, in order, across `workers` threads.
fn map_pairs(
    normalized: &[(Vec<u8>, Vec<u8>)],
    reference: &[u8],
    circular: bool,
    expected_distance: i64,
    trim_paired_overhangs: bool,
    workers: usize,
    iteration: usize,
    progress: Option<&(dyn Fn(&Progress) + Sync)>,
) -> Result<Vec<PairedAlignment>, MapError> {
    let search = CandidateSearch::new(reference.to_vec(), circular)?;
    let kernel = Kernel::new(0.9, 3, 0)?;
    let mapper = PairedMapper {
        search: &search,
        kernel: &kernel,
        circular_length: if circular { Some(search.reference.len() as i64) } else { None },
        expected_distance,
        overlap_score_extension: if trim_paired_overhangs { 30 } else { 0 },
        cache: crate::paired::CandidateCache::default(),
    };
    let total = normalized.len();
    let done = std::sync::atomic::AtomicUsize::new(0);
    let report = |count: usize| {
        if let Some(p) = progress
            && count.is_multiple_of(1000)
        {
            p(&Progress::Mapping { iteration: iteration + 1, pairs: count, total_pairs: total });
        }
    };
    let workers = workers.clamp(1, 64).min(total.max(1));
    if workers == 1 {
        let mut out = Vec::with_capacity(total);
        for (index, (read, mate)) in normalized.iter().enumerate() {
            out.push(mapper.map_normalized(read, mate, index as i64)?);
            report(index + 1);
        }
        return Ok(out);
    }
    // Pairs are independent: each has its own generator seeded by its index. Workers take pairs
    // in small blocks and the results are put back in input order.
    let block = 64;
    let next = std::sync::atomic::AtomicUsize::new(0);
    let results: Vec<Result<Vec<(usize, PairedAlignment)>, MapError>> = std::thread::scope(|scope| {
        let handles: Vec<_> = (0..workers)
            .map(|_| {
                scope.spawn(|| -> Result<Vec<(usize, PairedAlignment)>, MapError> {
                    let mut mine = Vec::new();
                    loop {
                        let start = next.fetch_add(block, std::sync::atomic::Ordering::Relaxed);
                        if start >= total {
                            return Ok(mine);
                        }
                        for index in start..(start + block).min(total) {
                            let (read, mate) = &normalized[index];
                            mine.push((index, mapper.map_normalized(read, mate, index as i64)?));
                            let count = done.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1;
                            report(count);
                        }
                    }
                })
            })
            .collect();
        handles.into_iter().map(|h| h.join().unwrap_or_else(|_| Err(MapError::new("A mapping worker stopped unexpectedly")))).collect()
    });
    let mut slots: Vec<Option<PairedAlignment>> = vec![None; total];
    for r in results {
        for (index, pair) in r? {
            slots[index] = Some(pair);
        }
    }
    slots.into_iter().map(|s| s.ok_or_else(|| MapError::new("A pair was not mapped"))).collect()
}

/// Runs every iteration and returns the last (`iterate_mapping`).
pub fn iterate_mapping(
    pairs: &[(FastqRead, FastqRead)],
    original_reference: &[u8],
    circular: bool,
    expected_distance: i64,
    max_iterations: usize,
    trim_paired_overhangs: bool,
    workers: usize,
    progress: Option<&(dyn Fn(&Progress) + Sync)>,
) -> Result<MappingIteration, MapError> {
    if ![1, 2, 3, 5, 10].contains(&max_iterations) {
        return Err(MapError::new("Unsupported native iteration preset"));
    }
    if expected_distance <= 0 {
        return Err(MapError::new("Expected insert distance must be positive"));
    }
    if pairs.is_empty() {
        return Err(MapError::new("No input read pairs"));
    }
    let mut normalized = Vec::with_capacity(pairs.len());
    for (r, m) in pairs {
        normalized.push((normalize(&r.sequence)?, reverse_complement(&normalize(&m.sequence)?)));
    }
    let original_text = String::from_utf8_lossy(original_reference);
    let mut reference = normalize(&original_text)?;
    let mut first: Option<Vec<PairedAlignment>> = None;
    let mut previous: Option<ConsensusResult> = None;
    let original_length = original_reference.len() as i64;
    for iteration in 0..max_iterations {
        let raw = map_pairs(&normalized, &reference, circular, expected_distance, trim_paired_overhangs, workers, iteration, progress)?;
        if raw.len() != pairs.len() {
            return Err(MapError::new("Checkpoint input pair count differs"));
        }
        let first_raw = first.get_or_insert_with(|| raw.clone());
        let mapping = previous.as_ref().map(|p| &p.mapping);
        let mut layout = AlignmentLayout::new(original_reference, circular, mapping)?;
        let origin_shift = mapping.map_or(0, |m| m.origin_shift);
        let mut reads = Vec::new();
        for (index, ((initial, current), sequences)) in first_raw.iter().zip(&raw).zip(&normalized).enumerate() {
            let selected = select_iteration_pair(initial, current, iteration == 0, origin_shift, original_length);
            let hits = [&selected.result.read, &selected.result.mate];
            let qualities = [selected.read_mapping_quality, selected.mate_mapping_quality];
            let seqs = [&sequences.0, &sequences.1];
            let paired = hits.iter().all(|h| h.is_some());
            for number in 0..2usize {
                let Some(hit) = hits[number] else { continue };
                let source_length = if selected.original_coordinates { original_length } else { reference.len() as i64 };
                let item = layout_read_from_hit(seqs[number], hit, source_length, circular, selected.original_coordinates)?;
                layout.add_read(item.clone())?;
                let distance = if paired { expected_distance * if hit.reverse != (number == 1) { -1 } else { 1 } } else { 0 };
                reads.push(ContigRead {
                    index: index as i64 * 2 + number as i64,
                    pair: index,
                    mate_number: number as i64 + 1,
                    hit: hit.clone(),
                    layout_read: item,
                    mate_index: if paired { index as i64 * 2 + 1 - number as i64 } else { -1 },
                    expected_distance: distance,
                    mapping_quality: qualities[number].unwrap_or(0),
                });
            }
        }
        let row = layout.reference_row()?;
        let mut consensus = None;
        if iteration + 1 < max_iterations {
            let mut gapped = row.render()?;
            if circular {
                while gapped.last() == Some(&b'-') {
                    gapped.pop();
                }
            }
            let consensus_reads = layout.consensus_reads()?;
            consensus = Some(build_consensus(&gapped, &consensus_reads, circular, iteration as i64, max_iterations as i64, 50)?);
        }
        if let Some(p) = progress {
            p(&Progress::IterationComplete {
                iteration: iteration + 1,
                raw_mapped_records: raw.iter().map(|p| p.read.is_some() as usize + p.mate.is_some() as usize).sum(),
                accepted_records: reads.len(),
            });
        }
        let converged = match (&consensus, &previous) {
            (Some(c), Some(p)) => {
                iteration >= 2
                    && c.mapping.reference_aligned == p.mapping.reference_aligned
                    && c.mapping.consensus_aligned == p.mapping.consensus_aligned
            }
            _ => false,
        };
        let result =
            MappingIteration { index: iteration, mapping_reference: reference.clone(), raw_pairs: raw, layout, reads, consensus: consensus.clone() };
        match consensus {
            None => return Ok(result),
            Some(_) if converged => return Ok(result),
            Some(c) => {
                reference = c.mapping.consensus();
                if reference.is_empty() {
                    return Err(MapError::new("Consensus became empty; this profile cannot continue"));
                }
                previous = Some(c);
                if iteration + 1 == max_iterations {
                    return Ok(result);
                }
            }
        }
    }
    Err(MapError::new("No iteration ran"))
}
