//! `AlignmentRefiner.java`: final gap placement for one DNA alignment, sliding each gap left and
//! right until the alignment stops changing, then rescoring it.

use super::support::{INT_MAX, K, Options, Scores, at, at_i, min3, positive, slide_gap};

/// `AlignmentRefiner.Options`: the default options with `c = false` and `D = 1`.
struct RefineOptions {
    base: Options,
    d: i32,
}

/// `AlignmentRefiner.Hit`. `k`/`m` are the query gap positions and sizes, `l`/`n` the reference's.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Hit {
    pub c: f32,
    pub f: i32,
    pub g: i32,
    pub h: i32,
    pub o: i32,
    pub p: i32,
    pub q: i32,
    pub j: bool,
    pub t: bool,
    pub k: Vec<i32>,
    pub l: Vec<i32>,
    pub m: Vec<i32>,
    pub n: Vec<i32>,
}

fn sum(values: &[i32]) -> i32 {
    values.iter().sum()
}

impl Hit {
    /// `equals`: `Float.compare` on the score, then every field.
    fn same_as(&self, other: &Hit) -> bool {
        self.c.to_bits() == other.c.to_bits()
            && self.f == other.f
            && self.g == other.g
            && self.h == other.h
            && self.o == other.o
            && self.p == other.p
            && self.q == other.q
            && self.j == other.j
            && self.t == other.t
            && self.k == other.k
            && self.l == other.l
            && self.m == other.m
            && self.n == other.n
    }

    /// `b(Hit)`: copies the alignment fields of another hit.
    fn copy_from(&mut self, other: &Hit) {
        *self = other.clone();
    }

    /// `f()`: the query span.
    fn query_span(&self) -> i32 {
        self.h - sum(&self.m)
    }

    /// `g()`: the reference span.
    fn reference_span(&self) -> i32 {
        self.h - sum(&self.n)
    }

    /// The 8-argument `a(...)`: alternate the two directions until nothing changes.
    fn settle(&mut self, var1: bool, var2: &RefineOptions, var3: &Scores, var4: &[u8], var5: &[u8], var6: bool, var8: bool) -> K<()> {
        let mut guard = 0u32;
        fn next_round(guard: &mut u32) -> K<()> {
            *guard += 1;
            if *guard > 100_000 { Err("refinement did not settle".into()) } else { Ok(()) }
        }
        if var8 {
            loop {
                next_round(&mut guard)?;
                let var9 = self.clone();
                self.reversed_pass(var1, var2, var3, var4, var5, var6, true)?;
                self.shift_gaps(var1, var2, var3, var4, var5, var6, false, true)?;
                if var9.same_as(self) {
                    break;
                }
            }
        } else {
            loop {
                next_round(&mut guard)?;
                let var9 = self.clone();
                self.shift_gaps(var1, var2, var3, var4, var5, var6, false, true)?;
                self.reversed_pass(var1, var2, var3, var4, var5, var6, true)?;
                if var9.same_as(self) {
                    break;
                }
            }
            self.shift_gaps(var1, var2, var3, var4, var5, var6, false, false)?;
        }
        self.rescore(&var2.base, var3, var4, var5, var6, false)?;
        Ok(())
    }

    /// The 7-argument `a(...)`: try both orders and keep the better.
    fn refine(&mut self, var1: bool, var2: &RefineOptions, var3: &Scores, var4: &[u8], var5: &[u8], var6: bool) -> K<()> {
        if !self.k.is_empty() || !self.l.is_empty() {
            self.rescore(&var2.base, var3, var4, var5, var6, false)?;
            let mut var8 = self.clone();
            var8.settle(var1, var2, var3, var4, var5, var6, false)?;
            let mut var9 = self.clone();
            var9.settle(var1, var2, var3, var4, var5, var6, true)?;
            if !(var8.c > var9.c) && (var8.c != var9.c || var8.o >= var9.o) {
                self.copy_from(&var9);
            } else {
                self.copy_from(&var8);
            }
        }
        Ok(())
    }

    /// The private 8-argument `b(...)`: a gap-shifting pass on the reversed alignment.
    fn reversed_pass(&mut self, var1: bool, var2: &RefineOptions, var3: &Scores, var4: &[u8], var5: &[u8], var6: bool, var8: bool) -> K<()> {
        let mut var9 = self.clone();
        var9.flip(var5.len() as i32, false)?;
        var9.shift_gaps(var1, var2, var3, var4, var5, var6, true, var8)?;
        var9.flip(var5.len() as i32, true)?;
        self.o = var9.o;
        self.h = var9.h;
        self.f = var9.f;
        self.g = var9.g;
        self.c = var9.c;
        self.k = var9.k;
        self.m = var9.m;
        self.l = var9.l;
        self.n = var9.n;
        Ok(())
    }

    /// `a(int, boolean)`: reverse the alignment, keeping the query start where the caller wants it.
    fn flip(&mut self, var1: i32, var2: bool) -> K<()> {
        let var3 = self.query_span() - 1;
        let var4 = if var2 { self.f - var3 } else { self.f + var3 };
        reverse_hit(self, var1)?;
        self.f = var4;
        Ok(())
    }

    /// The 9-argument `a(...)`: slide every gap as far as it improves the score.
    #[allow(clippy::cognitive_complexity)]
    fn shift_gaps(
        &mut self,
        _var1: bool,
        var2: &RefineOptions,
        var3: &Scores,
        var4: &[u8],
        var5: &[u8],
        var6: bool,
        var8: bool,
        var9: bool,
    ) -> K<()> {
        if self.k.is_empty() && self.l.is_empty() {
            return Ok(());
        }
        let mut rounds = 0u32;
        'label279: loop {
            rounds += 1;
            if rounds > 100_000 {
                return Err("gap shifting did not settle".into());
            }
            let mut var11: i32 = 0;
            let mut var12: i32 = 0;
            let mut var13 = self.f;
            let mut var14 = self.g;
            let mut var15 = if var6 {
                INT_MAX
            } else if var8 {
                var14 + 1
            } else {
                var4.len() as i32 - var14
            };
            let mut var16 = if var8 { var13 + 1 } else { var5.len() as i32 - var13 };
            let var17 = var2.base.c_upper;
            let var18 = var2.d;
            let mut var19 = super::support::GapList::default();
            let mut var20 = super::support::GapList::default();
            let mut var21: i32 = 0;
            let mut var22: i32 = 0;
            let mut var23: i32 = 0;
            let mut var24: i32 = 0;
            let mut var25: i32 = 0;
            let var26 = if var8 { self.f - self.query_span() + 1 } else { var5.len() as i32 - self.f - self.query_span() };
            let var27 = if var6 {
                INT_MAX
            } else if var8 {
                self.g - self.reference_span() + 1
            } else {
                var4.len() as i32 - self.g - self.reference_span()
            };
            if !(var26 >= 0 && var27 >= 0) {
                return Err("IllegalStateException".into());
            }
            let var28 = if var8 { -1 } else { 1 };
            let mut var10 = false;
            let mut var29: i32 = 0;
            let klen = self.k.len() as i32;
            let llen = self.l.len() as i32;
            let mut steps = 0u64;

            loop {
                steps += 1;
                if steps > 10_000_000 {
                    return Err("gap shifting did not finish".into());
                }
                let var30 = if var11 < klen { at_i(&self.k, var11)? - var23 } else { INT_MAX };
                let var31 = if var12 < llen { at_i(&self.l, var12)? - var24 } else { INT_MAX };
                let var32 = var30.min(var31);
                if var32 >= self.h {
                    if var10 {
                        self.k = var19.pos.clone();
                        self.m = var19.len.clone();
                        self.l = var20.pos.clone();
                        self.n = var20.len.clone();
                        self.h -= var25;
                    }
                    if !var10 {
                        return Ok(());
                    }
                    continue 'label279;
                }
                let var33 = var32 - var29;
                if var33 <= 0 {
                    if var30 <= var29 {
                        if at_i(&self.k, var11)? - var23 != var29 {
                            return Err("IllegalStateException: query gap out of step".into());
                        }
                        let mut var48 = at_i(&self.k, var11)? - var23;
                        let mut var51 = at_i(&self.m, var11)? + var23;
                        var23 = 0;
                        let var53 = var22;
                        var22 = 0;
                        var51 -= var53;
                        if var51 < 0 {
                            return Err("IllegalStateException".into());
                        }
                        var29 += var53;
                        if var51 == 0 {
                            var11 += 1;
                        } else {
                            let var54 = if var11 + 1 < klen { at_i(&self.k, var11 + 1)? } else { self.h };
                            let var55 = if var12 < llen { at_i(&self.l, var12)? } else { self.h };
                            let var56 = var54.min(var55);
                            let mut out = 0f32;
                            let var57 = slide_gap(
                                true,
                                var3,
                                var4,
                                var14,
                                var28,
                                var6,
                                var5,
                                var13,
                                var28,
                                false,
                                var51,
                                min3(var15 - var51, var16, var56 - var48 - var51 - var53),
                                &mut out,
                                true,
                            )?;
                            if var57 != 0 {
                                var10 = true;
                            }
                            var48 += var57;
                            let var58 = var48 - var25 + var53;
                            if var54 == var48 + var51 && var54 != self.h {
                                var23 = var51;
                                var11 += 1;
                            } else {
                                let var59 = var55 - (var48 + var51);
                                let var60 = var55 == self.h && var55 == var56 && var59 <= var18;
                                let var61 = var55 == var56 && var59 <= var17;
                                if var9 && (var60 || var61) {
                                    let var62 = if var12 < self.n.len() as i32 { at_i(&self.n, var12)? } else { INT_MAX };
                                    var21 = var62.min(var51);
                                    let mut var63 = var21;
                                    var10 = true;
                                    if var62 == INT_MAX {
                                        var63 = 0.max(var21 - var26);
                                    }
                                    var25 += var63;
                                    var51 -= var21;
                                    if var51 <= 0 {
                                        var11 += 1;
                                        continue;
                                    }
                                }
                                var19.add(var58, var51)?;
                                if var8 {
                                    var14 -= var57 + var51;
                                } else {
                                    var14 += var57 + var51;
                                }
                                var15 -= var57 + var51;
                                if var8 {
                                    var13 -= var57;
                                } else {
                                    var13 += var57;
                                }
                                var16 -= var57;
                                var11 += 1;
                                var29 += var57 + var51;
                            }
                        }
                    } else {
                        if var31 > var29 {
                            return Err("IllegalStateException".into());
                        }
                        if at_i(&self.l, var12)? - var24 != var29 {
                            return Err("IllegalStateException: reference gap out of step".into());
                        }
                        let mut var34 = at_i(&self.l, var12)? - var24;
                        let mut var35 = at_i(&self.n, var12)? + var24;
                        var24 = 0;
                        let var36 = var21;
                        var21 = 0;
                        var35 -= var36;
                        if var35 < 0 {
                            return Err("IllegalStateException".into());
                        }
                        var29 += var36;
                        if var35 == 0 {
                            var12 += 1;
                        } else {
                            let var37 = if var11 < klen { at_i(&self.k, var11)? } else { self.h };
                            let var38 = if var12 + 1 < llen { at_i(&self.l, var12 + 1)? } else { self.h };
                            let var39 = var37.min(var38);
                            let mut out = 0f32;
                            let var40 = slide_gap(
                                true,
                                var3,
                                var5,
                                var13,
                                var28,
                                false,
                                var4,
                                var14,
                                var28,
                                var6,
                                var35,
                                min3(var15, var16 - var35, var39 - var34 - var35 - var36),
                                &mut out,
                                true,
                            )?;
                            if var40 != 0 {
                                var10 = true;
                            }
                            var34 += var40;
                            let var41 = var34 - var25 + var36;
                            if var38 == var34 + var35 && var38 != self.h {
                                var24 = var35;
                                var12 += 1;
                            } else {
                                let var42 = var37 - (var34 + var35);
                                let var43 = var38 == self.h && var38 == var39 && var42 <= var18;
                                let var44 = var37 == var39 && var42 <= var17;
                                if var9 && (var43 || var44) {
                                    let var45 = if var11 < self.m.len() as i32 { at_i(&self.m, var11)? } else { INT_MAX };
                                    var22 = var45.min(var35);
                                    let mut var46 = var22;
                                    var10 = true;
                                    if var45 == INT_MAX {
                                        var46 = 0.max(var22 - var27);
                                    }
                                    var25 += var46;
                                    var35 -= var22;
                                    if var35 <= 0 {
                                        var12 += 1;
                                        continue;
                                    }
                                }
                                var20.add(var41, var35)?;
                                if var8 {
                                    var13 -= var40 + var35;
                                } else {
                                    var13 += var40 + var35;
                                }
                                var16 -= var40 + var35;
                                if var8 {
                                    var14 -= var40;
                                } else {
                                    var14 += var40;
                                }
                                var15 -= var40;
                                var12 += 1;
                                var29 += var40 + var35;
                            }
                        }
                    }
                } else {
                    if var8 {
                        var13 -= var33;
                        var14 -= var33;
                    } else {
                        var13 += var33;
                        var14 += var33;
                    }
                    var15 -= var33;
                    var16 -= var33;
                    var29 += var33;
                }
            }
        }
    }

    /// The 7-argument boolean `a(...)`: rescore and recount mismatches; whether the score changed.
    fn rescore(&mut self, var1: &Options, var2: &Scores, var3: &[u8], var4: &[u8], var5: bool, var6: bool) -> K<bool> {
        let mut var11 = 0usize;
        let mut var12 = 0usize;
        let mut var13 = self.f;
        let mut var14 = self.g;
        let mut var15: f32 = 0.0;
        let mut var16 = 0;
        let n3 = var3.len() as i32;
        let mut var19 = 0;
        while var19 < self.h {
            if var11 < self.k.len() && self.k.get(var11) == Some(&var19) {
                let var23 = self.m.get(var11).copied().ok_or("ArrayIndexOutOfBoundsException")?;
                var15 += var1.s(var23);
                var14 += if var6 { -var23 } else { var23 };
                var11 += 1;
                var19 += var23;
                var19 -= 1;
            } else if var12 < self.l.len() && self.l.get(var12) == Some(&var19) {
                let var22 = self.n.get(var12).copied().ok_or("ArrayIndexOutOfBoundsException")?;
                var15 += var1.s(var22);
                var13 += if var6 { -var22 } else { var22 };
                var12 += 1;
                var19 += var22;
                var19 -= 1;
            } else {
                if var5 {
                    while var14 < 0 {
                        var14 += n3;
                    }
                    while var14 >= n3 {
                        var14 -= n3;
                    }
                }
                if var14 >= n3 || var13 >= var4.len() as i32 {
                    return Err("IllegalStateException: position past the sequence".into());
                }
                let var18 = var2.get(at(var3, var14)?, at(var4, var13)?)?;
                if !positive(var18) {
                    var16 += 1;
                }
                var15 += var18;
                if var6 {
                    var13 -= 1;
                    var14 -= 1;
                } else {
                    var13 += 1;
                    var14 += 1;
                }
            }
            var19 += 1;
        }
        let changed = var15 != self.c;
        self.c = var15;
        self.o = var16;
        Ok(changed)
    }
}

/// The static `b(Hit, int)`: the same alignment seen from the other end of the query.
fn reverse_hit(hit: &mut Hit, query_length: i32) -> K<()> {
    let start = query_length - hit.query_span() - hit.f;
    if start > query_length {
        return Err("IllegalStateException".into());
    }
    hit.g += (if hit.t { -1 } else { 1 }) * (hit.reference_span() - 1);
    hit.f = start;
    hit.t = !hit.t;
    hit.j = !hit.j;
    std::mem::swap(&mut hit.p, &mut hit.q);
    let flip = |positions: &[i32], sizes: &[i32]| -> K<(Vec<i32>, Vec<i32>)> {
        let count = positions.len();
        let mut p = Vec::with_capacity(count);
        let mut s = Vec::with_capacity(sizes.len());
        for i in 0..count {
            let old = count - 1 - i;
            let size = sizes.get(old).copied().ok_or("ArrayIndexOutOfBoundsException")?;
            p.push(hit.h - positions.get(old).copied().unwrap_or(0) - size);
            s.push(size);
        }
        // Java sizes the second array by its own length; any extra entries stay zero.
        s.resize(sizes.len().max(count), 0);
        Ok((p, s))
    };
    let (l, n) = flip(&hit.l, &hit.n)?;
    let (k, m) = flip(&hit.k, &hit.m)?;
    hit.l = l;
    hit.n = n;
    hit.k = k;
    hit.m = m;
    Ok(())
}

/// `AlignmentRefiner.refine`.
pub fn refine(hit: &mut Hit, reference: &[u8], query: &[u8], circular: bool, scores: &Scores) -> K<()> {
    let options = RefineOptions { base: Options::default(), d: 1 };
    hit.refine(false, &options, scores, reference, query, circular)
}
