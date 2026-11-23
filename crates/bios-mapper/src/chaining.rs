//! `candidate_chaining.py`: the seed graph, its directional extensions and the raw alignments
//! read off it, before gap refinement and ranking. Nodes live in an arena; the original's object
//! identity becomes an index.

use std::collections::HashMap;

use crate::MapError;
use crate::dna::reverse_complement;
use crate::kernel::{ExtensionResult, Kernel};
use crate::primitives::{float32, nucleotide_score};
use crate::py::py_mod;
use crate::search::{CandidateSearch, InitialExtension};

#[derive(Clone, Debug)]
pub struct RawAlignment {
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

#[derive(Clone, Debug)]
struct Segment {
    length: i64,
    mismatch_count: i64,
    score: f64,
    score_gain: f64,
    ref_positions: Vec<i64>,
    query_positions: Vec<i64>,
    ref_sizes: Vec<i64>,
    query_sizes: Vec<i64>,
}

impl Segment {
    fn extension(result: &ExtensionResult, support: f64) -> Self {
        Segment {
            length: result.alignment_length,
            mismatch_count: result.mismatch_count,
            score: float32(result.score - support),
            score_gain: result.score_gain,
            ref_positions: result.ref_gap_positions.clone(),
            query_positions: result.query_gap_positions.clone(),
            ref_sizes: result.ref_gap_sizes.clone(),
            query_sizes: result.query_gap_sizes.clone(),
        }
    }

    fn mirrored(&self, destination_length: i64, source_length: i64) -> Self {
        let bonus = (source_length - destination_length).max(0) as f64 * 2.0 * 5.0;
        Segment { score_gain: float32(self.score_gain + bonus), ..self.clone() }
    }

    /// Native `a.equals` deliberately leaves out the score gain.
    fn equivalent(&self, other: &Segment) -> bool {
        self.length == other.length
            && self.mismatch_count == other.mismatch_count
            && self.score == other.score
            && self.ref_positions == other.ref_positions
            && self.query_positions == other.query_positions
            && self.ref_sizes == other.ref_sizes
            && self.query_sizes == other.query_sizes
    }
}

type Key = (i64, i64, i64, i64);

#[derive(Clone, Debug)]
struct Node {
    reference_id: i64,
    reference_start: i64,
    query_start: i64,
    length: i64,
    right: Option<Segment>,
    next: Option<usize>,
    left: Option<Segment>,
    previous: Option<usize>,
    processed: bool,
    next_failure: i64,
    previous_failure: i64,
}

impl Node {
    fn new(reference_id: i64, reference_start: i64, query_start: i64, length: i64) -> Self {
        Node {
            reference_id,
            reference_start,
            query_start,
            length,
            right: None,
            next: None,
            left: None,
            previous: None,
            processed: false,
            next_failure: 0,
            previous_failure: 0,
        }
    }

    fn key(&self) -> Key {
        (self.reference_id, self.reference_start, self.query_start, self.length)
    }

    fn contains(&self, other: &Node) -> bool {
        self.reference_id == other.reference_id
            && self.reference_start <= other.reference_start
            && self.reference_start + self.length >= other.reference_start + other.length
            && self.query_start <= other.query_start
            && self.query_start + self.length >= other.query_start + other.length
    }
}

fn percent_count(percent: i64, length: i64) -> i64 {
    let count = percent * length / 100;
    if percent > 0 { count.max(1) } else { 0 }
}

fn missing() -> MapError {
    MapError::new("Seed graph node is missing an extension")
}

struct Chainer<'a> {
    query: &'a [u8],
    reference: &'a [u8],
    circular: bool,
    kernel: &'a Kernel,
    reverse: bool,
    word_length: i64,
    nodes: Vec<Node>,
    index: HashMap<Key, usize>,
    sorted: Option<Vec<usize>>,
}

impl<'a> Chainer<'a> {
    fn canonical(&mut self, node: Node) -> usize {
        if let Some(&i) = self.index.get(&node.key()) {
            return i;
        }
        let i = self.nodes.len();
        self.index.insert(node.key(), i);
        self.nodes.push(node);
        i
    }

    fn ordered(&mut self) -> Vec<usize> {
        if self.sorted.is_none() {
            let direction = if self.reverse { 1 } else { -1 };
            let mut order: Vec<usize> = (0..self.nodes.len()).collect();
            let nodes = &self.nodes;
            order.sort_by_key(|&i| {
                let n = &nodes[i];
                (-n.length, -n.reference_id, direction * n.reference_start, direction * n.query_start)
            });
            self.sorted = Some(order);
        }
        self.sorted.clone().unwrap_or_default()
    }

    fn base(&self, position: i64) -> u8 {
        let n = self.reference.len() as i64;
        if self.circular {
            self.reference.get(py_mod(position, n) as usize).copied().unwrap_or(b'!')
        } else if 0 <= position && position < n {
            self.reference.get(position as usize).copied().unwrap_or(b'!')
        } else {
            b'!'
        }
    }

    /// Python indexing of the query: negative counts from the end, past the end fails.
    fn q(&self, i: i64) -> Result<u8, MapError> {
        let n = self.query.len() as i64;
        let j = if i < 0 { i + n } else { i };
        if j < 0 || j >= n {
            return Err(MapError::new("Query index out of range in chaining"));
        }
        Ok(self.query[j as usize])
    }

    fn exact_seed(&self, reference_id: i64, mut ref_position: i64, mut query_position: i64) -> Result<Node, MapError> {
        if self.base(ref_position) != self.q(query_position)? {
            return Ok(Node::new(reference_id, ref_position, query_position, 0));
        }
        while query_position > 0 && self.base(ref_position - 1) == self.q(query_position - 1)? && self.q(query_position - 1)? != b'N' {
            query_position -= 1;
            ref_position -= 1;
        }
        let (ref_start, query_start) = (ref_position, query_position);
        let mut length = 0;
        while query_position < self.query.len() as i64 && self.base(ref_position) == self.q(query_position)? && self.q(query_position)? != b'N' {
            length += 1;
            query_position += 1;
            ref_position += 1;
        }
        Ok(Node::new(reference_id, ref_start, query_start, length))
    }

    /// `node` is either a node of the arena or a fresh one not yet in it.
    fn add_extension(&mut self, node: NodeRef, result: &ExtensionResult, support: f64, direction: i64) -> Result<bool, MapError> {
        let (node_key, node_query_start, node_value) = match &node {
            NodeRef::Arena(i) => {
                let n = &self.nodes[*i];
                (n.key(), n.query_start, n.clone())
            }
            NodeRef::Fresh(n) => (n.key(), n.query_start, (**n).clone()),
        };
        let (ref_position, query_position) = if direction > 0 {
            (
                result.reference_start + result.alignment_length - result.ref_gap_sizes.iter().sum::<i64>() - 1,
                node_query_start - result.query_left + result.alignment_length - result.query_gap_sizes.iter().sum::<i64>() - 1,
            )
        } else {
            (result.reference_start, node_query_start - result.query_left)
        };
        let mut endpoint = self.exact_seed(result.reference_id, ref_position, query_position)?;
        // The containment check occurs before circular normalization in the app.
        if endpoint.key() != node_key && endpoint.contains(&node_value) {
            return Ok(false);
        }
        if self.circular {
            endpoint.reference_start = py_mod(endpoint.reference_start, self.reference.len() as i64);
        }
        let node = match node {
            NodeRef::Arena(i) => i,
            NodeRef::Fresh(n) => self.canonical(*n),
        };
        let segment = Segment::extension(result, support);
        let endpoint_key = endpoint.key();
        let link = endpoint.length >= self.word_length && endpoint_key != self.nodes[node].key();
        if direction > 0 {
            if self.nodes[node].right.is_some() {
                return Err(MapError::new("Seed already has a forward extension"));
            }
            self.nodes[node].right = Some(segment);
            if link {
                let next = self.canonical(endpoint);
                self.nodes[node].next = Some(next);
                self.nodes[next].next_failure = 0;
            }
        } else {
            if self.nodes[node].left.is_some() {
                return Err(MapError::new("Seed already has a backward extension"));
            }
            self.nodes[node].left = Some(segment);
            if link {
                let previous = self.canonical(endpoint);
                self.nodes[node].previous = Some(previous);
                self.nodes[previous].previous_failure = 0;
            }
        }
        self.sorted = None;
        Ok(true)
    }

    fn add_initial(&mut self, item: &InitialExtension) -> Result<(), MapError> {
        let seed = &item.seed;
        let fresh = Node::new(seed.reference_id, seed.reference_start(), seed.query_start, seed.length);
        // The same fresh node is passed twice; once it is in the arena both calls see that node.
        let as_ref = |chainer: &Chainer| match chainer.index.get(&fresh.key()) {
            Some(&i) => NodeRef::Arena(i),
            None => NodeRef::Fresh(Box::new(fresh.clone())),
        };
        if !self.reverse && item.right.score > 0.0 {
            let r = as_ref(self);
            self.add_extension(r, &item.right, item.right_support, 1)?;
        }
        if item.left.score > 0.0 {
            let r = as_ref(self);
            self.add_extension(r, &item.left, item.left_support, -1)?;
        }
        if self.reverse && item.right.score > 0.0 {
            let r = as_ref(self);
            self.add_extension(r, &item.right, item.right_support, 1)?;
        }
        Ok(())
    }

    fn mirror_missing(&mut self) -> Result<bool, MapError> {
        let mut changed = false;
        for i in self.ordered() {
            let (next, previous) = (self.nodes[i].next, self.nodes[i].previous);
            let can_next = match next {
                Some(nx) => {
                    let n = &self.nodes[nx];
                    n.previous.is_none() && n.previous_failure > 0 && {
                        match &n.left {
                            None => true,
                            Some(left) => left.score <= self.nodes[i].right.as_ref().ok_or_else(missing)?.score,
                        }
                    }
                }
                None => false,
            };
            let can_previous = match previous {
                Some(pv) => {
                    let p = &self.nodes[pv];
                    p.next.is_none() && p.next_failure > 0 && {
                        match &p.right {
                            None => true,
                            Some(right) => right.score <= self.nodes[i].left.as_ref().ok_or_else(missing)?.score,
                        }
                    }
                }
                None => false,
            };
            if let (true, Some(nx)) = (can_next, next) {
                let mirrored = self.nodes[i].right.as_ref().ok_or_else(missing)?.mirrored(self.nodes[nx].length, self.nodes[i].length);
                let n = &mut self.nodes[nx];
                n.left = Some(mirrored);
                n.previous = Some(i);
                n.next_failure = 0;
                changed = true;
            }
            if let (true, Some(pv)) = (can_previous, previous) {
                let mirrored = self.nodes[i].left.as_ref().ok_or_else(missing)?.mirrored(self.nodes[pv].length, self.nodes[i].length);
                let p = &mut self.nodes[pv];
                p.right = Some(mirrored);
                p.next = Some(i);
                p.previous_failure = 0;
                changed = true;
            }
        }
        Ok(changed)
    }

    fn mirror_better(&mut self) -> Result<bool, MapError> {
        fn better(first: &Segment, second: &Segment, ties: bool) -> bool {
            (first.score == second.score && ties && !first.equivalent(second)) || first.score > second.score
        }
        let mut changed = false;
        for i in self.ordered() {
            if let Some(nx) = self.nodes[i].next
                && self.nodes[nx].previous == Some(i)
            {
                let candidate = self.nodes[nx].left.as_ref().ok_or_else(missing)?;
                let own = self.nodes[i].right.as_ref().ok_or_else(missing)?;
                if better(candidate, own, self.reverse) {
                    let mirrored = candidate.mirrored(self.nodes[i].length, self.nodes[nx].length);
                    self.nodes[i].right = Some(mirrored);
                    self.nodes[i].previous_failure = 0;
                    changed = true;
                }
            }
            if let Some(pv) = self.nodes[i].previous
                && self.nodes[pv].next == Some(i)
            {
                let candidate = self.nodes[pv].right.as_ref().ok_or_else(missing)?;
                let own = self.nodes[i].left.as_ref().ok_or_else(missing)?;
                if better(candidate, own, !self.reverse) {
                    let mirrored = candidate.mirrored(self.nodes[i].length, self.nodes[pv].length);
                    self.nodes[i].left = Some(mirrored);
                    self.nodes[i].next_failure = 0;
                    changed = true;
                }
            }
        }
        Ok(changed)
    }

    fn extend(&self, i: usize, direction: i64, support: f64) -> Result<ExtensionResult, MapError> {
        let n = &self.nodes[i];
        self.kernel.extend_seed(
            self.query,
            self.reference,
            self.circular,
            n.reference_start,
            n.query_start,
            n.length,
            direction,
            self.word_length,
            support,
            50,
        )
    }

    fn complete_graph(&mut self) -> Result<(), MapError> {
        let q = self.query.len() as i64;
        let budget = ((percent_count(30, q) + percent_count(15, q)).min(1000) * 4) as f64;
        let mut changed = true;
        let mut rounds = 0u64;
        while changed {
            changed = self.mirror_missing()?;
            let mut retry = true;
            while retry {
                rounds += 1;
                if rounds > 1_000_000 {
                    return Err(MapError::new("Seed graph did not settle"));
                }
                retry = false;
                for i in self.ordered() {
                    let (has_next, has_previous) = (self.nodes[i].next.is_some(), self.nodes[i].previous.is_some());
                    let node = &self.nodes[i];
                    let try_previous = node.left.is_none() && (node.previous_failure == 0 || (node.previous_failure == 1 && has_next));
                    let try_next = node.right.is_none() && (node.next_failure == 0 || (node.next_failure == 1 && has_previous));
                    let mut previous_support = node.right.as_ref().map_or(0.0, |s| s.score_gain);
                    if has_next {
                        previous_support = float32(previous_support + budget);
                    }
                    let mut next_support = node.left.as_ref().map_or(0.0, |s| s.score_gain);
                    if has_previous {
                        next_support = float32(next_support + budget);
                    }
                    let directions: [i64; 2] = if self.reverse { [1, -1] } else { [-1, 1] };
                    for direction in directions {
                        if direction < 0 && try_previous {
                            let result = self.extend(i, -1, previous_support)?;
                            if result.score <= 0.0 {
                                self.nodes[i].previous_failure = if has_next { 2 } else { 1 };
                                retry = true;
                            } else {
                                if result.reference_id < 0 {
                                    return Err(MapError::new("Successful extension has no reference"));
                                }
                                if self.add_extension(NodeRef::Arena(i), &result, previous_support, -1)? {
                                    self.nodes[i].next_failure = 0;
                                    retry = true;
                                }
                            }
                        }
                        if direction > 0 && try_next {
                            let result = self.extend(i, 1, next_support)?;
                            if result.score <= 0.0 {
                                self.nodes[i].next_failure = if has_previous { 2 } else { 1 };
                                retry = true;
                            } else {
                                if result.reference_id < 0 {
                                    return Err(MapError::new("Successful extension has no reference"));
                                }
                                if self.add_extension(NodeRef::Arena(i), &result, next_support, 1)? {
                                    self.nodes[i].previous_failure = 0;
                                    retry = true;
                                }
                            }
                        }
                    }
                }
                if retry {
                    changed = true;
                }
            }
            if self.mirror_missing()? {
                changed = true;
            }
            if self.mirror_better()? {
                changed = true;
            }
        }
        Ok(())
    }

    fn materialize(&mut self, start: usize) -> Result<Option<RawAlignment>, MapError> {
        let mut seeds: Vec<usize> = Vec::new();
        let mut segments: Vec<Option<Segment>> = Vec::new();
        let mut node = Some(start);
        let mut mark = true;
        let mut visited = vec![false; self.nodes.len()];
        while let Some(i) = node {
            if visited[i] {
                return Err(MapError::new("Cycle in backward seed chain"));
            }
            visited[i] = true;
            if mark {
                self.nodes[i].processed = true;
            }
            seeds.push(i);
            segments.push(self.nodes[i].left.clone());
            if let Some(p) = self.nodes[i].previous
                && self.nodes[p].next != Some(i)
            {
                mark = false;
            }
            node = self.nodes[i].previous;
        }
        seeds.reverse();
        segments.reverse();
        segments.push(self.nodes[start].right.clone());
        mark = matches!(self.nodes[start].next, Some(nx) if self.nodes[nx].previous == Some(start));
        node = self.nodes[start].next;
        let mut visited = vec![false; self.nodes.len()];
        while let Some(i) = node {
            if visited[i] {
                return Err(MapError::new("Cycle in forward seed chain"));
            }
            visited[i] = true;
            if mark {
                self.nodes[i].processed = true;
            }
            seeds.push(i);
            segments.push(self.nodes[i].right.clone());
            if let Some(nx) = self.nodes[i].next
                && self.nodes[nx].previous != Some(i)
            {
                mark = false;
            }
            node = self.nodes[i].next;
        }
        if seeds.iter().any(|&i| self.nodes[i].left.is_none() || self.nodes[i].right.is_none()) {
            return Ok(None);
        }
        let segments: Vec<Segment> = segments.into_iter().collect::<Option<Vec<_>>>().ok_or_else(missing)?;
        let (Some(first), Some(&seed0)) = (segments.first(), seeds.first()) else {
            return Ok(None);
        };
        let seed = &self.nodes[seed0];
        let query_start = seed.query_start + seed.length - first.length + first.query_sizes.iter().sum::<i64>();
        let mut reference_start = seed.reference_start + seed.length - first.length + first.ref_sizes.iter().sum::<i64>();
        if query_start != 0 && reference_start != 0 {
            return Err(MapError::new("Chained alignment does not reach a query or reference end"));
        }
        if self.circular {
            reference_start = py_mod(reference_start, self.reference.len() as i64);
        }
        let mut result = RawAlignment {
            reference_id: seed.reference_id,
            query_start,
            reference_start,
            alignment_length: 0,
            reverse: self.reverse,
            score: 0.0,
            mismatch_count: 0,
            query_gap_positions: Vec::new(),
            ref_gap_positions: Vec::new(),
            query_gap_sizes: Vec::new(),
            ref_gap_sizes: Vec::new(),
        };
        for (index, segment) in segments.iter().enumerate() {
            let offset = result.alignment_length;
            result.ref_gap_positions.extend(segment.ref_positions.iter().map(|p| p + offset));
            result.ref_gap_sizes.extend(segment.ref_sizes.iter().copied());
            result.query_gap_positions.extend(segment.query_positions.iter().map(|p| p + offset));
            result.query_gap_sizes.extend(segment.query_sizes.iter().copied());
            result.alignment_length += segment.length;
            result.mismatch_count += segment.mismatch_count;
            result.score = float32(result.score + segment.score);
            if index < segments.len() - 1 {
                let seed = &self.nodes[seeds[index]];
                result.alignment_length -= seed.length;
                let mut score = 0.0;
                for offset in 0..seed.length {
                    score = float32(score + nucleotide_score(self.base(seed.reference_start + offset), self.q(seed.query_start + offset)?)?);
                }
                result.score = float32(result.score - score);
            }
        }
        normalize_overlapping_gaps(&mut result)?;
        Ok(Some(result))
    }

    fn raw_alignments(&mut self) -> Result<Vec<RawAlignment>, MapError> {
        self.complete_graph()?;
        let mut result = Vec::new();
        for i in self.ordered() {
            if !self.nodes[i].processed {
                self.nodes[i].processed = true;
                if let Some(alignment) = self.materialize(i)? {
                    result.push(alignment);
                }
            }
        }
        Ok(result)
    }
}

enum NodeRef {
    Arena(usize),
    Fresh(Box<Node>),
}

fn get(v: &[i64], i: usize) -> Result<i64, MapError> {
    v.get(i).copied().ok_or_else(|| MapError::new("Gap index out of range"))
}

fn sub(v: &mut [i64], i: usize, amount: i64) -> Result<(), MapError> {
    *v.get_mut(i).ok_or_else(|| MapError::new("Gap index out of range"))? -= amount;
    Ok(())
}

fn set(v: &mut [i64], i: usize, value: i64) -> Result<(), MapError> {
    *v.get_mut(i).ok_or_else(|| MapError::new("Gap index out of range"))? = value;
    Ok(())
}

/// Native post-merge cancellation and coalescing of overlapping gaps.
pub fn normalize_overlapping_gaps(alignment: &mut RawAlignment) -> Result<(), MapError> {
    const BIG: i64 = 2147483647;
    let mut guard = 0u32;
    loop {
        guard += 1;
        if guard > 100_000 {
            return Err(MapError::new("Gap normalization did not settle"));
        }
        let (mut qi, mut ri, mut end, mut removed) = (0usize, 0usize, 0i64, 0i64);
        let mut previous_query = false;
        while qi < alignment.query_gap_positions.len() || ri < alignment.ref_gap_positions.len() {
            let q = alignment.query_gap_positions.get(qi).copied().unwrap_or(BIG);
            let r = alignment.ref_gap_positions.get(ri).copied().unwrap_or(BIG);
            if q < r {
                set(&mut alignment.query_gap_positions, qi, q - removed)?;
                if q < end {
                    if !previous_query {
                        let overlap = (end - q).min(get(&alignment.query_gap_sizes, qi)?);
                        let rj = ri.checked_sub(1).ok_or_else(|| MapError::new("Gap index out of range"))?;
                        sub(&mut alignment.ref_gap_sizes, rj, overlap)?;
                        sub(&mut alignment.query_gap_sizes, qi, overlap)?;
                        alignment.alignment_length -= overlap;
                        end -= overlap;
                        removed += overlap;
                    }
                    set(&mut alignment.query_gap_positions, qi, end)?;
                }
                end = get(&alignment.query_gap_positions, qi)? + get(&alignment.query_gap_sizes, qi)?;
                previous_query = true;
                qi += 1;
            } else {
                set(&mut alignment.ref_gap_positions, ri, r - removed)?;
                if r < end {
                    if previous_query {
                        let overlap = (end - r).min(get(&alignment.ref_gap_sizes, ri)?);
                        let qj = qi.checked_sub(1).ok_or_else(|| MapError::new("Gap index out of range"))?;
                        sub(&mut alignment.query_gap_sizes, qj, overlap)?;
                        sub(&mut alignment.ref_gap_sizes, ri, overlap)?;
                        alignment.alignment_length -= overlap;
                        end -= overlap;
                        removed += overlap;
                    }
                    set(&mut alignment.ref_gap_positions, ri, end)?;
                }
                end = get(&alignment.ref_gap_positions, ri)? + get(&alignment.ref_gap_sizes, ri)?;
                previous_query = false;
                ri += 1;
            }
        }
        let length = alignment.alignment_length;
        for (positions, sizes) in
            [(&mut alignment.query_gap_positions, &mut alignment.query_gap_sizes), (&mut alignment.ref_gap_positions, &mut alignment.ref_gap_sizes)]
        {
            if sizes.len() < positions.len() {
                return Err(MapError::new("Gap sizes and positions differ in length"));
            }
            for index in (1..positions.len()).rev() {
                if positions[index - 1] + sizes[index - 1] == positions[index] {
                    sizes[index - 1] += sizes[index];
                    sizes[index] = 0;
                }
            }
            for index in 0..positions.len() {
                if positions[index] + sizes[index] > length {
                    sizes[index] = (length - positions[index]).max(0);
                }
            }
            let kept: Vec<(i64, i64)> = positions.iter().zip(sizes.iter()).filter(|(_, s)| **s > 0).map(|(p, s)| (*p, *s)).collect();
            *positions = kept.iter().map(|k| k.0).collect();
            *sizes = kept.iter().map(|k| k.1).collect();
        }
        if removed == 0 {
            return Ok(());
        }
    }
}

/// Raw candidates for a normalized query, forward strand then reverse, in native order.
pub fn chain_candidates(query: &[u8], search: &CandidateSearch, kernel: &Kernel, special_search: bool) -> Result<Vec<RawAlignment>, MapError> {
    let reverse = reverse_complement(query);
    let requests = search.initial_extensions(query, kernel, special_search)?;
    let mut out = Vec::new();
    let mut start = 0;
    while start < requests.len() {
        let strand = requests[start].seed.reverse;
        let mut end = start;
        while end < requests.len() && requests[end].seed.reverse == strand {
            end += 1;
        }
        let mut chainer = Chainer {
            query: if strand { &reverse } else { query },
            reference: &search.reference,
            circular: search.circular,
            kernel,
            reverse: strand,
            word_length: search.word_length,
            nodes: Vec::new(),
            index: HashMap::new(),
            sorted: None,
        };
        for item in &requests[start..end] {
            chainer.add_initial(item)?;
        }
        out.extend(chainer.raw_alignments()?);
        start = end;
    }
    Ok(out)
}
