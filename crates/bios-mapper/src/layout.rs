//! `alignment_layout.py`: gap maps, shared insertion columns and the projection of every read
//! and of the reference into one padded alignment.

use std::collections::BTreeMap;

use crate::MapError;
use crate::consensus::{ConsensusMapping, ConsensusRead};
use crate::dna::reverse_complement;
use crate::py::{floor_div, index, py_mod, slice};
use crate::ranking::RankedAlignment;

const BIG: i64 = 2147483647;

/// Gap runs `(ungapped anchor, size)`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct GapMap {
    pub runs: Vec<(i64, i64)>,
}

impl GapMap {
    pub fn new(runs: Vec<(i64, i64)>) -> Self {
        GapMap { runs }
    }

    pub fn from_gapped(positions: &[i64], sizes: &[i64]) -> Self {
        let mut total = 0;
        let mut runs: Vec<(i64, i64)> = Vec::new();
        for (&position, &size) in positions.iter().zip(sizes) {
            let anchor = position - total;
            match runs.last_mut() {
                Some(last) if last.0 == anchor => last.1 += size,
                _ => runs.push((anchor, size)),
            }
            total += size;
        }
        GapMap { runs }
    }

    pub fn from_sequence(sequence: &[u8]) -> Self {
        let (mut position, mut pending) = (0i64, 0i64);
        let mut runs = Vec::new();
        for &base in sequence {
            if base == b'-' {
                pending += 1;
            } else {
                if pending > 0 {
                    runs.push((position, pending));
                    pending = 0;
                }
                position += 1;
            }
        }
        if pending > 0 {
            runs.push((position, pending));
        }
        GapMap { runs }
    }

    pub fn total(&self) -> i64 {
        self.runs.iter().map(|r| r.1).sum()
    }

    pub fn to_ungapped(&self, mut position: i64) -> i64 {
        for &(anchor, size) in &self.runs {
            if anchor > position {
                break;
            }
            position = anchor.max(position - size);
        }
        position
    }

    pub fn render(&self, sequence: &[u8]) -> Vec<u8> {
        let mut output = Vec::with_capacity(sequence.len() + self.total().max(0) as usize);
        let mut current = 0i64;
        let n = sequence.len() as i64;
        for &(anchor, size) in &self.runs {
            let anchor = current.max(n.min(anchor));
            output.extend_from_slice(slice(sequence, current, anchor));
            output.extend(std::iter::repeat_n(b'-', size.max(0) as usize));
            current = anchor;
        }
        output.extend_from_slice(slice(sequence, current, n));
        output
    }

    pub fn add(&self, other: &GapMap) -> GapMap {
        let mut values: BTreeMap<i64, i64> = self.runs.iter().copied().collect();
        for &(position, size) in &other.runs {
            *values.entry(position).or_insert(0) += size;
        }
        GapMap { runs: values.into_iter().collect() }
    }

    pub fn subtract(&self, other: &GapMap) -> GapMap {
        let values: BTreeMap<i64, i64> = other.runs.iter().copied().collect();
        let get = |p: i64| values.get(&p).copied().unwrap_or(0);
        GapMap { runs: self.runs.iter().filter(|&&(p, s)| s > get(p)).map(|&(p, s)| (p, s - get(p))).collect() }
    }

    pub fn shift(&self, amount: i64) -> GapMap {
        GapMap { runs: self.runs.iter().map(|&(p, s)| (p + amount, s)).collect() }
    }

    pub fn subset(&self, start: i64, end: i64) -> GapMap {
        GapMap { runs: self.runs.iter().copied().filter(|&(p, _)| start <= p && p <= end).collect() }
    }

    pub fn within(&self, start: i64, end: i64) -> i64 {
        self.runs.iter().filter(|&&(p, _)| start <= p && p <= end).map(|r| r.1).sum()
    }

    pub fn project_ungapped(&self, other: &GapMap) -> GapMap {
        let mut result: BTreeMap<i64, i64> = BTreeMap::new();
        for &(p, s) in &other.runs {
            *result.entry(self.to_ungapped(p)).or_insert(0) += s;
        }
        GapMap { runs: result.into_iter().collect() }
    }

    fn first_is_zero(&self) -> Option<i64> {
        match self.runs.first() {
            Some(&(0, size)) => Some(size),
            _ => None,
        }
    }
}

/// One row of the padded alignment: leading padding, the read with its internal gaps, trailing
/// padding.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AlignedRow {
    pub leading: i64,
    pub trailing: i64,
    pub sequence: Vec<u8>,
    pub gaps: GapMap,
}

impl AlignedRow {
    pub fn from_sequence(sequence: &[u8]) -> Self {
        let n = sequence.len();
        let leading = sequence.iter().take_while(|&&b| b == b'-').count();
        let mut trailing = sequence.iter().rev().take_while(|&&b| b == b'-').count();
        if leading == n {
            trailing = 0;
        }
        let end = if trailing > 0 { n - trailing } else { n };
        let internal = sequence.get(leading..end.max(leading)).unwrap_or(&[]);
        AlignedRow {
            leading: leading as i64,
            trailing: trailing as i64,
            sequence: internal.iter().copied().filter(|&b| b != b'-').collect(),
            gaps: GapMap::from_sequence(internal),
        }
    }

    pub fn internal(&self) -> Vec<u8> {
        self.gaps.render(&self.sequence)
    }

    pub fn end(&self) -> i64 {
        self.leading + self.sequence.len() as i64 + self.gaps.total()
    }

    pub fn render(&self) -> Result<Vec<u8>, MapError> {
        if self.leading < 0 || self.trailing < 0 {
            return Err(MapError::new("Negative terminal padding requires circular row adjustment"));
        }
        let mut out = vec![b'-'; self.leading as usize];
        out.extend(self.internal());
        out.extend(std::iter::repeat_n(b'-', self.trailing as usize));
        Ok(out)
    }
}

/// Shared insertion widths and exact row gap insertion (`PaddingBuilder`).
#[derive(Clone, Debug)]
struct PaddingBuilder {
    reference_length: i64,
    circular: bool,
    gaps: BTreeMap<i64, i64>,
    before: i64,
    after: i64,
    extra_circular: i64,
    original_after: i64,
    minimum: i64,
    maximum: i64,
    allow_negative: bool,
    finished: bool,
}

impl PaddingBuilder {
    fn new(reference_length: i64, circular: bool) -> Self {
        PaddingBuilder {
            reference_length,
            circular,
            gaps: BTreeMap::new(),
            before: 0,
            after: 0,
            extra_circular: 0,
            original_after: 0,
            minimum: 0,
            maximum: 0,
            allow_negative: false,
            finished: false,
        }
    }

    fn gap_total(&self) -> i64 {
        self.gaps.values().sum()
    }

    fn length(&self) -> i64 {
        self.maximum - self.minimum
    }

    fn gap(&self, at: i64) -> i64 {
        self.gaps.get(&at).copied().unwrap_or(0)
    }

    fn add_gap(&mut self, anchor: i64, size: i64) -> Result<(), MapError> {
        if !self.circular && anchor >= self.reference_length {
            return Err(MapError::new("Reference gap outside linear sequence"));
        }
        if self.reference_length == 0 {
            return Err(MapError::new("Empty reference in padding"));
        }
        let anchor = py_mod(anchor, self.reference_length);
        let value = self.gap(anchor).max(size);
        self.gaps.insert(anchor, value);
        Ok(())
    }

    fn add_overhang(&mut self, before: i64, after: i64) {
        self.before = self.before.max(before);
        self.after = self.after.max(after);
    }

    fn finish(&mut self) {
        if !self.finished {
            self.minimum = -self.before;
            self.original_after = self.after;
            if self.circular && self.after > 0 {
                self.after = self.to_gapped(self.after - 1) + 1;
            }
            self.maximum = self.reference_length + self.gap_total() + self.after;
            self.finished = true;
        }
    }

    fn add_extra_gaps(&mut self, gaps: &GapMap) -> Result<(), MapError> {
        if !self.finished {
            return Err(MapError::new("Extra gaps require finished padding"));
        }
        self.after = self.original_after;
        self.finished = false;
        for &(position, size) in &gaps.runs {
            self.add_gap(position, size)?;
        }
        self.finish();
        Ok(())
    }

    fn extend_to(&mut self, length: i64) {
        self.maximum = self.maximum.max(self.minimum + length);
    }

    fn set_length(&mut self, length: i64) {
        self.maximum = self.minimum + length;
    }

    fn add_circular_padding(&mut self, length: i64) -> Result<(), MapError> {
        if !self.circular || length < 0 {
            return Err(MapError::new("Invalid circular padding"));
        }
        self.extra_circular += length;
        self.maximum += length;
        Ok(())
    }

    fn circular_gap(&self, position: i64) -> i64 {
        if self.circular && py_mod(position, self.reference_length) == 0 { self.extra_circular } else { 0 }
    }

    fn to_gapped(&self, position: i64) -> i64 {
        if position < 0 {
            return position;
        }
        if !self.circular && position >= self.reference_length {
            return position + self.gap_total();
        }
        let rotations = floor_div(position, self.reference_length);
        let position = py_mod(position, self.reference_length);
        position + self.gaps.range(..=position).map(|(_, s)| s).sum::<i64>() + rotations * (self.reference_length + self.gap_total())
    }

    fn reference_row(&mut self, reference: &[u8]) -> Result<AlignedRow, MapError> {
        self.finish();
        if reference.len() as i64 != self.reference_length {
            return Err(MapError::new("Unexpected ungapped reference length"));
        }
        Ok(AlignedRow {
            leading: self.before,
            trailing: self.after,
            sequence: reference.to_vec(),
            gaps: GapMap { runs: self.gaps.iter().map(|(&p, &s)| (p, s)).collect() },
        })
    }

    fn normalized(&self, current: i64) -> i64 {
        if self.circular { py_mod(current, self.reference_length) } else { current }
    }

    fn project(&self, start: i64, adjustment: i64, query_gaps: &GapMap, reference_gaps: &GapMap, sequence: &[u8]) -> Result<AlignedRow, MapError> {
        if !self.finished {
            return Err(MapError::new("Finish shared padding before projecting rows"));
        }
        let (mut start, mut adjustment) = (start, adjustment);
        if start == 0 && adjustment < 0 && !self.circular {
            start += adjustment;
            adjustment = 0;
        }
        let reference_span = sequence.len() as i64 + query_gaps.total() - reference_gaps.total();
        let mut current = (-1i64).max(start);
        let mut end = start + reference_span - 1;
        if !self.circular {
            end = end.min(self.reference_length - 1);
        }
        let (mut positions, mut sizes) = (Vec::new(), Vec::new());
        let mut inserted = 0;
        for &(original_anchor, own_size) in &reference_gaps.runs {
            let mut anchor = original_anchor;
            if self.circular && self.reference_length > 0 && anchor < current - 1 {
                let k = (current - 1 - anchor + self.reference_length - 1) / self.reference_length;
                anchor += k * self.reference_length;
                while anchor < current - 1 {
                    anchor += self.reference_length;
                }
            }
            if anchor > end {
                continue;
            }
            while anchor > current + 1 {
                current += 1;
                let normalized = self.normalized(current);
                let width = self.gap(normalized) + self.circular_gap(normalized);
                if width != 0 {
                    positions.push(current - start + inserted);
                    sizes.push(width);
                    inserted += width;
                }
            }
            let mut width = self.gap(py_mod(original_anchor, self.reference_length));
            if original_anchor == start {
                width = own_size;
            }
            if width < own_size {
                return Err(MapError::new("Read insertion was not registered in shared padding"));
            }
            if anchor > 0 {
                width += self.circular_gap(anchor);
            }
            let remaining = width - own_size;
            current = anchor;
            if remaining != 0 {
                positions.push(anchor - start + inserted);
                sizes.push(remaining);
            }
            inserted += width;
        }
        while current < end {
            current += 1;
            let normalized = self.normalized(current);
            let width = self.gap(normalized) + self.circular_gap(normalized);
            if width != 0 {
                positions.push(current - start + inserted);
                sizes.push(width);
                inserted += width;
            }
        }
        let extra = GapMap::from_gapped(&positions, &sizes);
        let mut combined = query_gaps.add(&query_gaps.project_ungapped(&extra));
        let gapped_start = self.to_gapped(start) + adjustment;
        let internal_length = combined.total() + sequence.len() as i64;
        let mut leading = gapped_start - self.minimum;
        let trailing = self.maximum - (gapped_start + internal_length);
        if let Some(size) = combined.first_is_zero() {
            leading += size;
            combined = combined.subset(1, BIG);
        }
        if !self.allow_negative && (leading < 0 || trailing < 0) {
            return Err(MapError::new(format!("Insufficient padding: leading={leading}, trailing={trailing}")));
        }
        Ok(AlignedRow { leading, trailing, sequence: sequence.to_vec(), gaps: combined })
    }
}

/// A read as the layout sees it (`LayoutRead`), in reference-facing orientation.
#[derive(Clone, Debug, PartialEq)]
pub struct LayoutRead {
    pub sequence: Vec<u8>,
    pub reference_start: i64,
    pub query_gaps: GapMap,
    pub reference_gaps: GapMap,
    pub adjustment: i64,
    pub mismatch_count: i64,
    pub trim_left: i64,
    pub trim_right: i64,
    pub original_length: i64,
    pub original_coordinates: bool,
    pub trusted_existing: bool,
}

impl LayoutRead {
    fn gap_event_count(&self) -> i64 {
        (self.query_gaps.runs.len() + self.reference_gaps.runs.len()) as i64
    }
}

/// Converts a mapper hit to the reference-facing row (`layout_read_from_hit`). `sequence` is the
/// normalized sequence the mapper saw.
pub fn layout_read_from_hit(
    sequence: &[u8],
    hit: &RankedAlignment,
    reference_length: i64,
    circular: bool,
    original_coordinates: bool,
) -> Result<LayoutRead, MapError> {
    let mut query_gaps = GapMap::from_gapped(&hit.query_gap_positions, &hit.query_gap_sizes);
    let query_span = hit.alignment_length - hit.query_gap_sizes.iter().sum::<i64>();
    let mut oriented = if hit.reverse { reverse_complement(sequence) } else { sequence.to_vec() };
    if circular && query_span < sequence.len() as i64 {
        oriented = slice(&oriented, hit.query_start, hit.query_start + query_span).to_vec();
    }
    let mut position = hit.reference_start;
    if !circular && hit.query_start > 0 {
        if position != 0 {
            return Err(MapError::new("Native linear clipped-start hit must start at reference origin"));
        }
        position = -hit.query_start;
        query_gaps = query_gaps.shift(hit.query_start);
    }
    let (mut inserted, mut i) = (0i64, 0usize);
    let mut reference_gaps: Vec<(i64, i64)> = Vec::new();
    let positions = &hit.ref_gap_positions;
    let sizes = &hit.ref_gap_sizes;
    let size_at = |j: usize| sizes.get(j).copied().ok_or_else(|| MapError::new("Gap sizes and positions differ in length"));
    while i < positions.len() {
        let mut anchor = hit.reference_start + positions[i] - inserted;
        let mut size = size_at(i)?;
        while i + 1 < positions.len() && positions[i + 1] == positions[i] + size_at(i)? {
            i += 1;
            size += size_at(i)?;
        }
        inserted += size;
        if circular {
            anchor = py_mod(anchor, reference_length);
        }
        match reference_gaps.last_mut() {
            Some(last) if last.0 == anchor => last.1 += size,
            _ => reference_gaps.push((anchor, size)),
        }
        i += 1;
    }
    let original_length = oriented.len() as i64;
    Ok(LayoutRead {
        sequence: oriented,
        reference_start: position,
        query_gaps,
        reference_gaps: GapMap::new(reference_gaps),
        adjustment: 0,
        mismatch_count: hit.mismatch_count,
        trim_left: 0,
        trim_right: 0,
        original_length,
        original_coordinates,
        trusted_existing: false,
    })
}

/// The common alignment for new-consensus and retained-original hits (`AlignmentLayout`).
pub struct AlignmentLayout {
    original_reference: Vec<u8>,
    pub circular: bool,
    mapping: Option<ConsensusMapping>,
    original_length: i64,
    reference_length: i64,
    origin_shift: i64,
    consensus_builder: PaddingBuilder,
    original_builder: Option<PaddingBuilder>,
    consensus_gaps: Option<GapMap>,
    original_gaps: Option<GapMap>,
    pub reads: Vec<LayoutRead>,
    ready: bool,
    adjustment: i64,
    reference_start: i64,
    extra_before: i64,
    reverse_rotation: i64,
    rotated_reference_end: i64,
    original_overhang: GapMap,
    original_underhang: GapMap,
    consensus_overhang: GapMap,
    reference_row: Option<AlignedRow>,
}

fn builder_missing() -> MapError {
    MapError::new("Original-coordinate reads require a consensus mapping")
}

impl AlignmentLayout {
    pub fn new(original_reference: &[u8], circular: bool, mapping: Option<&ConsensusMapping>) -> Result<Self, MapError> {
        let original_length = original_reference.len() as i64;
        let reference_length = mapping.map_or(original_length, |m| m.consensus().len() as i64);
        let mut layout = AlignmentLayout {
            original_reference: original_reference.to_vec(),
            circular,
            mapping: mapping.cloned(),
            original_length,
            reference_length,
            origin_shift: mapping.map_or(0, |m| m.origin_shift),
            consensus_builder: PaddingBuilder::new(reference_length, circular),
            original_builder: mapping.map(|_| PaddingBuilder::new(original_length, circular)),
            consensus_gaps: mapping.map(|m| GapMap::new(m.gap_runs(true))),
            original_gaps: mapping.map(|m| GapMap::new(m.gap_runs(false))),
            reads: Vec::new(),
            ready: false,
            adjustment: 0,
            reference_start: 0,
            extra_before: 0,
            reverse_rotation: 0,
            rotated_reference_end: 0,
            original_overhang: GapMap::default(),
            original_underhang: GapMap::default(),
            consensus_overhang: GapMap::default(),
            reference_row: None,
        };
        if let Some(m) = mapping {
            for (position, size) in layout.effective_consensus_gaps().runs {
                if position == 0 && !circular {
                    layout.consensus_builder.add_overhang(size, 0);
                } else {
                    layout.consensus_builder.add_gap(position, size)?;
                }
            }
            if !circular {
                layout.consensus_builder.add_overhang(0, m.trailing_gaps(true));
            }
            let original_gaps = layout.original_gaps.clone().unwrap_or_default();
            let f = layout.original_builder.as_mut().ok_or_else(builder_missing)?;
            for &(position, size) in &original_gaps.runs {
                if position == 0 && !circular {
                    f.add_overhang(size, 0);
                } else {
                    f.add_gap(position, size)?;
                }
            }
            f.add_overhang(0, m.trailing_gaps(false));
        }
        Ok(layout)
    }

    fn effective_consensus_gaps(&self) -> GapMap {
        let gaps = self.consensus_gaps.clone().unwrap_or_default();
        if self.circular
            && let Some(&(position, trailing)) = gaps.runs.last()
            && position == self.reference_length
        {
            return gaps.add(&GapMap::new(vec![(0, trailing)]));
        }
        gaps
    }

    pub fn add_read(&mut self, read: LayoutRead) -> Result<(), MapError> {
        if self.ready {
            return Err(MapError::new("Cannot add reads after finalizing shared columns"));
        }
        let circular = self.circular;
        let builder =
            if read.original_coordinates { self.original_builder.as_mut().ok_or_else(builder_missing)? } else { &mut self.consensus_builder };
        for &(position, size) in &read.reference_gaps.runs {
            builder.add_gap(position, size)?;
        }
        let after =
            read.reference_start + read.sequence.len() as i64 + read.query_gaps.total() - read.reference_gaps.total() - builder.reference_length;
        builder.add_overhang(if !circular { 0.max(-read.reference_start) } else { 0 }, 0.max(after));
        self.reads.push(read);
        Ok(())
    }

    fn remove_insertions_inside_mapping(gaps: &GapMap, offset: i64, shared_gaps: &GapMap, target_gaps: &GapMap, source_gaps: &GapMap) -> GapMap {
        let (mut adjustment, mut source_index, mut gap_index) = (0i64, 0usize, 0usize);
        let mut removed = Vec::new();
        for &(position, size) in &target_gaps.runs {
            let mut anchor = position - offset + adjustment;
            while source_index < source_gaps.runs.len() && source_gaps.runs[source_index].0 + source_gaps.runs[source_index].1 < anchor {
                let amount = source_gaps.runs[source_index].1;
                source_index += 1;
                anchor -= amount;
                adjustment -= amount;
            }
            while gap_index < gaps.runs.len() && gaps.runs[gap_index].0 < anchor {
                gap_index += 1;
            }
            let existing = match gaps.runs.get(gap_index) {
                Some(&(p, s)) if p == anchor => s,
                _ => 0,
            };
            let amount = existing.min(shared_gaps.within(anchor + 1, anchor + size));
            if amount != 0 {
                removed.push((anchor, amount));
            }
            adjustment += size;
        }
        gaps.subtract(&GapMap::new(removed))
    }

    fn project_row(
        &self,
        original_coordinates: bool,
        start: i64,
        adjustment: i64,
        query_gaps: &GapMap,
        ref_gaps: &GapMap,
        sequence: &[u8],
    ) -> Result<AlignedRow, MapError> {
        let builder = if original_coordinates { self.original_builder.as_ref().ok_or_else(builder_missing)? } else { &self.consensus_builder };
        let mut row = builder.project(start, adjustment, query_gaps, ref_gaps, sequence)?;
        if self.circular
            && let Some(gaps) = &self.consensus_gaps
            && let Some(&(position, trailing)) = gaps.runs.last()
            && position == self.reference_length
        {
            if row.leading < trailing {
                return Err(MapError::new("Circular mapping has insufficient leading padding"));
            }
            row.leading -= trailing;
        }
        Ok(row)
    }

    /// The reference row; finalizes the shared columns the first time.
    #[allow(clippy::cognitive_complexity)] // a line-for-line port; see the crate notes
    pub fn reference_row(&mut self) -> Result<AlignedRow, MapError> {
        if self.ready
            && let Some(row) = &self.reference_row
        {
            return Ok(row.clone());
        }
        self.consensus_builder.finish();
        if let Some(f) = self.original_builder.as_mut() {
            f.finish();
        }
        let mut reference = self.original_reference.clone();
        if self.origin_shift != 0 {
            let shift = self.origin_shift;
            let mut rotated = slice(&reference, shift, reference.len() as i64).to_vec();
            rotated.extend_from_slice(slice(&reference, 0, shift));
            reference = rotated;
        }
        if self.mapping.is_none() {
            let row = self.consensus_builder.reference_row(&reference)?;
            self.reference_row = Some(row.clone());
            self.ready = true;
            return Ok(row);
        }
        let empty = GapMap::default();
        let mut m = self.consensus_gaps.clone().unwrap_or_default();
        let mut n = self.original_gaps.clone().unwrap_or_default();
        let (mut start, mut original_adjustment) = (0i64, 0i64);
        if let Some(size) = n.first_is_zero() {
            if !self.circular {
                start = size;
                n = n.subset(1, BIG);
            } else {
                original_adjustment = -size;
            }
        }
        if let Some(size) = m.first_is_zero() {
            if !self.circular {
                start -= size;
                m = m.subset(1, BIG);
            } else {
                self.adjustment = -size;
            }
        }
        self.consensus_gaps = Some(m.clone());
        self.original_gaps = Some(n.clone());
        self.reference_start = start;
        let old_placeholder = vec![b'A'; self.original_length.max(0) as usize];
        let new_placeholder = vec![b'A'; self.reference_length.max(0) as usize];
        let a_run = |amount: i64| vec![b'A'; (amount + 1).max(0) as usize];
        if self.circular {
            let length = self.consensus_builder.length();
            self.consensus_builder.extend_to(length * 2);
            let f = self.original_builder.as_ref().ok_or_else(builder_missing)?;
            self.extra_before = original_adjustment + f.project(0, 0, &empty, &empty, b"G")?.leading;
        }
        let e = &self.consensus_builder;
        let f = self.original_builder.as_ref().ok_or_else(builder_missing)?;
        let replacement_original = GapMap::from_sequence(&e.project(start, self.adjustment, &n, &m, &old_placeholder)?.render()?);
        let replacement_consensus = GapMap::from_sequence(&f.project(-start, original_adjustment, &m, &n, &new_placeholder)?.render()?);
        let existing_original = GapMap::from_sequence(&f.project(0, 0, &empty, &empty, &old_placeholder)?.render()?);
        let existing_consensus = GapMap::from_sequence(&e.project(0, 0, &empty, &empty, &new_placeholder)?.render()?);
        let mut replacement_original = Self::remove_insertions_inside_mapping(&replacement_original, start, &existing_original, &m, &n);
        let mut replacement_consensus = Self::remove_insertions_inside_mapping(&replacement_consensus, -start, &existing_consensus, &n, &m);
        let original_span = self.original_length + n.total() - n.within(self.original_length, BIG);
        let consensus_span = self.reference_length + m.total() - m.within(self.reference_length, BIG);
        let original_end = original_span + start;
        if original_end > consensus_span {
            let amount = original_end - consensus_span;
            let anchor = self.original_length - amount - 1;
            self.original_overhang = f.project(anchor, 0, &empty, &empty, &a_run(amount))?.gaps.shift(anchor);
        } else if consensus_span > original_end {
            let amount = consensus_span - original_end;
            let anchor = self.reference_length - amount - 1;
            self.consensus_overhang = e.project(anchor, 0, &empty, &empty, &a_run(amount))?.gaps.shift(anchor);
        }
        let leading_original = n.within(0, 0) + 0.max(start);
        let leading_consensus = m.within(0, 0) + 0.max(-start);
        if leading_original > leading_consensus {
            let amount = leading_original - leading_consensus;
            self.consensus_overhang = self.consensus_overhang.add(&e.project(0, 0, &empty, &empty, &a_run(amount))?.gaps);
        } else if leading_consensus > leading_original {
            let amount = leading_consensus - leading_original;
            self.original_underhang = f.project(0, 0, &empty, &empty, &a_run(amount))?.gaps;
        }
        if !self.circular
            && let Some(size) = replacement_original.first_is_zero()
        {
            start += size - self.reference_start;
        }
        replacement_original = replacement_original.subset(if self.circular { 0 } else { 1 }, self.original_length - 1);
        start += self.extra_before;
        if let Some(size) = replacement_consensus.first_is_zero() {
            if self.circular {
                self.adjustment = -size;
            } else {
                start -= size + self.reference_start;
            }
        }
        if self.circular {
            start -= f.gap(0);
            let pushed_back = self.effective_consensus_gaps().within(0, 0) - m.within(0, 0);
            if pushed_back > 0 {
                replacement_consensus = replacement_consensus.add(&GapMap::new(vec![(0, pushed_back)]));
                self.original_builder.as_mut().ok_or_else(builder_missing)?.add_overhang(pushed_back, 0);
            }
        }
        if start < 0 {
            self.consensus_builder.add_overhang(-start, 0);
        } else {
            self.original_builder.as_mut().ok_or_else(builder_missing)?.add_overhang(start, 0);
        }
        replacement_consensus = replacement_consensus.subset(if self.circular { 0 } else { 1 }, self.reference_length - 1);
        self.consensus_builder.add_extra_gaps(&replacement_consensus)?;
        self.original_builder.as_mut().ok_or_else(builder_missing)?.add_extra_gaps(&replacement_original)?;
        let mut original_terminal = 0;
        if self.circular {
            self.consensus_builder.add_circular_padding(self.original_overhang.total())?;
            let total = self.consensus_overhang.total();
            self.original_builder.as_mut().ok_or_else(builder_missing)?.add_circular_padding(total)?;
        } else {
            original_terminal = self.original_overhang.total() + self.original_underhang.total();
        }
        let f_length = self.original_builder.as_ref().ok_or_else(builder_missing)?.length();
        let length = self.consensus_builder.length().max(f_length + original_terminal);
        self.consensus_builder.extend_to(length);
        self.original_builder.as_mut().ok_or_else(builder_missing)?.extend_to(length);
        let mut row = self.project_row(false, self.reference_start, self.adjustment, &n, &m, &reference)?;
        row.gaps = row.gaps.add(&self.original_overhang).add(&self.original_underhang);
        row.trailing -= self.original_overhang.total() + self.original_underhang.total();
        row.leading += self.extra_before;
        row.trailing -= self.extra_before;
        if self.origin_shift != 0 {
            let mut remaining = self.origin_shift;
            let rendered = row.render()?;
            let (mut position, mut rotation) = (row.end() - 1, 0i64);
            loop {
                let base = index(&rendered, position).ok_or_else(|| MapError::new("Origin rotation ran off the reference row"))?;
                if base != b'-' {
                    if remaining == 0 {
                        break;
                    }
                    remaining -= 1;
                }
                position -= 1;
                rotation += 1;
            }
            self.reverse_rotation = rotation;
            let end = (rendered.len() - rendered.iter().rev().take_while(|&&b| b == b'-').count()) as i64;
            let total = rendered.len() as i64;
            let mut rotated = slice(&rendered, end - rotation, end).to_vec();
            rotated.extend_from_slice(slice(&rendered, 0, end - rotation));
            rotated.extend_from_slice(slice(&rendered, end, total));
            row = AlignedRow::from_sequence(&rotated);
        }
        self.rotated_reference_end = row.end();
        self.ready = true;
        if self.origin_shift != 0 {
            let mut overhang = -row.trailing;
            for read in &self.reads {
                overhang = overhang.max(-self.project_read(read)?.trailing);
            }
            if overhang != 0 {
                let length = self.consensus_builder.length();
                self.consensus_builder.set_length(length + overhang);
                let f = self.original_builder.as_mut().ok_or_else(builder_missing)?;
                let length = f.length();
                f.set_length(length + overhang);
                row.trailing += overhang;
                self.consensus_builder.allow_negative = true;
                f.allow_negative = true;
            }
        }
        self.reference_row = Some(row.clone());
        Ok(row)
    }

    /// A read's row in the finalized alignment.
    pub fn project_read(&self, read: &LayoutRead) -> Result<AlignedRow, MapError> {
        if !self.ready {
            return Err(MapError::new("Finalize reference_row before projecting reads"));
        }
        let mut row = self.project_row(
            read.original_coordinates,
            read.reference_start,
            read.adjustment,
            &read.query_gaps,
            &read.reference_gaps,
            &read.sequence,
        )?;
        if self.reverse_rotation != 0 {
            let end = self.rotated_reference_end;
            if end <= 0 {
                return Err(MapError::new("Rotated reference row is empty"));
            }
            row.leading += self.reverse_rotation;
            row.trailing -= self.reverse_rotation;
            while row.trailing < 0 || row.leading >= end {
                row.trailing += end;
                row.leading -= end;
            }
            while row.leading < 0 {
                row.trailing -= end;
                row.leading += end;
            }
        }
        let circular_length = self.consensus_builder.gap_total() + self.reference_length;
        if self.circular && read.query_gaps.first_is_zero().is_some() && read.reference_start == self.reference_length - 1 {
            row.leading -= circular_length;
            row.trailing += circular_length;
        }
        if self.circular && row.leading >= circular_length {
            row.leading -= circular_length;
            row.trailing += circular_length;
        }
        Ok(row)
    }

    /// The reads as the iteration consensus counts them.
    pub fn consensus_reads(&mut self) -> Result<Vec<ConsensusRead>, MapError> {
        self.reference_row()?;
        let mut out = Vec::with_capacity(self.reads.len());
        for read in &self.reads {
            let row = self.project_read(read)?;
            out.push(ConsensusRead {
                aligned_sequence: row.internal(),
                mismatch_count: read.mismatch_count,
                alignment_start: row.leading,
                trim_left: read.trim_left,
                trim_right: read.trim_right,
                gap_event_count: read.gap_event_count(),
                original_length: read.original_length,
                trusted_existing: read.trusted_existing,
            });
        }
        Ok(out)
    }
}
