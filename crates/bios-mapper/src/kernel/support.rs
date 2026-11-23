//! `ExtensionSupport.java`: options, the score table, the gap lists and the static helpers the
//! extension and refinement use. Overloaded Java methods `a(...)` are named after what they do;
//! each says which overload it is. A Java exception becomes an `Err`.

pub type K<T> = Result<T, String>;

pub const INT_MAX: i32 = i32::MAX;

/// A byte of a sequence, failing where Java would throw `ArrayIndexOutOfBoundsException`.
pub fn at(s: &[u8], i: i32) -> K<u8> {
    if i < 0 {
        return Err(format!("index {i} out of bounds for length {}", s.len()));
    }
    s.get(i as usize).copied().ok_or_else(|| format!("index {i} out of bounds for length {}", s.len()))
}

pub fn at_i(s: &[i32], i: i32) -> K<i32> {
    if i < 0 {
        return Err(format!("index {i} out of bounds for length {}", s.len()));
    }
    s.get(i as usize).copied().ok_or_else(|| format!("index {i} out of bounds for length {}", s.len()))
}

/// Java's `Math.max(float, float)` for the finite values the kernel uses (+0 above -0).
pub fn fmax(a: f32, b: f32) -> f32 {
    if a.is_nan() || b.is_nan() {
        return f32::NAN;
    }
    if a == 0.0 && b == 0.0 {
        return if a.is_sign_negative() { b } else { a };
    }
    if a >= b { a } else { b }
}

/// Default reference-mapping options (`ExtensionSupport.b`). `k` and `l` stay true, as in the
/// original server, which never changes them.
#[derive(Clone, Debug)]
pub struct Options {
    pub a: i32,
    pub q: i32,
    /// Java field `B`.
    pub b_remaining: i32,
    /// Java field `C`.
    pub c_upper: i32,
    pub k: bool,
    pub l: bool,
    pub n: f32,
    pub gap_percent: i32,
    pub mismatch_percent: i32,
    pub max_mismatches: i32,
}

impl Default for Options {
    fn default() -> Self {
        Options { a: 50, q: 0, b_remaining: 3, c_upper: 0, k: true, l: true, n: 0.5, gap_percent: 15, mismatch_percent: 30, max_mismatches: INT_MAX }
    }
}

impl Options {
    /// `b.a(long)`: permitted gaps.
    pub fn gaps(&self, length: i64) -> i32 {
        let mut count = (self.gap_percent as i64 * length / 100) as i32;
        if self.gap_percent > 0 && count == 0 {
            count = 1;
        }
        if self.a == 0 { 0 } else { count }
    }

    /// `b.b(long)`: permitted mismatches.
    pub fn mismatches(&self, length: i64) -> i32 {
        let mut count = (self.mismatch_percent as i64 * length / 100) as i32;
        if self.mismatch_percent > 0 && count == 0 {
            count = 1;
        }
        count.min(self.max_mismatches)
    }

    /// `b.s(int)`: the gap cost, with diminishing extensions.
    pub fn s(&self, length: i32) -> f32 {
        if length == 0 {
            return 0.0;
        }
        let mut cost: f32 = -5.0 + length.min(4) as f32 * -6.0;
        let mut remaining = length - 4;
        let mut extension: f32 = -6.0;
        let mut block = 0;
        while remaining > 0 && block < 5 {
            cost += remaining.min(4) as f32 * extension;
            remaining -= 4;
            block += 1;
            extension /= 2.0;
        }
        cost
    }
}

/// The extension result (`ExtensionSupport.e`).
#[derive(Clone, Debug, Default)]
pub struct ResultE {
    pub a: f32,
    pub b: f32,
    pub c: i32,
    pub d: i32,
    pub e: i32,
    pub f: i32,
    pub g: i32,
    pub l: i32,
    pub n: i32,
    pub o: i32,
    pub h: Vec<i32>,
    pub i: Vec<i32>,
    pub j: Vec<i32>,
    pub k: Vec<i32>,
    pub p: bool,
    pub q: bool,
}

impl ResultE {
    /// `e.a()`: the reset state.
    pub fn reset(&mut self) {
        self.n = -1;
        self.o = -1;
        self.p = false;
        self.a = 0.0;
        self.b = 0.0;
        self.c = -1;
        self.d = -1;
        self.g = -1;
        self.l = 0;
        self.h = Vec::new();
        self.i = Vec::new();
        self.j = Vec::new();
        self.k = Vec::new();
        self.q = false;
        self.e = -1;
        self.f = -1;
    }
}

/// A run-length gap list (`ExtensionSupport.h`): positions, lengths and their total.
#[derive(Clone, Debug, Default)]
pub struct GapList {
    pub pos: Vec<i32>,
    pub len: Vec<i32>,
    pub c: i32,
}

impl GapList {
    /// `h.a(int, int)`: adds a gap, extending the last one when it ends where this starts.
    pub fn add(&mut self, position: i32, length: i32) -> K<()> {
        if length == 0 {
            return Err("IllegalArgumentException".into());
        }
        if !self.pos.is_empty() && self.end()? == position {
            if let Some(last) = self.len.last_mut() {
                *last += length;
            }
        } else {
            self.pos.push(position);
            self.len.push(length);
        }
        self.c += length;
        Ok(())
    }

    /// `h.a()`
    pub fn count(&self) -> i32 {
        self.pos.len() as i32
    }

    /// `h.b()`: where the last gap ends.
    pub fn end(&self) -> K<i32> {
        Ok(self.last_pos()? + self.last_len()?)
    }

    /// `h.c()`
    pub fn last_pos(&self) -> K<i32> {
        self.pos.last().copied().ok_or_else(|| "ArrayIndexOutOfBoundsException".to_string())
    }

    /// `h.d()`
    pub fn last_len(&self) -> K<i32> {
        self.len.last().copied().ok_or_else(|| "ArrayIndexOutOfBoundsException".to_string())
    }

    /// `h.c(int)`: shortens (or removes) the last gap.
    pub fn shorten(&mut self, length: i32) -> K<()> {
        let previous = self.last_len()?;
        if length > previous {
            return Err("IllegalArgumentException".into());
        }
        self.c -= length;
        if length == previous {
            self.pos.pop();
            self.len.pop();
        } else if let Some(last) = self.len.last_mut() {
            *last -= length;
        }
        Ok(())
    }
}

/// `Encoder.a(byte)`: the complementary symbol.
pub fn complement(symbol: u8) -> u8 {
    const FROM: &[u8] = b"ACGTRYKMSWBDHVNUacgtrykmswbdhvnu";
    const TO: &[u8] = b"TGCAYRMKSWVHDBNAtgcayrmkswvhdbna";
    match FROM.iter().position(|&x| x == symbol) {
        Some(i) => TO.get(i).copied().unwrap_or(symbol),
        None => symbol,
    }
}

/// `ExtensionSupport.Scores`: the 127 by 127 nucleotide score table.
pub struct Scores {
    table: Vec<[f32; 127]>,
}

impl Scores {
    pub fn new() -> Self {
        let alphabet = b"ACGTRYSWKMBDHVNIU?";
        let bases: [&[u8]; 18] =
            [b"A", b"C", b"G", b"T", b"AG", b"CT", b"CG", b"AT", b"GT", b"AC", b"CGT", b"AGT", b"ACT", b"ACG", b"ACGT", b"AGT", b"T", b"ACGT"];
        let mut table = vec![[0f32; 127]; 127];
        let mut set = |a: u8, b: u8, v: f32| {
            if let Some(row) = table.get_mut(a as usize)
                && let Some(cell) = row.get_mut(b as usize)
            {
                *cell = v;
            }
        };
        for (left, &a) in alphabet.iter().enumerate() {
            for (right, &b) in alphabet.iter().enumerate() {
                let mut total: f32 = 0.0;
                for x in bases[left] {
                    for y in bases[right] {
                        total += if x == y { 5.0 } else { -4.0 };
                    }
                }
                let value = total / (bases[left].len() * bases[right].len()) as i32 as f32;
                let (la, lb) = (a.to_ascii_lowercase(), b.to_ascii_lowercase());
                set(a, b, value);
                set(la, b, value);
                set(a, lb, value);
                set(la, lb, value);
            }
        }
        for symbol in 0..127usize {
            if symbol != b'N' as usize
                && symbol != b'?' as usize
                && let Some(row) = table.get_mut(symbol)
                && let Some(cell) = row.get_mut(symbol)
                && *cell < 1.0
            {
                *cell = 1.0;
            }
        }
        Scores { table }
    }

    /// `score[a][b]`.
    pub fn get(&self, a: u8, b: u8) -> K<f32> {
        self.table.get(a as usize).and_then(|row| row.get(b as usize)).copied().ok_or_else(|| format!("score index {a},{b} out of bounds"))
    }
}

/// `b(float)`: a positive score.
pub fn positive(v: f32) -> bool {
    v > 0.0
}

/// `a(byte)`: `Character.isLowerCase`.
pub fn lower(b: u8) -> bool {
    b.is_ascii_lowercase()
}

/// `a(Scores, byte, byte, boolean)`.
pub fn same(s: &Scores, a: u8, b: u8, exact: bool) -> K<bool> {
    Ok(a == b || (!exact && positive(s.get(a, b)?)))
}

/// `a(Scores, byte, byte, boolean, boolean)`: matches, complementing the first when asked.
pub fn matches(s: &Scores, a: u8, b: u8, complement_first: bool, exact: bool) -> K<bool> {
    if complement_first { same(s, complement(a), b, exact) } else { same(s, a, b, exact) }
}

/// `b(Scores, byte, byte, boolean)`: one score, complementing the first when asked.
pub fn score_of(s: &Scores, a: u8, b: u8, complement_first: bool) -> K<f32> {
    s.get(if complement_first { complement(a) } else { a }, b)
}

fn wrap(mut p: i32, n: i32) -> i32 {
    while p < 0 {
        p += n;
    }
    while p >= n {
        p -= n;
    }
    p
}

/// The 21-argument `a(...)`: the length matched from a position, allowing mismatches (and
/// recursing on lowercase or gap alternatives). The structural-graph arguments are always null.
pub fn matched_length(
    sc: &Scores,
    r: &[u8],
    mut p1: i32,
    circular: bool,
    q: &[u8],
    mut p2: i32,
    d1: i32,
    d2: i32,
    max: i32,
    mut mismatches: i32,
    lower_branches: i32,
    gap_branches: i32,
    mut var12: bool,
    var13: bool,
    exact: bool,
    depth: u32,
) -> K<f32> {
    if depth > 64 {
        return Err("recursion too deep".into());
    }
    let n1 = r.len() as i32;
    let mut var21: f32 = 0.0;
    let mut var22: i32 = 0;
    let var23 = d1 != d2;
    loop {
        let advanced = 'label201: {
            if (circular || (p1 >= 0 && p1 < n1)) && p2 >= 0 && p2 < q.len() as i32 {
                if var22 >= max {
                    return Ok(var22 as f32);
                }
                if circular {
                    p1 = wrap(p1, n1);
                }
                if lower_branches > 0 && var12 && lower(at(r, p1)?) {
                    let a = var22 as f32
                        + matched_length(
                            sc,
                            r,
                            p1,
                            circular,
                            q,
                            p2,
                            d1,
                            d2,
                            max - var22,
                            mismatches,
                            lower_branches - 1,
                            gap_branches,
                            false,
                            var13,
                            exact,
                            depth + 1,
                        )?;
                    let b = var22 as f32
                        + matched_length(
                            sc,
                            r,
                            p1 + d1,
                            circular,
                            q,
                            p2,
                            d1,
                            d2,
                            max - var22,
                            mismatches,
                            lower_branches - 1,
                            gap_branches,
                            true,
                            var13,
                            exact,
                            depth + 1,
                        )?;
                    return Ok(fmax(a, b));
                }
                if lower_branches > 0 && var12 && lower(at(q, p2)?) {
                    let a = matched_length(
                        sc,
                        r,
                        p1,
                        circular,
                        q,
                        p2,
                        d1,
                        d2,
                        max,
                        mismatches,
                        lower_branches - 1,
                        gap_branches,
                        false,
                        var13,
                        exact,
                        depth + 1,
                    )?;
                    let b = matched_length(
                        sc,
                        r,
                        p1,
                        circular,
                        q,
                        p2 + d2,
                        d1,
                        d2,
                        max,
                        mismatches,
                        lower_branches - 1,
                        gap_branches,
                        true,
                        var13,
                        exact,
                        depth + 1,
                    )?;
                    return Ok(fmax(a, b));
                }
                if matches(sc, at(r, p1)?, at(q, p2)?, var23, exact)? {
                    var22 += 1;
                    break 'label201 true;
                }
                if mismatches > 0 {
                    mismatches -= 1;
                    if gap_branches > 0 && var12 {
                        let a = (var22 - 1) as f32
                            + matched_length(
                                sc,
                                r,
                                p1 + d1,
                                circular,
                                q,
                                p2,
                                d1,
                                d2,
                                max - var22,
                                mismatches,
                                lower_branches,
                                gap_branches - 1,
                                true,
                                var13,
                                exact,
                                depth + 1,
                            )?;
                        let b = (var22 - 1) as f32
                            + matched_length(
                                sc,
                                r,
                                p1,
                                circular,
                                q,
                                p2 + d2,
                                d1,
                                d2,
                                max - var22,
                                mismatches,
                                lower_branches,
                                gap_branches - 1,
                                true,
                                var13,
                                exact,
                                depth + 1,
                            )?;
                        if a > var21 {
                            var21 = a;
                        }
                        if b > var21 {
                            var21 = b;
                        }
                    }
                    if var13 {
                        var22 += 1;
                    }
                    break 'label201 true;
                }
            }
            false
        };
        if !advanced {
            return Ok(fmax(var22 as f32, var21));
        }
        p1 += d1;
        p2 += d2;
        var12 = true;
    }
}

/// The 15-argument `a(...)`: [`matched_length`] with two lowercase branches, no gap branches.
pub fn matched_count(
    sc: &Scores,
    r: &[u8],
    p1: i32,
    circular: bool,
    q: &[u8],
    p2: i32,
    d1: i32,
    d2: i32,
    max: i32,
    mismatches: i32,
    exact: bool,
) -> K<i32> {
    Ok(matched_length(sc, r, p1, circular, q, p2, d1, d2, max, mismatches, 2, 0, true, true, exact, 0)? as i32)
}

/// `a(byte[], int, boolean, byte[], int, int, int, char)`: the exact run from a position.
pub fn exact_run(r: &[u8], mut p1: i32, circular: bool, q: &[u8], mut p2: i32, d1: i32, d2: i32, stop: u8) -> K<i32> {
    let mut count = 0;
    let complement_first = d1 != d2;
    let n1 = r.len() as i32;
    while (circular || (p1 >= 0 && p1 < n1)) && p2 >= 0 && p2 < q.len() as i32 {
        if circular {
            p1 = wrap(p1, n1);
        }
        let mut a = at(r, p1)?;
        let b = at(q, p2)?;
        if complement_first {
            a = complement(a);
        }
        if a != b || a == stop {
            return Ok(count);
        }
        count += 1;
        p1 += d1;
        p2 += d2;
    }
    Ok(count)
}

/// The 12-argument boolean `a(...)`: whether a window matches with at most `allowed` mismatches.
pub fn window_matches(
    sc: &Scores,
    r: &[u8],
    mut p1: i32,
    circular: bool,
    q: &[u8],
    mut p2: i32,
    length: i32,
    mut allowed: i32,
    direction: i32,
    last_may_differ_first: bool,
    exact: bool,
    wildcard: u8,
) -> K<bool> {
    if length == 0 {
        return Ok(false);
    }
    let n1 = r.len() as i32;
    if circular {
        p1 = wrap(p1, n1);
    }
    for i in 0..length {
        let a = at(r, p1)?;
        let b = at(q, p2)?;
        if !matches(sc, a, b, direction < 0, exact)? {
            if a != wildcard && b != wildcard {
                if !last_may_differ_first && i == 0 {
                    return Ok(false);
                }
                if last_may_differ_first && i == length - 1 {
                    return Ok(false);
                }
            }
            allowed -= 1;
            if allowed < 0 {
                return Ok(false);
            }
        }
        p1 += direction;
        p2 += 1;
        if circular && p1 == n1 {
            p1 = 0;
        }
        if circular && p1 == -1 {
            p1 = n1 - 1;
        }
    }
    Ok(true)
}

/// The 10-argument int `a(...)`: the penalty for mismatches just behind a gap.
pub fn lookbehind_penalty(
    sc: &Scores,
    r: &[u8],
    mut p1: i32,
    circular: bool,
    q: &[u8],
    mut p2: i32,
    var6: i32,
    var7: i32,
    d1: i32,
    d2: i32,
) -> K<i32> {
    let mut total: i8 = 0;
    let outside: i8 = -4;
    let mut mismatch: i8 = -4;
    let count = var6.max(var7.min(3));
    if count >= 4 {
        mismatch = -3;
    }
    if count >= 6 {
        mismatch = -2;
    }
    let complement_first = d1 != d2;
    let n1 = r.len() as i32;
    for _ in 0..count {
        p1 += d1;
        p2 += d2;
        if circular {
            p1 = wrap(p1, n1);
        }
        if p1 >= 0 && p1 < n1 && p2 >= 0 && p2 < q.len() as i32 {
            if !matches(sc, at(r, p1)?, at(q, p2)?, complement_first, false)? {
                total = total.wrapping_add(mismatch);
            }
        } else {
            total = total.wrapping_add(outside);
        }
    }
    Ok(total as i32)
}

/// The 9-argument float `a(...)`: the summed score over a stretch.
pub fn score_sum(sc: &Scores, r: &[u8], mut p1: i32, circular: bool, q: &[u8], mut p2: i32, d1: i32, d2: i32, length: i32) -> K<f32> {
    let mut total: f32 = 0.0;
    let n1 = r.len() as i32;
    if circular {
        p1 = wrap(p1, n1);
    }
    for _ in 0..length {
        let mut a = at(r, p1)?;
        let b = at(q, p2)?;
        if d1 != d2 {
            a = complement(a);
        }
        total += sc.get(a, b)?;
        p1 += d1;
        p2 += d2;
        if circular && p1 == n1 {
            p1 = 0;
        }
        if circular && p1 == -1 {
            p1 = n1 - 1;
        }
    }
    Ok(total)
}

/// The 10-argument float `b(...)`: the better of two stretch scores.
pub fn best_score_sum(sc: &Scores, r: &[u8], p1: i32, circular: bool, q: &[u8], p2: i32, d1: i32, d2: i32, l1: i32, l2: i32) -> K<f32> {
    Ok(fmax(score_sum(sc, r, p1, circular, q, p2, d1, d2, l1)?, score_sum(sc, r, p1, circular, q, p2, d1, d2, l2)?))
}

/// `b(int, int, int, int)`.
pub fn min4(a: i32, b: i32, c: i32, d: i32) -> i32 {
    a.min(b).min(c.min(d))
}

/// `b(int, int, int)`.
pub fn min3(a: i32, b: i32, c: i32) -> i32 {
    if a < b { a.min(c) } else { b.min(c) }
}

/// The 8-argument long `a(...)`: the longest alignment the seed allows.
pub fn possible_length(r_len: i32, circular: bool, q_len: i32, ref_seed: i32, query_seed: i32) -> i64 {
    let mut length = r_len.min(q_len) as i64;
    if !circular {
        let before = ref_seed.min(query_seed);
        let after = (r_len - ref_seed).min(q_len - query_seed);
        length = ((before + after) as i64).min(length);
    }
    length
}

/// `a(h, h, int)`: the first gap position of either list, or `limit`.
pub fn first_gap(a: &GapList, b: &GapList, limit: i32) -> i32 {
    let mut value = limit;
    if let Some(&p) = a.pos.first() {
        value = value.min(p);
    }
    if let Some(&p) = b.pos.first() {
        value = value.min(p);
    }
    value
}

/// The 8-argument int `a(...)`: mismatches over a stretch.
pub fn mismatch_count(sc: &Scores, r: &[u8], q: &[u8], mut p1: i32, mut p2: i32, length: i32, circular1: bool, circular2: bool) -> K<i32> {
    let mut count = 0;
    for _ in 0..length {
        while circular1 && p1 >= r.len() as i32 {
            p1 -= r.len() as i32;
        }
        while circular2 && p2 >= q.len() as i32 {
            p2 -= q.len() as i32;
        }
        if !positive(sc.get(at(r, p1)?, at(q, p2)?)?) {
            count += 1;
        }
        p1 += 1;
        p2 += 1;
    }
    Ok(count)
}

/// The 14-argument int `a(...)`: how far a gap slides forward, adding the score gained to `out`.
pub fn slide_gap(
    active: bool,
    sc: &Scores,
    s1: &[u8],
    mut i1: i32,
    d1: i32,
    circular1: bool,
    s2: &[u8],
    mut i2: i32,
    d2: i32,
    circular2: bool,
    gap: i32,
    limit: i32,
    out: &mut f32,
    unbounded: bool,
) -> K<i32> {
    *out = 0.0;
    if !active {
        return Ok(0);
    }
    let mut existing: f32 = 0.0;
    let mut moved: f32 = 0.0;
    let mut far = i1 + gap * d1;
    let complement_first = d1 != d2;
    let mut result = 0;
    let steps = if unbounded { limit } else { limit.min(gap + 2) };
    let (n1, n2) = (s1.len() as i32, s2.len() as i32);
    for step in 0..steps {
        while circular1 && i1 >= n1 {
            i1 -= n1;
        }
        while circular1 && i1 < 0 {
            i1 += n1;
        }
        while circular1 && far >= n1 {
            far -= n1;
        }
        while circular1 && far < 0 {
            far += n1;
        }
        while circular2 && i2 >= n2 {
            i2 -= n2;
        }
        while circular2 && i2 < 0 {
            i2 += n2;
        }
        existing += score_of(sc, at(s1, far)?, at(s2, i2)?, complement_first)?;
        moved += score_of(sc, at(s1, i1)?, at(s2, i2)?, complement_first)?;
        if moved >= existing {
            *out += moved;
            result = step + 1;
            existing = 0.0;
            moved = 0.0;
        }
        i1 += d1;
        i2 += d2;
        far += d1;
    }
    Ok(result)
}

/// The 12-argument boolean `a(...)` taking two sequences first: whether a lowercase base is
/// better treated as a gap.
pub fn lowercase_is_gap(r: &[u8], circular: bool, q: &[u8], sc: &Scores, p1: i32, p2: i32, d1: i32, d2: i32) -> K<bool> {
    let stay = matched_length(sc, r, p1, circular, q, p2, d1, d2, 10, 0, 4, -1, false, false, false, 0)? as i32;
    let skip = matched_length(sc, r, p1 + d1, circular, q, p2, d1, d2, 10, 0, 4, -1, true, false, false, 0)? as i32;
    Ok(skip > stay)
}

/// `a(int, int, boolean, int)`: an index moved and wrapped.
pub fn moved(mut index: i32, by: i32, circular: bool, length: i32) -> i32 {
    index += by;
    if circular {
        while index < 0 {
            index += length;
        }
        while index >= length {
            index -= length;
        }
    }
    index
}

/// The 19-argument int `a(...)`: how far back a gap slides. Structural lists are always empty and
/// translation is off.
pub fn slide_gap_back(
    active: bool,
    mut var1: i32,
    s1: &[u8],
    var3: i32,
    var4: i32,
    circular1: bool,
    var6: &GapList,
    s2: &[u8],
    var8: i32,
    var9: i32,
    circular2: bool,
    var11: &GapList,
    sc: &Scores,
    var13: i32,
    var14: i32,
    out: &mut f32,
) -> K<i32> {
    *out = 0.0;
    if !active {
        return Ok(var1);
    }
    let (n1, n2) = (s1.len() as i32, s2.len() as i32);
    let mut var19 = var3 + var4;
    let mut var20 = var8 + var9;
    if circular1 {
        var19 = wrap(var19, n1);
    }
    if circular2 {
        var20 = wrap(var20, n2);
    }
    let mut var21 = -var14 + 1;
    if var11.c > 0 {
        var21 = var21.max(var11.end()?);
    }
    if var6.c > 0 {
        var21 = var21.max(var6.end()?);
    }
    let complement_first = var4 != var9;
    while (circular1 || var19 < n1) && (circular2 || var20 < n2) && var1 > var21 {
        let mut found = false;
        let mut var24: f32 = 0.0;
        let mut var25: f32 = 0.0;
        let var26 = 1.max(var1 - var21);
        let mut var27 = 1;
        while var27 <= var26 && !found {
            let var28 = var27 - 1;
            let var29 = moved(var19, var28 * var4, circular1, n1);
            let var30 = moved(var19, (var28 - var13) * var4, circular1, n1);
            let var31 = moved(var20, var28 * var9, circular2, n2);
            if var29 < 0 || var30 < 0 || var31 < 0 || var29 >= n1 || var31 >= n2 {
                return Err("IllegalStateException".into());
            }
            var24 += score_of(sc, at(s1, var29)?, at(s2, var31)?, complement_first)?;
            var25 += score_of(sc, at(s1, var30)?, at(s2, var31)?, complement_first)?;
            if var25 >= var24 {
                var1 -= var27;
                var19 = moved(var19, var27 * var4, circular1, n1);
                var20 = moved(var20, var27 * var9, circular2, n2);
                found = true;
            }
            var27 += 1;
        }
        if !found {
            break;
        }
    }
    Ok(var1)
}

/// `a(h, h, int, boolean)`: the gap positions (or sizes) of two lists as one array: the first
/// list reversed into backward coordinates, then the second.
pub fn gap_array(var0: &GapList, var1: &GapList, var2: i32, sizes: bool) -> Vec<i32> {
    let var4 = var0.count();
    let var5 = var1.count();
    let mut out = Vec::with_capacity((var4 + var5) as usize);
    for var7 in 0..var4 {
        let idx = (var4 - 1 - var7) as usize;
        let size = var0.len.get(idx).copied().unwrap_or(0);
        if sizes {
            out.push(size);
        } else {
            out.push(var2 - 1 - var0.pos.get(idx).copied().unwrap_or(0) - (size - 1));
        }
    }
    for var9 in 0..var5 as usize {
        if sizes {
            out.push(var1.len.get(var9).copied().unwrap_or(0));
        } else {
            out.push(var2 + var1.pos.get(var9).copied().unwrap_or(0));
        }
    }
    out
}
