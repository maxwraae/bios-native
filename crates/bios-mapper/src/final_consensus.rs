//! `final_consensus.py`: the weighted-60 consensus, coverage and quality of the final padded rows.

use std::collections::HashMap;

use crate::MapError;
use crate::py::{floor_div, py_mod};
use crate::sam::SamRead;

/// The counts of one base in one column: reads, summed quality, highest quality.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BaseCount {
    pub count: i64,
    pub quality_sum: i64,
    pub quality_max: i64,
}

/// The final consensus (`FinalConsensus`). `column_counts` keeps each column's bases in the order
/// they were first seen, as the original's dictionaries do.
#[derive(Clone, Debug, PartialEq)]
pub struct FinalConsensus {
    pub consensus_aligned: Vec<u8>,
    pub coverage: Vec<f64>,
    pub quality: Vec<i64>,
    pub metadata: Metadata,
    pub column_counts: Vec<Vec<(char, BaseCount)>>,
}

impl FinalConsensus {
    pub fn consensus_ungapped(&self) -> Vec<u8> {
        self.consensus_aligned.iter().copied().filter(|&b| b != b'-').collect()
    }
}

/// The profile values the original records with its consensus.
#[derive(Clone, Debug, PartialEq)]
pub struct Metadata {
    pub threshold: f64,
    pub counts_cache_window: i64,
    pub raw_gap_quality: &'static str,
    pub circular_length: i64,
    pub padded_length: i64,
    pub read_count: i64,
}

fn base_set(base: char) -> &'static str {
    match base {
        'A' => "A",
        'C' => "C",
        'G' => "G",
        'T' => "T",
        'R' => "AG",
        'Y' => "CT",
        'M' => "AC",
        'K' => "GT",
        'S' => "CG",
        'W' => "AT",
        'B' => "CGT",
        'D' => "AGT",
        'H' => "ACT",
        'V' => "ACG",
        'N' | '?' => "ACGT",
        '-' => "-",
        _ => "ACGT",
    }
}

/// The IUPAC symbol for a set of states (`IUPAC[frozenset(...)]`), states as a bit set over ACGT-.
fn iupac_for(states: u8) -> Option<char> {
    let table: [(&str, char); 16] = [
        ("A", 'A'),
        ("C", 'C'),
        ("G", 'G'),
        ("T", 'T'),
        ("AG", 'R'),
        ("CT", 'Y'),
        ("AC", 'M'),
        ("GT", 'K'),
        ("CG", 'S'),
        ("AT", 'W'),
        ("CGT", 'B'),
        ("AGT", 'D'),
        ("ACT", 'H'),
        ("ACG", 'V'),
        ("ACGT", 'N'),
        ("-", '-'),
    ];
    table.iter().find(|(set, _)| bits(set) == states).map(|t| t.1)
}

fn bit(state: char) -> u8 {
    match state {
        'A' => 1,
        'C' => 2,
        'G' => 4,
        'T' => 8,
        '-' => 16,
        _ => 0,
    }
}

fn bits(set: &str) -> u8 {
    set.chars().map(bit).fold(0, |a, b| a | b)
}

/// `combined_quality`: the mapping and base qualities combined as probabilities.
pub fn combined_quality(base_quality: i64, mapping_quality: i64) -> i64 {
    if mapping_quality < 0 {
        return base_quality;
    }
    let error = 1.0 - (1.0 - 10f64.powf(-(base_quality as f64) / 10.0)) * (1.0 - 10f64.powf(-(mapping_quality as f64) / 10.0));
    let value = if error == 0.0 { 2147483647 } else { (-10.0 * error.log10() + 0.5).floor() as i64 };
    value.min(base_quality).min(mapping_quality)
}

fn gap_quality(quality: i64) -> i64 {
    if quality >= 40 {
        return quality;
    }
    if quality <= 30 {
        return floor_div(quality, 2);
    }
    let q = quality as f64;
    let fraction = (q - 30.0) / 10.0;
    (q / 2.0 + q * fraction / 2.0 + 0.5).floor() as i64
}

fn adjusted_qualities(
    internal: &[u8],
    quality: &[char],
    star: bool,
    mapping_quality: i64,
    leading: i64,
    trim_left: i64,
    trim_right: i64,
    circular_length: i64,
    hash_size: i64,
    minimum_gap: bool,
) -> Result<Vec<i64>, MapError> {
    let size = internal.len();
    let positions: Vec<usize> = internal.iter().enumerate().filter(|(_, b)| **b != b'-').map(|(i, _)| i).collect();
    let qualities: Vec<i64> = if star { vec![0; positions.len()] } else { quality.iter().map(|&c| c as i64 - 33).collect() };
    if qualities.len() != positions.len() {
        return Err(MapError::new("Ungapped sequence and quality lengths differ"));
    }
    let mut raw = vec![0i64; size];
    for (&position, &q) in positions.iter().zip(&qualities) {
        raw[position] = combined_quality(q, mapping_quality);
    }
    for (i, pair) in positions.windows(2).enumerate() {
        let (left, right) = (pair[0], pair[1]);
        if right > left + 1 {
            let gap_q = if minimum_gap { qualities[i].min(qualities[i + 1]) } else { floor_div(qualities[i] + qualities[i + 1], 2) };
            let value = combined_quality(gap_q, mapping_quality);
            for cell in &mut raw[left + 1..right] {
                *cell = value;
            }
        }
    }
    let mut result = raw.clone();
    let finish = size as i64 - trim_right;
    let mut begin = trim_left;
    while begin < finish {
        let mut coordinate = leading + begin;
        if circular_length != 0 {
            coordinate = py_mod(coordinate, circular_length);
        }
        let mut width = hash_size - py_mod(coordinate, hash_size);
        if circular_length != 0 {
            width = width.min(circular_length - coordinate);
        }
        let end = finish.min(begin + width);
        if end <= begin {
            return Err(MapError::new("Quality window did not advance"));
        }
        quality_window(internal, &raw, begin, end, &mut result)?;
        begin = end;
    }
    Ok(result)
}

fn quality_window(sequence: &[u8], raw: &[i64], begin: i64, end: i64, output: &mut [i64]) -> Result<(), MapError> {
    let n = sequence.len() as i64;
    let at = |i: i64| -> Result<u8, MapError> {
        if i < 0 {
            return Err(MapError::new("Quality window index out of range"));
        }
        sequence.get(i as usize).copied().ok_or_else(|| MapError::new("Quality window index out of range"))
    };
    let raw_at = |i: i64| -> Result<i64, MapError> {
        if i < 0 {
            return Err(MapError::new("Quality window index out of range"));
        }
        raw.get(i as usize).copied().ok_or_else(|| MapError::new("Quality window index out of range"))
    };
    let set = |output: &mut [i64], i: i64, v: i64| {
        if let Some(cell) = output.get_mut(i as usize) {
            *cell = v;
        }
    };
    let scan_end = n.min(end + 100);
    let mut position = begin;
    while at(position)? == b'-' && position >= 1 && position > begin - 100 {
        position -= 1;
    }
    let first_base = at(position)?;
    while first_base != b'-' && position >= 1 && position > begin - 100 {
        position -= 1;
        let b = at(position)?;
        if b != b'-' && b != first_base {
            break;
        }
    }
    let mut left_quality = raw_at(position)?;
    while position < end {
        let gap_begin = position;
        while position < scan_end && at(position)? == b'-' {
            position += 1;
        }
        if position >= scan_end {
            for column in begin.max(gap_begin)..end {
                set(output, column, gap_quality(left_quality));
            }
            break;
        }
        let group_begin = position;
        let group_base = at(position)?;
        let mut members = vec![position];
        let mut cursor = position + 1;
        while cursor < scan_end && cursor - group_begin < 100 {
            let base = at(cursor)?;
            if base != b'-' {
                if base != group_base {
                    break;
                }
                members.push(cursor);
            }
            cursor += 1;
        }
        let last = *members.last().unwrap_or(&position);
        let group_end = last + 1;
        let mut adjusted = Vec::with_capacity(members.len());
        for (i, &member) in members.iter().enumerate() {
            adjusted.push(raw_at(member)?.min(raw_at(members[members.len() - 1 - i])?));
        }
        let preceding_gap_quality = gap_quality(left_quality.min(adjusted[0]));
        for column in begin.max(gap_begin)..group_begin.min(end) {
            set(output, column, preceding_gap_quality);
        }
        for (i, &member) in members.iter().enumerate() {
            if begin <= member && member < end {
                set(output, member, adjusted[i]);
            }
            if i + 1 < members.len() {
                let internal_gap_quality = gap_quality(adjusted[i].min(adjusted[i + 1]));
                for column in begin.max(member + 1)..members[i + 1].min(end) {
                    set(output, column, internal_gap_quality);
                }
            }
        }
        left_quality = *adjusted.last().unwrap_or(&left_quality);
        position = group_end;
    }
    Ok(())
}

fn call_column(counts: &[(char, BaseCount)], read_count: i64, threshold: f64) -> (char, i64) {
    if counts.is_empty() {
        return ('?', 0);
    }
    let mut qualities: Vec<(char, i64)> = counts.iter().map(|(b, v)| (*b, v.quality_sum)).collect();
    if qualities.iter().map(|q| q.1).sum::<i64>() == 0 {
        for q in qualities.iter_mut() {
            q.1 = 1;
        }
    }
    let order = ['A', 'C', 'G', 'T', '-'];
    let mut weights = [0i64; 5];
    for &(base, quality) in &qualities {
        let states = base_set(base);
        let amount = floor_div(12 * quality, states.len() as i64);
        for state in states.chars() {
            if let Some(i) = order.iter().position(|&o| o == state) {
                weights[i] += amount;
            }
        }
    }
    let total: i64 = weights.iter().sum();
    let required = (total as f64 * threshold) as i64;
    let mut sorted: Vec<usize> = (0..5).collect();
    // Python's sorted(..., reverse=True) keeps equal weights in their original order.
    sorted.sort_by(|&a, &b| weights[b].cmp(&weights[a]));
    let mut selected: u8 = 0;
    let mut cumulative = 0i64;
    let mut previous_weight: Option<i64> = None;
    for &state in &sorted {
        let weight = weights[state];
        if selected != 0 {
            if cumulative >= required && Some(weight) != previous_weight {
                break;
            }
            if selected == bit('-') && total - weight >= required && read_count > 2 {
                break;
            }
        }
        selected |= bit(order[state]);
        cumulative += weight;
        previous_weight = Some(weight);
    }
    let consensus = if cumulative < required {
        '-'
    } else if selected & bit('-') != 0 && selected.count_ones() > 1 {
        'N'
    } else {
        iupac_for(selected).unwrap_or('N')
    };
    let mut contributing = bits(base_set(consensus));
    if consensus == 'N' {
        contributing |= bit('-');
    }
    let quality: i64 = counts.iter().map(|(base, v)| if contributing & bits(base_set(*base)) != 0 { v.quality_sum } else { -v.quality_sum }).sum();
    (consensus, quality.max(0))
}

/// A row's sequence, qualities, mapping quality, leading padding and trims.
type QualityKey = (Vec<u8>, String, i64, i64, i64, i64);

/// `final_consensus` with the CLI's settings: threshold 0.6 and the given counts-cache window.
pub fn final_consensus(
    padded_reference: &[u8],
    reads: &[SamRead],
    circular_length: i64,
    counts_cache_window: i64,
) -> Result<FinalConsensus, MapError> {
    let threshold = 0.6;
    let circular = circular_length;
    let length = if circular != 0 { circular } else { padded_reference.len() as i64 };
    if counts_cache_window <= 0 {
        return Err(MapError::new("Counts cache window must be positive"));
    }
    let minimum_gap = counts_cache_window == 256;
    let n = length.max(0) as usize;
    let mut counts: Vec<Vec<(char, BaseCount)>> = vec![Vec::new(); n];
    let mut coverage = vec![0.0f64; n];
    let mut cache: HashMap<QualityKey, Vec<i64>> = HashMap::new();
    for row in reads {
        let sequence: Vec<u8> = row
            .internal
            .iter()
            .map(|b| match b.to_ascii_uppercase() {
                b'U' => b'T',
                other => other,
            })
            .collect();
        let star = row.quality == "*";
        let quality: Vec<char> = row.quality.chars().collect();
        let key = (sequence.clone(), row.quality.clone(), row.mapping_quality, row.leading, row.trim_left, row.trim_right);
        let adjusted = match cache.get(&key) {
            Some(a) => a.clone(),
            None => {
                let a = adjusted_qualities(
                    &sequence,
                    &quality,
                    star,
                    row.mapping_quality,
                    row.leading,
                    row.trim_left,
                    row.trim_right,
                    circular,
                    counts_cache_window,
                    minimum_gap,
                )?;
                if cache.len() < 8192 {
                    cache.insert(key, a.clone());
                }
                a
            }
        };
        for offset in row.trim_left..(sequence.len() as i64 - row.trim_right) {
            let mut position = row.leading + offset;
            if circular != 0 {
                position = py_mod(position, circular);
            } else if !(0 <= position && position < length) {
                continue;
            }
            let p = position as usize;
            if row.mapped_locations != 0 {
                coverage[p] += 1.0 / row.mapped_locations as f64;
            }
            let base = sequence.get(offset as usize).copied().ok_or_else(|| MapError::new("Row offset out of range"))? as char;
            let effective_quality = 1.max(adjusted.get(offset as usize).copied().unwrap_or(0));
            let column = &mut counts[p];
            match column.iter_mut().find(|(b, _)| *b == base) {
                Some((_, v)) => {
                    v.count += 1;
                    v.quality_sum += effective_quality;
                    v.quality_max = v.quality_max.max(effective_quality);
                }
                None => column.push((base, BaseCount { count: 1, quality_sum: effective_quality, quality_max: effective_quality })),
            }
        }
    }
    let calls: Vec<(char, i64)> = counts.iter().map(|c| call_column(c, reads.len() as i64, threshold)).collect();
    Ok(FinalConsensus {
        consensus_aligned: calls.iter().map(|c| c.0 as u8).collect(),
        coverage,
        quality: calls.iter().map(|c| c.1).collect(),
        metadata: Metadata {
            threshold,
            counts_cache_window,
            raw_gap_quality: if minimum_gap { "minimum" } else { "mean" },
            circular_length: circular,
            padded_length: padded_reference.len() as i64,
            read_count: reads.len() as i64,
        },
        column_counts: counts,
    })
}
