//! `sam_export.py`: final padded rows, their native sort order, paired-overhang trimming and the
//! SAM text.

use std::collections::HashMap;

use crate::MapError;
use crate::dna::reverse_complement_text;
use crate::layout::{AlignedRow, LayoutRead};
use crate::py::is_py_space;
use crate::ranking::RankedAlignment;

/// One final read row (`SamRead`), as written to `sam-rows.jsonl`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SamRead {
    pub index: i64,
    pub name: String,
    pub leading: i64,
    /// The row's bases and gaps between its leading and trailing padding.
    pub internal: Vec<u8>,
    /// FASTQ qualities, or `*`.
    pub quality: String,
    pub reverse: bool,
    pub mate_index: i64,
    pub expected_distance: i64,
    pub trim_left: i64,
    pub trim_right: i64,
    pub mapping_quality: i64,
    pub mapped_locations: i64,
    pub structural_split: bool,
    pub contig_sort_key: (i64, i64, i64),
}

impl SamRead {
    pub fn end(&self) -> i64 {
        self.leading + self.internal.len() as i64
    }
}

fn gapped_trim(sequence: &[u8], bases: i64, left: bool) -> Result<i64, MapError> {
    if bases < 0 {
        return Err(MapError::new("Negative trim length"));
    }
    if bases == 0 {
        return Ok(0);
    }
    let mut seen = 0;
    let iter: Box<dyn Iterator<Item = &u8>> = if left { Box::new(sequence.iter()) } else { Box::new(sequence.iter().rev()) };
    for (columns, &base) in iter.enumerate() {
        seen += (base != b'-') as i64;
        if seen == bases {
            return Ok(columns as i64 + 1);
        }
    }
    Err(MapError::new("Trim exceeds ungapped read length"))
}

/// What `sam_read_from_layout` needs about one accepted read.
pub struct RowSource<'a> {
    pub layout_read: &'a LayoutRead,
    pub row: &'a AlignedRow,
    pub hit: &'a RankedAlignment,
    pub mate_number: i64,
    pub raw_name: &'a str,
    pub raw_sequence: &'a str,
    pub raw_quality: &'a str,
    pub index: i64,
    pub mate_index: i64,
    pub expected_distance: i64,
    pub mapping_quality: i64,
    pub circular_length: i64,
}

fn char_slice(s: &[char], start: i64, end: i64) -> Vec<char> {
    crate::py::slice(s, start, end).to_vec()
}

/// Builds the export row from the normalized mapper output and the original FASTQ record.
pub fn sam_read_from_layout(src: &RowSource) -> Result<SamRead, MapError> {
    if !(src.mate_number == 1 || src.mate_number == 2) {
        return Err(MapError::new("Illumina mate number must be 1 or 2"));
    }
    if src.expected_distance < 0 {
        return Err(MapError::new("Expected normalized distance must be nonnegative"));
    }
    let hit = src.hit;
    let reverse = hit.reverse != (src.mate_number == 2);
    let mut sequence: Vec<char> =
        if reverse { reverse_complement_text(src.raw_sequence).chars().collect() } else { src.raw_sequence.chars().collect() };
    let star = src.raw_quality == "*";
    let mut quality: Vec<char> = if reverse && !star { src.raw_quality.chars().rev().collect() } else { src.raw_quality.chars().collect() };
    let mut name = src.raw_name.to_string();
    let span = hit.alignment_length - hit.query_gap_sizes.iter().sum::<i64>();
    if src.circular_length != 0 && span < sequence.len() as i64 {
        sequence = char_slice(&sequence, hit.query_start, hit.query_start + span);
        if !star {
            quality = char_slice(&quality, hit.query_start, hit.query_start + span);
        }
        let raw_length = src.raw_sequence.chars().count() as i64;
        let begin = if hit.reverse { raw_length - hit.query_start - span } else { hit.query_start };
        let end = begin + span;
        let normalized_name = format!("{}{}", src.raw_name, if src.mate_number == 2 { " (reversed)" } else { "" });
        name = format!("{normalized_name} (fragment {} -> {end})", begin + 1);
        if hit.reverse {
            name.push_str(" (reversed)");
        }
    } else if reverse {
        name.push_str(" (reversed)");
    }
    let internal_full = src.row.internal();
    let extra_leading = internal_full.iter().take_while(|&&b| b == b'-').count();
    let mut leading = src.row.leading + extra_leading as i64;
    let trailing = internal_full.iter().rev().take_while(|&&b| b == b'-').count();
    let internal: Vec<u8> =
        if extra_leading >= internal_full.len() { Vec::new() } else { internal_full[extra_leading..internal_full.len() - trailing].to_vec() };
    if src.circular_length != 0 && leading < 0 {
        leading += src.circular_length;
    }
    let row_bases: String = internal.iter().filter(|&&b| b != b'-').map(|&b| b as char).collect::<String>().to_uppercase();
    let read_bases: String = sequence.iter().collect::<String>().to_uppercase();
    if row_bases != read_bases {
        return Err(MapError::new("Projected row does not match oriented original read or fragment"));
    }
    if !star && quality.len() != sequence.len() {
        return Err(MapError::new("Sequence and quality lengths differ"));
    }
    let quality: String = if star { "*".into() } else { quality.iter().map(|&c| char::from_u32((c as u32).clamp(33, 126)).unwrap_or('~')).collect() };
    let trim_left = gapped_trim(&internal, src.layout_read.trim_left, true)?;
    let trim_right = gapped_trim(&internal, src.layout_read.trim_right, false)?;
    Ok(SamRead {
        index: src.index,
        name,
        leading,
        quality,
        reverse,
        mate_index: src.mate_index,
        expected_distance: if src.mate_index >= 0 { if reverse { -src.expected_distance } else { src.expected_distance } } else { 0 },
        trim_left,
        trim_right,
        mapping_quality: src.mapping_quality,
        mapped_locations: 1,
        structural_split: false,
        contig_sort_key: (src.row.leading, internal_full.len() as i64, src.row.gaps.total()),
        internal,
    })
}

/// The native comparator and twin-array quicksort, with its deliberate instability.
pub fn sort_contig_reads(mut rows: Vec<SamRead>) -> Result<Vec<SamRead>, MapError> {
    let mut keys: Vec<(i64, i64, i64)> = rows.iter().map(|r| r.contig_sort_key).collect();
    struct Sorter<'a> {
        rows: &'a mut Vec<SamRead>,
        keys: &'a mut Vec<(i64, i64, i64)>,
    }
    impl Sorter<'_> {
        fn swap(&mut self, a: usize, b: usize) {
            self.rows.swap(a, b);
            self.keys.swap(a, b);
        }
        fn med3(&self, a: usize, b: usize, c: usize) -> usize {
            let k = &self.keys;
            if k[a] < k[b] {
                if k[b] < k[c] {
                    b
                } else if k[a] < k[c] {
                    c
                } else {
                    a
                }
            } else if k[b] > k[c] {
                b
            } else if k[a] > k[c] {
                c
            } else {
                a
            }
        }
        fn vecswap(&mut self, a: usize, b: usize, n: usize) {
            for i in 0..n {
                self.swap(a + i, b + i);
            }
        }
        fn sort(&mut self, off: usize, length: usize, depth: u32) -> Result<(), MapError> {
            if depth > 900 {
                return Err(MapError::new("Row sort recursion too deep"));
            }
            if length < 7 {
                for i in off..off + length {
                    let mut j = i;
                    while j > off && self.keys[j - 1] > self.keys[j] {
                        self.swap(j, j - 1);
                        j -= 1;
                    }
                }
                return Ok(());
            }
            let mut middle = off + (length >> 1);
            if length > 7 {
                let (mut low, mut high) = (off, off + length - 1);
                if length > 40 {
                    let step = length / 8;
                    low = self.med3(low, low + step, low + 2 * step);
                    middle = self.med3(middle - step, middle, middle + step);
                    high = self.med3(high - 2 * step, high - step, high);
                }
                middle = self.med3(low, middle, high);
            }
            let pivot = self.keys[middle];
            let (mut a, mut b) = (off as i64, off as i64);
            let (mut c, mut d) = ((off + length) as i64 - 1, (off + length) as i64 - 1);
            loop {
                while b <= c && self.keys[b as usize] <= pivot {
                    if self.keys[b as usize] == pivot {
                        self.swap(a as usize, b as usize);
                        a += 1;
                    }
                    b += 1;
                }
                while c >= b && self.keys[c as usize] >= pivot {
                    if self.keys[c as usize] == pivot {
                        self.swap(c as usize, d as usize);
                        d -= 1;
                    }
                    c -= 1;
                }
                if b > c {
                    break;
                }
                self.swap(b as usize, c as usize);
                b += 1;
                c -= 1;
            }
            let end = (off + length) as i64;
            let off_i = off as i64;
            let size = (a - off_i).min(b - a);
            self.vecswap(off, (b - size) as usize, size as usize);
            let size = (d - c).min(end - d - 1);
            self.vecswap(b as usize, (end - size) as usize, size as usize);
            let size = b - a;
            if size > 1 {
                self.sort(off, size as usize, depth + 1)?;
            }
            let size = d - c;
            if size > 1 {
                self.sort((end - size) as usize, size as usize, depth + 1)?;
            }
            Ok(())
        }
    }
    let n = rows.len();
    Sorter { rows: &mut rows, keys: &mut keys }.sort(0, n, 0)?;
    Ok(rows)
}

fn trim_paired_overhang(read: &SamRead, mate: &SamRead) -> SamRead {
    if read.structural_split || mate.structural_split {
        return read.clone();
    }
    let (mut left, mut right) = (read.trim_left, read.trim_right);
    let len = read.internal.len() as i64;
    if mate.expected_distance > 0 && read.expected_distance < 0 && read.leading < mate.leading && read.end() > mate.leading + 30 {
        let amount = mate.leading - read.leading;
        if amount + right >= len {
            left = len;
            right = 0;
        } else {
            left = left.max(amount);
        }
    }
    if read.expected_distance > 0 && mate.expected_distance < 0 && mate.end() < read.end() && mate.end() > read.leading + 30 {
        let amount = read.end() - mate.end();
        if amount + left >= len {
            left = len;
            right = 0;
        } else {
            right = right.max(amount);
        }
    }
    SamRead { trim_left: left, trim_right: right, ..read.clone() }
}

/// Trims each row against its unmodified mate, keeping the input order.
pub fn trim_all_paired_overhangs(reads: &[SamRead]) -> Vec<SamRead> {
    let mut by_index: HashMap<i64, usize> = HashMap::new();
    for (i, r) in reads.iter().enumerate() {
        by_index.insert(r.index, i);
    }
    reads
        .iter()
        .map(|read| match by_index.get(&read.mate_index) {
            Some(&m) => trim_paired_overhang(read, &reads[m]),
            None => read.clone(),
        })
        .collect()
}

fn safe_name(name: &str) -> String {
    let mut name = name.to_string();
    if name.starts_with('*') || name.starts_with('=') {
        name = format!("_{}", &name[1..]);
    }
    name.chars().map(|c| if "\\?,[]'\"(){}".contains(c) || is_py_space(c) { '_' } else { c }).collect()
}

fn unreversed_name(name: &str) -> &str {
    name.strip_suffix(" (reversed)").unwrap_or(name)
}

fn chars_from_end(s: &str, n: usize) -> Option<char> {
    s.chars().rev().nth(n - 1)
}

fn drop_last_two(s: &str) -> &str {
    let mut it = s.char_indices().rev();
    it.next();
    match it.next() {
        Some((i, _)) => &s[..i],
        None => "",
    }
}

#[derive(Clone, PartialEq, Eq)]
enum PairMode {
    Identical,
    Suffix,
    Joined,
}

#[derive(Clone)]
struct PairName {
    mode: PairMode,
    name: String,
}

impl PairName {
    fn new(first: &str, second: Option<&str>) -> Self {
        let first = unreversed_name(first).to_string();
        let second = second.map(|s| unreversed_name(s).to_string());
        let suffix1 = first.chars().count() > 2 && chars_from_end(&first, 2) == Some('/');
        let suffix2 = second.as_ref().is_some_and(|s| s.chars().count() > 2 && chars_from_end(s, 2) == Some('/'));
        let same = match &second {
            Some(s) => (suffix1 && suffix2 && drop_last_two(&first) == drop_last_two(s)) || (!suffix1 && !suffix2 && &first == s),
            None => false,
        };
        let mode = match &second {
            None => PairMode::Identical,
            Some(s) if same && &first == s && !first.ends_with("/?") => PairMode::Identical,
            Some(s)
                if same
                    && ((first.ends_with("/1") && s.ends_with("/2"))
                        || (first.ends_with("/2") && s.ends_with("/1"))
                        || (first.ends_with("/?") && s.ends_with("/?"))) =>
            {
                PairMode::Suffix
            }
            _ => PairMode::Joined,
        };
        let name = match mode {
            PairMode::Identical => first.clone(),
            PairMode::Suffix => drop_last_two(&first).to_string(),
            PairMode::Joined => format!("{first}%26{}", second.unwrap_or_default()),
        };
        PairName { mode, name }
    }

    fn first_flag(&self, name: &str) -> Option<bool> {
        let name = unreversed_name(name);
        match self.mode {
            PairMode::Joined => Some(self.name.starts_with(&format!("{name}%26"))),
            PairMode::Suffix if name.ends_with("/1") => Some(true),
            PairMode::Suffix if name.ends_with("/2") => Some(false),
            _ => None,
        }
    }
}

fn cigar(reference: &[u8], read: &SamRead) -> Result<String, MapError> {
    let sequence = &read.internal;
    let n = sequence.len() as i64;
    let left = read.trim_left.min(n);
    let right = read.trim_right.min(n - left);
    let count_gaps = |s: &[u8]| s.iter().filter(|&&b| b == b'-').count() as i64;
    let soft_left = left - count_gaps(&sequence[..left.max(0) as usize]);
    let soft_right = if right != 0 { right - count_gaps(&sequence[(n - right) as usize..]) } else { 0 };
    let mut operations: Vec<(i64, char)> = Vec::new();
    if soft_left != 0 {
        operations.push((soft_left, 'S'));
    }
    let start = read.leading + read.trim_left;
    let reference_end = (reference.len() - reference.iter().rev().take_while(|&&b| b == b'-').count()) as i64;
    let extra = start - reference_end;
    if extra > 0 {
        operations.push((extra, 'P'));
    }
    let render = |ops: &[(i64, char)]| ops.iter().map(|(n, op)| format!("{n}{op}")).collect::<String>();
    if soft_left == sequence.iter().filter(|&&b| b != b'-').count() as i64 {
        return Ok(render(&operations));
    }
    let length = n - read.trim_left - read.trim_right;
    let state = |i: i64| -> Result<char, MapError> {
        let r = crate::py::index(reference, start + i).ok_or_else(|| MapError::new("CIGAR reference index out of range"))?;
        let q = crate::py::index(sequence, read.trim_left + i).ok_or_else(|| MapError::new("CIGAR read index out of range"))?;
        let ref_gap = r == b'-';
        let read_gap = q == b'-';
        Ok(if ref_gap {
            if read_gap { 'P' } else { 'I' }
        } else if read_gap {
            'D'
        } else {
            'M'
        })
    };
    let mut current = state(0)?;
    let mut count = 1;
    for i in 1..length {
        let op = state(i)?;
        if op == current {
            count += 1;
        } else {
            operations.push((count, current));
            current = op;
            count = 1;
        }
    }
    operations.push((count, current));
    let reference_leading = reference.iter().take_while(|&&b| b == b'-').count() as i64;
    // The original subtracts the stripped length from the full length: the leading gap count.
    let extra = reference_leading - (start + length + read.trim_right);
    if extra > 0 {
        operations.push((extra, 'P'));
    }
    if soft_right != 0 {
        operations.push((soft_right, 'S'));
    }
    Ok(render(&operations))
}

/// SAM lines for the rows, in row order, keeping duplicates (`export_sam`, padded, circular as
/// given, no `=` bases, ambiguities kept).
pub fn export_sam(reference: &[u8], reads: &[SamRead], reference_name: &str, sample_name: &str, circular: bool) -> Result<Vec<String>, MapError> {
    let mut by_index: HashMap<i64, usize> = HashMap::new();
    for (i, r) in reads.iter().enumerate() {
        by_index.insert(r.index, i);
    }
    if by_index.len() != reads.len() {
        return Err(MapError::new("Contig row indices must be unique; names may repeat"));
    }
    let group_suffix = safe_name(&format!("assembled_against_{reference_name}"));
    let reference_name = safe_name(reference_name);
    let original_reference = reference;
    let doubled: Vec<u8>;
    let reference: &[u8] = if circular {
        let leading = reference.iter().take_while(|&&b| b == b'-').count();
        let trailing = reference.iter().rev().take_while(|&&b| b == b'-').count();
        let core: &[u8] = if leading >= reference.len() { &[] } else { &reference[leading..reference.len() - trailing] };
        let mut v = vec![b'-'; leading];
        v.extend_from_slice(core);
        v.extend_from_slice(core);
        doubled = v;
        &doubled
    } else {
        reference
    };
    let mut prefix = Vec::with_capacity(reference.len() + 1);
    prefix.push(0i64);
    for &base in reference {
        let last = *prefix.last().unwrap_or(&0);
        prefix.push(last + (base != b'-') as i64);
    }
    let position = |read: &SamRead| -> Result<i64, MapError> {
        let i = read.leading + read.trim_left;
        crate::py::index(&prefix, i).map(|p| p + 1).ok_or_else(|| MapError::new("SAM position outside the reference"))
    };
    let mate_of = |read: &SamRead| by_index.get(&read.mate_index).map(|&m| &reads[m]);
    let mut group_keys: Vec<i64> = vec![-1];
    let mut orientations: HashMap<i64, [i64; 4]> = HashMap::new();
    for read in reads {
        let Some(mate) = mate_of(read) else { continue };
        let key = read.expected_distance.abs();
        if !group_keys.contains(&key) {
            group_keys.push(key);
            orientations.insert(key, [0; 4]);
        }
        let orientation = if read.reverse == mate.reverse {
            1
        } else if read.reverse == (read.leading < mate.leading) {
            2
        } else {
            0
        };
        if let Some(o) = orientations.get_mut(&key) {
            o[orientation] += 1;
        }
    }
    // Integer-key Java HashMap iteration order, including stable bucket chains.
    let mut capacity: i64 = 16;
    while group_keys.len() as f64 > capacity as f64 * 0.75 {
        capacity *= 2;
    }
    let bucket = |key: i64| -> i64 {
        let unsigned = key & 0xffffffff;
        (unsigned ^ (unsigned >> 16)) & (capacity - 1)
    };
    group_keys.sort_by_key(|&k| bucket(k));
    let mut group_names: HashMap<i64, String> = HashMap::new();
    let mut lines = vec![
        "@HD\tVN:1.6\tSO:unsorted".to_string(),
        format!("@SQ\tSN:{reference_name}\tLN:{}", original_reference.iter().filter(|&&b| b != b'-').count()),
    ];
    let mut pair_group_number = 1;
    for &key in &group_keys {
        let label = if key == -1 { "Unpaired_reads".to_string() } else { format!("Paired_Group_{pair_group_number}") };
        if key != -1 {
            pair_group_number += 1;
        }
        let group = format!("{label}_{group_suffix}");
        lines.push(format!("@RG\tID:{group}\tSM:{sample_name}{}", if key != -1 { format!("\tPI:{key}") } else { String::new() }));
        group_names.insert(key, group);
    }
    let orientation_by_group: HashMap<i64, usize> = orientations
        .iter()
        .map(|(&key, counts)| {
            let mut best = 0;
            for i in 1..4 {
                if counts[i] > counts[best] {
                    best = i;
                }
            }
            (key, best)
        })
        .collect();
    let mut pair_names: HashMap<String, PairName> = HashMap::new();
    for read in reads {
        let mate = mate_of(read);
        let sequence: String = read.internal.iter().filter(|&&b| b != b'-').map(|&b| b as char).collect();
        if sequence.is_empty() {
            continue;
        }
        let naming = match mate {
            None => PairName::new(&read.name, None),
            Some(mate) => match pair_names.remove(&mate.name) {
                Some(n) => n,
                None => {
                    let name = unreversed_name(&read.name);
                    let first = if name.ends_with("/1") {
                        true
                    } else if name.ends_with("/2") {
                        false
                    } else {
                        let orientation = orientation_by_group.get(&read.expected_distance.abs()).copied().unwrap_or(0);
                        let same = read.reverse == mate.reverse;
                        if (orientation == 1 || orientation == 3) != same {
                            true
                        } else {
                            let left = read.leading < mate.leading;
                            let positive_is_left = (read.expected_distance > 0) == left;
                            positive_is_left == ((orientation == if same { 1 } else { 0 }) == (left == !read.reverse))
                        }
                    };
                    let naming = if first { PairName::new(&read.name, Some(&mate.name)) } else { PairName::new(&mate.name, Some(&read.name)) };
                    pair_names.insert(read.name.clone(), naming.clone());
                    naming
                }
            },
        };
        let (first, second) = match naming.first_flag(&read.name) {
            None => {
                let first = mate.is_some() && pair_names.contains_key(&read.name);
                (first, mate.is_some() && !first)
            }
            Some(first) => (first, !first),
        };
        let mut flag = if mate.is_some() { 3 } else { 0 } | if read.reverse { 16 } else { 0 };
        flag |= if mate.is_some_and(|m| m.reverse) { 32 } else { 0 };
        flag |= if first { 64 } else { 0 };
        flag |= if second { 128 } else { 0 };
        let pos = position(read)?;
        let mate_position = match mate {
            Some(m) => position(m)?,
            None => 0,
        };
        let mut template_length = 0;
        if let Some(m) = mate {
            let delta = pos - mate_position;
            template_length = delta.abs() + if delta < 0 { m.internal.len() as i64 } else { sequence.chars().count() as i64 };
            if delta >= 0 {
                template_length = -template_length;
            }
        }
        let sequence = sequence.replace(['U', 'u'], "T").replace('?', "N");
        let quality = if read.mapping_quality < 0 { 255 } else { read.mapping_quality.min(254) };
        let key = if mate.is_some() { read.expected_distance.abs() } else { -1 };
        let group = group_names.get(&key).ok_or_else(|| MapError::new("Missing read group"))?;
        let fields = [
            naming.name.clone(),
            flag.to_string(),
            reference_name.clone(),
            pos.to_string(),
            quality.to_string(),
            cigar(reference, read)?,
            if mate.is_some() { "=".into() } else { "*".into() },
            mate_position.to_string(),
            template_length.to_string(),
            sequence,
            read.quality.clone(),
            format!("RG:Z:{group}"),
        ];
        lines.push(fields.join("\t"));
    }
    Ok(lines)
}
