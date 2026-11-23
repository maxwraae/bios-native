//! `iterative_consensus.py`: the weighted consensus each iteration maps against next, and its
//! normalized alignment back to the preceding reference.

use crate::MapError;
use crate::py::py_mod;

/// A read as the iteration consensus counts it.
#[derive(Clone, Debug)]
pub struct ConsensusRead {
    pub aligned_sequence: Vec<u8>,
    pub mismatch_count: i64,
    pub alignment_start: i64,
    pub trim_left: i64,
    pub trim_right: i64,
    pub gap_event_count: i64,
    pub original_length: i64,
    pub trusted_existing: bool,
}

/// Five saturated vote tracks, in A/C/G/T/gap tie order.
struct ConsensusCounter {
    length: i64,
    circular_length: i64,
    counts: [Vec<i64>; 5],
}

fn ambiguity(base: u8) -> Option<&'static [usize]> {
    Some(match base {
        b'R' => &[0, 2],
        b'Y' => &[1, 3],
        b'M' => &[0, 1],
        b'W' => &[0, 3],
        b'S' => &[1, 2],
        b'K' => &[2, 3],
        b'B' => &[1, 2, 3],
        b'D' => &[0, 2, 3],
        b'H' => &[0, 1, 3],
        b'V' => &[0, 1, 2],
        _ => return None,
    })
}

impl ConsensusCounter {
    fn new(length: i64, circular_length: i64) -> Self {
        let length = if circular_length > 0 { circular_length } else { length };
        let n = length.max(0) as usize;
        ConsensusCounter { length, circular_length, counts: [vec![0; n], vec![0; n], vec![0; n], vec![0; n], vec![0; n]] }
    }

    fn add(&mut self, read: &ConsensusRead, downweight_end_gaps: bool) -> Result<(), MapError> {
        if read.mismatch_count < 0 {
            return Err(MapError::new("Negative mismatch count"));
        }
        let sequence = &read.aligned_sequence;
        let begin = sequence.iter().take_while(|&&b| b == b'-').count();
        let end = sequence.len() - sequence.iter().rev().take_while(|&&b| b == b'-').count();
        let ungapped_length = sequence.iter().filter(|&&b| b != b'-').count() as i64;
        let mut seen = 0i64;
        let weight: i64 = match read.mismatch_count {
            0 => 40,
            1 => 10,
            2 => 4,
            3 => 2,
            _ => 1,
        };
        for local in begin..end.max(begin) {
            let base = sequence[local];
            let clipped = seen < read.trim_left || seen >= ungapped_length - read.trim_right;
            if base != b'-' {
                seen += 1;
            }
            if clipped {
                continue;
            }
            let base = base.to_ascii_uppercase();
            let mut position = read.alignment_start + local as i64;
            if self.circular_length > 0 {
                position = py_mod(position, self.circular_length);
            }
            if !(0 <= position && position < self.length) {
                return Err(MapError::new("Consensus read position outside padded alignment"));
            }
            let single;
            let all = [0usize, 1, 2, 3];
            let (states, value): (&[usize], i64) = match base {
                b'A' | b'C' | b'G' | b'T' | b'U' => {
                    single = [match base {
                        b'A' => 0,
                        b'C' => 1,
                        b'G' => 2,
                        _ => 3,
                    }];
                    (&single, weight)
                }
                b'-' => {
                    let margin = if downweight_end_gaps { 8 } else { 0 };
                    (&[4], if seen < margin || seen >= ungapped_length - margin { 1 } else { weight })
                }
                other => match ambiguity(other) {
                    Some(states) => (states, 1.max(weight >> 1)),
                    None => (&all, 1.max(weight >> 2)),
                },
            };
            for &state in states {
                let cell = &mut self.counts[state][position as usize];
                *cell = 32767.min(*cell + value);
            }
        }
        Ok(())
    }

    fn consensus(&self, reference: &[u8], fallback_uncovered: bool) -> Result<Vec<u8>, MapError> {
        if reference.len() as i64 != self.length {
            return Err(MapError::new("Reference and count-array lengths differ"));
        }
        let mut output = Vec::with_capacity(reference.len());
        for position in 0..self.length as usize {
            let (mut base, mut maximum) = (b'-', 0);
            for (state, symbol) in b"ACGT-".iter().enumerate() {
                if self.counts[state][position] > maximum {
                    base = *symbol;
                    maximum = self.counts[state][position];
                }
            }
            if fallback_uncovered && maximum == 0 {
                base = reference[position];
            }
            output.push(base);
        }
        Ok(output)
    }
}

fn remove_double_gaps(first: &mut [u8], second: &mut [u8], length: usize) -> usize {
    let mut target = 0;
    for source in 0..length {
        if first[source] == b'-' && second[source] == b'-' {
            continue;
        }
        first[target] = first[source];
        second[target] = second[source];
        target += 1;
    }
    target
}

fn remove_positions(sequence: &mut [u8], positions: &[usize], length: usize) -> Result<(), MapError> {
    let (mut source, mut removed) = (0usize, 0usize);
    for target in 0..length.saturating_sub(positions.len()) {
        while removed < positions.len() && positions[removed] == source {
            if sequence.get(source) != Some(&b'-') {
                return Err(MapError::new("Attempted to cancel a nongap"));
            }
            source += 1;
            removed += 1;
        }
        let value = *sequence.get(source).ok_or_else(|| MapError::new("Gap cancellation ran past the alignment"))?;
        sequence[target] = value;
        source += 1;
    }
    Ok(())
}

fn cancel_opposing_gaps(first: &mut [u8], second: &mut [u8], length: usize) -> Result<usize, MapError> {
    let (mut remove_first, mut remove_second) = (Vec::new(), Vec::new());
    let (mut first_start, mut second_start) = (-1i64, -1i64);
    let (mut first_size, mut second_size) = (0i64, 0i64);
    let (mut in_first, mut in_second) = (false, false);
    let at = |s: &[u8], i: i64| -> Result<u8, MapError> {
        if i < 0 {
            return Err(MapError::new("Gap cancellation index out of range"));
        }
        s.get(i as usize).copied().ok_or_else(|| MapError::new("Gap cancellation index out of range"))
    };
    for position in 0..=length {
        let p = position as i64;
        if position >= length || second[position] != b'-' {
            let ended = in_second;
            in_second = false;
            if ended && first_start >= 0 {
                if in_first {
                    return Err(MapError::new("Overlapping gap runs should have been removed"));
                }
                let at_end = position == length || first_start == 0;
                let separation = second_start - (first_start + first_size);
                let cancelled = second_size.min(first_size);
                if separation <= 10.max(cancelled * 2) {
                    let mut old_errors = 0;
                    for i in 0..separation.max(0) {
                        let k = first_start + first_size + i;
                        old_errors += (at(first, k)? != at(second, k)?) as i64;
                    }
                    let mut new_errors = 0;
                    for i in 0..(separation + cancelled).max(0) {
                        new_errors += (at(first, first_start + first_size + i)? != at(second, first_start + i)?) as i64;
                    }
                    if new_errors <= cancelled + (if at_end { 0 } else { cancelled }) + old_errors {
                        remove_first.extend(((first_start + first_size - cancelled)..(first_start + first_size)).map(|x| x as usize));
                        remove_second.extend((second_start..(second_start + cancelled)).map(|x| x as usize));
                    }
                }
                first_start = -1;
            }
        } else if !in_second {
            in_second = true;
            second_start = p;
            second_size = 1;
        } else {
            second_size += 1;
        }
        if position >= length || first[position] != b'-' {
            in_first = false;
        } else if !in_first {
            in_first = true;
            first_start = p;
            first_size = 1;
        } else {
            first_size += 1;
        }
    }
    if !remove_second.is_empty() {
        remove_positions(first, &remove_first, length)?;
        remove_positions(second, &remove_second, length)?;
    }
    Ok(length - remove_second.len())
}

fn shift_gaps(first: &mut [u8], second: &[u8], length: usize, direction: i64) -> Result<bool, MapError> {
    let mut changed = false;
    let mut gap_start: i64 = -1;
    let (mut bases, mut skipped_gaps, mut gain) = (0i64, 0i64, 0i64);
    let n = length as i64;
    let mut position: i64 = if direction == 1 { 0 } else { n - 1 };
    let get = |s: &[u8], i: i64| -> Result<u8, MapError> {
        if i < 0 {
            return Err(MapError::new("Gap shift index out of range"));
        }
        s.get(i as usize).copied().ok_or_else(|| MapError::new("Gap shift index out of range"))
    };
    while 0 <= position && position < n {
        let mut move_end: i64 = -1;
        let here = get(first, position)?;
        if here != b'-' {
            if gap_start >= 0 {
                let comparison = gap_start + bases * direction;
                if 0 < gap_start && gap_start < n - 1 {
                    let other = get(second, position)?;
                    if here == other {
                        gain -= 1;
                    }
                    let compared = get(second, comparison)?;
                    if compared == here {
                        gain += 1;
                    }
                    let improved = here != other && compared == here && (gap_start - position).abs() <= 50 + bases * 2 && gain >= 1;
                    bases += 1;
                    if improved {
                        move_end = position;
                    }
                } else {
                    gap_start = -1;
                }
            }
        } else {
            if gap_start > 0 && bases > 0 {
                if (gap_start - position).abs() <= 50 + bases * 2 {
                    if gain >= 0 {
                        move_end = position - direction;
                    } else {
                        gap_start = -1;
                    }
                } else {
                    skipped_gaps += 1;
                }
            }
            if gap_start == -1 {
                gap_start = position;
                bases = 0;
                skipped_gaps = 0;
                gain = 0;
            }
        }
        if move_end >= 0 {
            changed = true;
            let mut source = move_end + direction * (-(bases + skipped_gaps) + 1);
            let mut target = gap_start;
            let mut moved = 0;
            let mut guard = 0u64;
            while moved < bases {
                guard += 1;
                if guard > 10_000_000 {
                    return Err(MapError::new("Gap shift did not finish"));
                }
                if get(first, source)? == b'-' {
                    if skipped_gaps <= 0 {
                        return Err(MapError::new("Gap-shift accounting error"));
                    }
                    skipped_gaps -= 1;
                    source += direction;
                    continue;
                }
                let value = get(first, source)?;
                get(first, target)?;
                first[target as usize] = value;
                first[source as usize] = b'-';
                source += direction;
                target += direction;
                moved += 1;
            }
            gap_start += direction * bases + skipped_gaps;
            bases = 0;
            skipped_gaps = 0;
            gain = 0;
        }
        position += direction;
    }
    Ok(changed)
}

/// The normalized alignment between a reference and its consensus (`ConsensusMapping`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConsensusMapping {
    pub reference_aligned: Vec<u8>,
    pub consensus_aligned: Vec<u8>,
    pub circular: bool,
    pub origin_shift: i64,
}

impl ConsensusMapping {
    pub fn consensus(&self) -> Vec<u8> {
        self.consensus_aligned.iter().copied().filter(|&b| b != b'-').collect()
    }

    /// Native `aj$a.a`: ungapped positions, with its terminal-gap convention.
    pub fn gap_runs(&self, consensus: bool) -> Vec<(i64, i64)> {
        let mut runs = Vec::new();
        let (mut position, mut pending) = (0i64, 0i64);
        for (&r, &c) in self.reference_aligned.iter().zip(&self.consensus_aligned) {
            if r == b'-' && c == b'-' {
                continue;
            }
            let is_gap = if consensus { c == b'-' } else { r == b'-' };
            if is_gap {
                pending += 1;
            } else {
                if pending > 0 {
                    runs.push((position, pending));
                    pending = 0;
                }
                position += 1;
            }
        }
        if pending > 0 && consensus && self.circular {
            runs.push((position, pending));
        }
        runs
    }

    pub fn trailing_gaps(&self, consensus: bool) -> i64 {
        let mut count = 0;
        for (&r, &c) in self.reference_aligned.iter().rev().zip(self.consensus_aligned.iter().rev()) {
            if r == b'-' && c == b'-' {
                continue;
            }
            if if consensus { c == b'-' } else { r == b'-' } {
                count += 1;
            } else {
                break;
            }
        }
        count
    }
}

/// Gap cancellation, shifting and the circular-origin choice (`normalize_consensus_mapping`).
pub fn normalize_consensus_mapping(reference: &[u8], consensus: &[u8], circular: bool) -> Result<ConsensusMapping, MapError> {
    if reference.len() != consensus.len() {
        return Err(MapError::new("Reference and consensus must share alignment columns"));
    }
    let mut first = reference.to_vec();
    let mut second = consensus.to_vec();
    let mut length = first.len();
    let mut previous: Option<usize> = None;
    let mut guard = 0u32;
    while previous != Some(length) {
        guard += 1;
        if guard > 100_000 {
            return Err(MapError::new("Consensus normalization did not settle"));
        }
        previous = Some(length);
        length = remove_double_gaps(&mut first, &mut second, length);
        length = cancel_opposing_gaps(&mut first, &mut second, length)?;
        length = cancel_opposing_gaps(&mut second, &mut first, length)?;
        let mut shifted = false;
        for direction in [-1, 1] {
            shifted = shift_gaps(&mut first, &second, length, direction)? || shifted;
            shifted = shift_gaps(&mut second, &first, length, direction)? || shifted;
        }
        if shifted {
            length = remove_double_gaps(&mut first, &mut second, length);
            length = cancel_opposing_gaps(&mut first, &mut second, length)?;
            length = cancel_opposing_gaps(&mut second, &mut first, length)?;
        }
    }
    first.truncate(length);
    second.truncate(length);
    let mut origin = 0;
    let has_gap = |a: &[u8], b: &[u8]| -> bool { [a.first(), b.first(), a.last(), b.last()].contains(&Some(&b'-')) };
    if circular && length > 0 && has_gap(&first, &second) {
        let mut found = false;
        for column in 1..length {
            origin += (first[column - 1] != b'-') as i64;
            if ![first[column], second[column], first[column - 1], second[column - 1]].contains(&b'-') {
                first.rotate_left(column);
                second.rotate_left(column);
                found = true;
                break;
            }
        }
        if !found {
            return Err(MapError::new("No two adjacent nongap columns for circular origin"));
        }
    }
    Ok(ConsensusMapping { reference_aligned: first, consensus_aligned: second, circular, origin_shift: origin })
}

/// The consensus of one iteration (`ConsensusResult`), without its vote counts.
#[derive(Clone, Debug)]
pub struct ConsensusResult {
    pub raw_consensus: Vec<u8>,
    pub mapping: ConsensusMapping,
    pub included_reads: i64,
    pub excluded_reads: i64,
}

pub fn build_consensus(
    gapped_reference: &[u8],
    reads: &[ConsensusRead],
    circular: bool,
    iteration: i64,
    max_iterations: i64,
    max_gap_size: i64,
) -> Result<ConsensusResult, MapError> {
    let n = gapped_reference.len() as i64;
    let mut counter = ConsensusCounter::new(n, if circular { n } else { 0 });
    let (mut included, mut excluded) = (0, 0);
    for read in reads {
        let original_length = read.original_length;
        if iteration == 0 && max_iterations > 2 && !read.trusted_existing && read.mismatch_count + read.gap_event_count > original_length * 16 / 100 {
            excluded += 1;
            continue;
        }
        counter.add(read, iteration < 2)?;
        included += 1;
    }
    let fallback = max_gap_size <= 15 || (iteration <= 0 && max_iterations >= 3) || (iteration <= 1 && max_iterations >= 5);
    let raw = if included + excluded > 0 { counter.consensus(gapped_reference, fallback)? } else { gapped_reference.to_vec() };
    let mapping = normalize_consensus_mapping(gapped_reference, &raw, circular)?;
    Ok(ConsensusResult { raw_consensus: raw, mapping, included_reads: included, excluded_reads: excluded })
}
