//! `SeedExtension.java`: one directional gapped extension from an exact seed. Variable names keep
//! the Java numbering so each statement can be checked against the original. Doubles stay
//! doubles and floats stay floats, as Java computes them.

use super::support::exact_run;
use super::support::{
    GapList, INT_MAX, K, Options, ResultE, Scores, at, best_score_sum, first_gap, gap_array, lookbehind_penalty, lower, lowercase_is_gap,
    matched_count, min3, min4, mismatch_count, positive, possible_length, score_sum, slide_gap, slide_gap_back, window_matches,
};

const WILDCARD: u8 = b'N';

/// `SeedExtension.extend` for one DNA query with no structural graph, gap mask or translation.
pub fn extend(
    var7: &[u8],
    var8: bool,
    var24: &[u8],
    var10: i32,
    var11: i32,
    var12: i32,
    var13: i32,
    var14: i32,
    var15: &Scores,
    var16: &Options,
    var19: i32,
    var20: i32,
    var21: f32,
    var22: i32,
) -> K<ResultE> {
    let var23 = false;
    let var26 = var7.len() as i32;
    let var27 = var24.len() as i32;
    let mut var29 = ResultE::default();
    let var30 = WILDCARD;
    let mut var31 = var19 != -1;
    let mut var32 = var19 != 1;
    let var33 = false;
    let var34 = false;
    let var35 = var19 == 0;
    let var36 = var35;
    let mut var37: i32 = 0;
    let mut var38 = false;
    let mut var39: f32 = var21;
    let mut var41: i32 = 0;
    let mut var42: f64 = 0.0;
    let mut var44 = var10;
    let mut var45 = var11;

    for var46 in 0..var12 {
        while var8 && var44 >= var26 {
            var44 -= var26;
        }
        let var49 = var15.get(at(var7, var44)?, at(var24, var45)?)?;
        var39 += var49;
        if !positive(var49) {
            var41 += 1;
        }
        var44 += 1;
        var45 += 1;
        if var46 == var13 + var14 - 1 {
            var42 = (var39 - var21) as f64;
        }
    }
    if var42 <= 0.0 {
        var42 = var39 as f64;
    }

    let var137: f32 = var39;
    let mut var138: f64 = var39 as f64;
    var29.a = var39;
    var29.b = 0.0;
    var29.l = var41;
    let var139: f32 = var16.n;
    let mut var140: f64 = var42 * var139 as f64;
    let var50 = if var16.q != 0 { var16.q } else { var13 - 1 };
    let mut var51 = GapList::default();
    let mut var52 = GapList::default();
    let mut var53 = GapList::default();
    let mut var54 = GapList::default();
    let mut var57: i32 = 0;
    let var58 = var16.b_remaining;
    let mut var59: i32 = 0;
    let var66 = possible_length(var26, var8, var27, var10, var11);
    let mut var68 = var16.gaps(var66);
    let var69 = var16.mismatches(var66);
    let mut var70 = var69 - var41;
    let var73: i32 = 1;
    let mut var74 = false;
    let mut var75 = false;
    let var79 = var16.c_upper;
    let var82 = WILDCARD;
    let var83 = if var23 { var12 * 3 } else { var12 };
    let var84 = if var23 { 3 } else { 1 };
    let mut var18: f32 = 0.0;

    while var31 || var32 {
        if var34 {
            var31 = false;
        }

        if var31 {
            var75 = true;
            let mut var85 = var68;
            let mut var86 = var70;
            let mut var87 = var29.l;
            var31 = false;
            let mut var88: f64 = var29.a as f64;
            let mut var90 = var57 + var84;

            loop {
                let var91: i32 = 1;
                let mut var92 = var10 + (var83 - 1) + var90 + var51.c;
                let var26 = var7.len() as i32;
                let mut var157 = false;
                if var8 {
                    while var92 < 0 {
                        var92 += var26;
                    }
                    while var92 >= var26 {
                        var92 -= var26;
                    }
                } else {
                    if var92 > var26 || var92 < -1 {
                        break;
                    }
                    var157 = var92 >= var26 || var92 < 0;
                }
                let var163 = var11 + (var83 - 1) + var90 + var52.c;
                if var163 >= var27 {
                    break;
                }
                if var37 == -1 {
                    var37 = exact_run(var7, var92 - var91, var8, var24, var163 - 1, -var91, -1, var82)?;
                }
                let var169: u8 = if var92 < var7.len() as i32 && var92 >= 0 { at(var7, var92)? } else { 0 };
                let var172 = at(var24, var163)?;
                let var101 = var169 == var172 && var169 != var82;
                if var101 {
                    var37 += 1;
                } else {
                    var37 = 0;
                }
                if !var35 && var37 >= var20 {
                    var38 = true;
                }
                if !var101 && var38 {
                    break;
                }
                if var169 != var172 && !var74 && var35 {
                    var31 = true;
                    var32 = true;
                    break;
                }
                let var102 = if var8 {
                    INT_MAX
                } else if var91 > 0 {
                    var26 - var92
                } else {
                    var92 + 1
                };
                let var103 = var27 - var163;
                if var157 {
                    break;
                }
                let mut var179 = var90 + (var83 - var84) + var52.c + var51.c;
                let mut var182 = false;

                if !var182 && lower(var169) && lowercase_is_gap(var7, var8, var24, var15, var92, var163, var91, 1)? {
                    var51.add(var179, 1)?;
                    var182 = true;
                }
                if !var182 && lower(var172) && lowercase_is_gap(var24, false, var7, var15, var163, var92, var91, 1)? {
                    var52.add(var179, 1)?;
                    var182 = true;
                }

                if var182 {
                    var90 -= var84;
                    var29.a = var88 as f32;
                    var29.b = (var138 - var137 as f64) as f32;
                    var29.l = var87;
                    var140 = if var16.l { 0.0 } else { var140.min((var29.a * var139) as f64) };
                    var57 = var90;
                    var70 = var86;
                    var68 = var85;
                    if var35 {
                        var32 = true;
                    }
                } else {
                    if var86 > 0 && var85 > 0 && var169 != var172 && var102 > var58 && var103 > var58 {
                        let var189 = min3(20, var102, var103);
                        let var195 = min3(20, var102 - 1, var103 - 1);
                        let var202 = matched_count(var15, var7, var92 + var91, var8, var24, var163 + 1, var91, 1, var195, 1, false)?;
                        let var205 = matched_count(var15, var7, var92 + var91, var8, var24, var163 + 1, var91, 1, var195, 0, false)?;
                        if var205 < var195 {
                            let mut var210 = var50.max(var202);
                            let mut var213 = 1;
                            if var202 == var195 {
                                var213 -= 1;
                            }
                            let mut var216: f32 = -10000.0;
                            let mut var219: f32 = -10000.0;
                            let mut var222 = 0;
                            let mut var225 = 0;
                            let mut var228 = INT_MAX;
                            let mut var119: i8 = 0;
                            let mut var120: i8 = 0;
                            let mut var121 = -1;
                            let mut var122 = 0;
                            let mut var123: f32 = 0.0;
                            let mut var124 = -1;
                            let mut var125 = 0;
                            let mut var126: f32 = 0.0;

                            let mut var127 = 1;
                            while var127 <= var228 && var127 <= var22 {
                                var120 = var120.wrapping_add(2);
                                var119 = var119.wrapping_add(var91 as i8);
                                if var210 < 8 && var210 < var120 as i32 {
                                    var210 = (var120 as i32).min(8);
                                }
                                let mut var128 = min3(var102 - var127, var103, var210);
                                let mut var129 = min3(var103 - var127, var102, var210);
                                if var129 <= 0 && var128 <= 0 {
                                    break;
                                }
                                let var130 = var16.s(var127);
                                let mut var131: f32 = -10000.0;
                                let mut var132: f32 = -10000.0;
                                if var102 - var127 >= var58
                                    && var128 > 0
                                    && window_matches(
                                        var15,
                                        var7,
                                        var92 + var119 as i32,
                                        var8,
                                        var24,
                                        var163,
                                        var128,
                                        var73,
                                        var91,
                                        false,
                                        false,
                                        var30,
                                    )?
                                {
                                    if var189 > var128 {
                                        var128 = var128.max(matched_count(
                                            var15,
                                            var7,
                                            var92 + var119 as i32,
                                            var8,
                                            var24,
                                            var163,
                                            var91,
                                            1,
                                            var189,
                                            var213,
                                            false,
                                        )?);
                                    }
                                    if var128 != var121 {
                                        var122 = lookbehind_penalty(var15, var7, var92, var8, var24, var163, var128, var50, -var91, -1)?;
                                        var123 = best_score_sum(var15, var7, var92, var8, var24, var163, var91, 1, var128, var189)?;
                                        var121 = var128;
                                    }
                                    let var133 = var130 + score_sum(var15, var7, var92 + var119 as i32, var8, var24, var163, var91, 1, var128)?;
                                    if var133 > -10000.0 {
                                        let var134 = var133 + var122 as f32;
                                        if (var134 > 0.0 && var134 > var123) || var134 > var123 + 12.0 {
                                            var131 = var134;
                                        }
                                    }
                                }
                                if var103 - var127 >= var58
                                    && var129 > 0
                                    && window_matches(var15, var7, var92, var8, var24, var163 + var127, var129, var73, var91, false, false, var30)?
                                {
                                    if var189 > var129 {
                                        var129 = var129.max(matched_count(
                                            var15,
                                            var7,
                                            var92,
                                            var8,
                                            var24,
                                            var163 + var127,
                                            var91,
                                            1,
                                            var189,
                                            var213,
                                            false,
                                        )?);
                                    }
                                    if var129 != var124 {
                                        var125 = lookbehind_penalty(var15, var7, var92, var8, var24, var163, var129, var50, -var91, -1)?;
                                        var126 = best_score_sum(var15, var7, var92, var8, var24, var163, var91, 1, var129, var189)?;
                                        var124 = var129;
                                    }
                                    let var261 = var130 + score_sum(var15, var7, var92, var8, var24, var163 + var127, var91, 1, var129)?;
                                    if var261 > -10000.0 {
                                        let var264 = var261 + var125 as f32;
                                        if (var264 > 0.0 && var264 > var126) || var264 > var126 + 12.0 {
                                            var132 = var264;
                                        }
                                    }
                                }
                                if var131 > var216 && var131 >= var132 {
                                    var222 = var127;
                                    var216 = var131;
                                    var228 = if var225 > 0 { var127 } else { var127 + 5 };
                                } else if var132 > var219 && var132 >= var131 {
                                    var225 = var127;
                                    var219 = var132;
                                    var228 = if var222 > 0 { var127 } else { var127 + 5 };
                                }
                                var127 += 1;
                            }

                            let mut var239 = 0;
                            let mut var242 = 0;
                            // Which lists receive (245) and give back (248) the gap: 51/52 or 52/51.
                            let mut ref_side: Option<bool> = None;
                            if var222 > 0 && var216 > var219 {
                                let var254 = INT_MAX;
                                var242 = slide_gap(
                                    var36,
                                    var15,
                                    var7,
                                    var92,
                                    var91,
                                    var8,
                                    var24,
                                    var163,
                                    1,
                                    false,
                                    var222,
                                    min4(var195 - 1, var102 - var222, var103, var254),
                                    &mut var18,
                                    false,
                                )?;
                                var239 = var222;
                                var88 += var16.s(var239) as f64;
                                var182 = true;
                                ref_side = Some(true);
                                var88 += var18 as f64;
                            } else if var225 > 0 && var219 > var216 {
                                let var253 = INT_MAX;
                                var242 = slide_gap(
                                    var36,
                                    var15,
                                    var24,
                                    var163,
                                    1,
                                    false,
                                    var7,
                                    var92,
                                    var91,
                                    var8,
                                    var225,
                                    min4(var195 - 1, var102, var103 - var225, var253),
                                    &mut var18,
                                    false,
                                )?;
                                var239 = var225;
                                var88 += var16.s(var239) as f64;
                                var182 = true;
                                ref_side = Some(false);
                                var88 += var18 as f64;
                            }

                            if var182 {
                                let (var245, var248) = match ref_side {
                                    Some(true) => (&mut var51, &mut var52),
                                    _ => (&mut var52, &mut var51),
                                };
                                while !var23 && var239 > 0 && var248.c > 0 && var248.end()? >= var179 - var79 {
                                    let var255 = var248.last_len()?;
                                    let var259 = var239.min(var255);
                                    var248.shorten(var259)?;
                                    var87 += 2;
                                    var239 -= var259;
                                    var179 -= var259;
                                }
                                if var239 > 0 {
                                    let var256 = mismatch_count(var15, var7, var24, var92, var163, var242, var8, false)?;
                                    var245.add(var179 + var242, var239)?;
                                    var87 += var256;
                                    var86 -= var256;
                                }
                                if var88 > var138 {
                                    var138 = var88;
                                }
                                var90 -= var84;
                                var85 -= 1;
                                var86 -= 1;
                                var90 += var242;
                                if !var35 {
                                    var37 = -1;
                                }
                            }
                        }
                    }

                    let mut var190 = false;
                    if !var182 {
                        let var196 = var15.get(var169, var172)?;
                        var88 += var196 as f64;
                        if var88 > var138 {
                            var138 = var88;
                        }
                        if !positive(var196) {
                            var190 = true;
                        }
                    }
                    let var197 = var190 || var182;
                    if var190 {
                        var86 -= 1;
                        var87 += 1;
                    }
                    if var16.l || var182 || (var88 > var29.a as f64 && var87 <= var16.mismatches((var59 + var90 + var83) as i64)) {
                        var29.a = var88 as f32;
                        var29.b = (var138 - var137 as f64) as f32;
                        var29.l = var87;
                        var140 = if var16.l { 0.0 } else { var140.min((var29.a * var139) as f64) };
                        var57 = var90;
                        var70 = var86;
                        var68 = var85;
                        if var35 {
                            var32 = true;
                        }
                    }
                    if var35 && var197 && var86 > 0 && (var16.l || var182) {
                        var31 = true;
                        var32 = true;
                        break;
                    }
                    if var88 < var140 || var86 < 0 {
                        if var16.l {
                            var29.reset();
                            var29.c = var59 + var53.c;
                            var29.d = var59 + var54.c;
                            var29.g = var59 + var57 + var83 + var52.c + var51.c + var54.c + var53.c;
                            return Ok(var29);
                        }
                        break;
                    }
                }
                var90 += var84;
            }
        }

        if var33 {
            var32 = false;
        }

        if var32 {
            var74 = true;
            let mut var141 = var68;
            let mut var142 = var70;
            let mut var143 = var29.l;
            var32 = false;
            let mut var146: f64 = var29.a as f64;
            let mut var150 = var59 + var84;

            loop {
                let var151: i32 = 1;
                let mut var152 = var10 - var150 - var53.c;
                let var26 = var7.len() as i32;
                let mut var159 = false;
                if !var8 {
                    if var152 > var26 || var152 < -1 {
                        break;
                    }
                    var159 = var152 >= var26 || var152 < 0;
                } else {
                    while var152 < 0 {
                        var152 += var26;
                    }
                    while var152 >= var26 {
                        var152 -= var26;
                    }
                }
                let var165 = var11 - var150 - var54.c;
                if var165 < 0 {
                    break;
                }
                if var37 == -1 {
                    var37 = exact_run(var7, var152 + var151, var8, var24, var165 + 1, var151, 1, var82)?;
                }
                let var171: u8 = if var152 < var7.len() as i32 && var152 >= 0 { at(var7, var152)? } else { 0 };
                let var174 = at(var24, var165)?;
                let var175 = var171 == var174 && var171 != var82;
                if var175 {
                    var37 += 1;
                } else {
                    var37 = 0;
                }
                if !var35 && var37 >= var20 {
                    var38 = true;
                }
                if !var175 && var38 {
                    break;
                }
                if var171 != var174 && !var75 && var35 {
                    var31 = true;
                    var32 = true;
                    break;
                }
                let var176 = if var8 {
                    INT_MAX
                } else if var151 > 0 {
                    var152 + 1
                } else {
                    var26 - var152
                };
                let var177 = var165 + 1;
                if var159 {
                    break;
                }
                let mut var181 = var150 - var84 + var54.c + var53.c;
                let mut var184 = false;

                if !var184 && lower(var171) && lowercase_is_gap(var7, var8, var24, var15, var152, var165, -var151, -1)? {
                    var53.add(var181, 1)?;
                    var184 = true;
                }
                if !var184 && lower(var174) && lowercase_is_gap(var24, false, var7, var15, var165, var152, -var151, -1)? {
                    var54.add(var181, 1)?;
                    var184 = true;
                }

                if var184 {
                    var150 -= var84;
                    var29.a = var146 as f32;
                    var29.b = (var138 - var137 as f64) as f32;
                    var29.l = var143;
                    var140 = if var16.k { 0.0 } else { var140.min((var29.a * var139) as f64) };
                    var59 = var150;
                    var70 = var142;
                    var68 = var141;
                    if !var16.k && var35 {
                        var31 = true;
                    }
                } else {
                    if var142 > 0 && var141 > 0 && var171 != var174 && var176 > var58 && var177 > var58 {
                        let var193 = min3(20, var176, var177);
                        let var199 = min3(20, var176 - 1, var177 - 1);
                        let var204 = matched_count(var15, var7, var152 - var151, var8, var24, var165 - 1, -var151, -1, var199, 1, false)?;
                        let var208 = matched_count(var15, var7, var152 - var151, var8, var24, var165 - 1, -var151, -1, var199, 0, false)?;
                        if var208 < var199 {
                            let mut var212 = var50.max(var204);
                            let mut var215 = 1;
                            if var204 == var199 {
                                var215 -= 1;
                            }
                            let mut var218: f32 = -10000.0;
                            let mut var221: f32 = -10000.0;
                            let mut var224 = 0;
                            let mut var227 = 0;
                            let mut var230 = INT_MAX;
                            let mut var231: i8 = 0;
                            let mut var232: i8 = 0;
                            let mut var233 = -1;
                            let mut var234 = 0;
                            let mut var235: f32 = 0.0;
                            let mut var236 = -1;
                            let mut var237 = 0;
                            let mut var238: f32 = 0.0;

                            let mut var240 = 1;
                            while var240 <= var230 && var240 <= var22 {
                                var231 = var231.wrapping_add(var151 as i8);
                                var232 = var232.wrapping_add(2);
                                if var212 < 8 && var212 < var232 as i32 {
                                    var212 = (var232 as i32).min(8);
                                }
                                let mut var243 = min3(var176 - var240, var177, var212);
                                let mut var246 = min3(var177 - var240, var176, var212);
                                if var246 <= 0 && var243 <= 0 {
                                    break;
                                }
                                let var249 = var16.s(var240);
                                let mut var257: f32 = -10000.0;
                                let mut var260: f32 = -10000.0;
                                let w231 = var231 as i32;
                                if var176 - var240 >= var58
                                    && var243 > 0
                                    && window_matches(
                                        var15,
                                        var7,
                                        var152 - w231 - (if var151 > 0 { var243 - 1 } else { 1 - var243 }),
                                        var8,
                                        var24,
                                        var165 - var243 + 1,
                                        var243,
                                        var73,
                                        var151,
                                        true,
                                        false,
                                        var30,
                                    )?
                                {
                                    if var193 > var243 {
                                        var243 = var243.max(matched_count(
                                            var15,
                                            var7,
                                            var152 - w231,
                                            var8,
                                            var24,
                                            var165,
                                            -var151,
                                            -1,
                                            var193,
                                            var215,
                                            false,
                                        )?);
                                    }
                                    if var243 != var233 {
                                        var234 = lookbehind_penalty(var15, var7, var152, var8, var24, var165, var243, var50, var151, 1)?;
                                        var235 = best_score_sum(var15, var7, var152, var8, var24, var165, -var151, -1, var243, var193)?;
                                        var233 = var243;
                                    }
                                    let var262 = var249 + score_sum(var15, var7, var152 - w231, var8, var24, var165, -var151, -1, var243)?;
                                    if var262 > -10000.0 {
                                        let var265 = var262 + var234 as f32;
                                        if (var265 > 0.0 && var265 > var235) || var265 > var235 + 12.0 {
                                            var257 = var265;
                                        }
                                    }
                                }
                                if var177 - var240 >= var58
                                    && var246 > 0
                                    && window_matches(
                                        var15,
                                        var7,
                                        var152 - (if var151 > 0 { var246 - 1 } else { 1 - var246 }),
                                        var8,
                                        var24,
                                        var165 - var246 - var240 + 1,
                                        var246,
                                        var73,
                                        var151,
                                        true,
                                        false,
                                        var30,
                                    )?
                                {
                                    if var193 > var246 {
                                        var246 = var246.max(matched_count(
                                            var15,
                                            var7,
                                            var152,
                                            var8,
                                            var24,
                                            var165 - var240,
                                            -var151,
                                            -1,
                                            var193,
                                            var215,
                                            false,
                                        )?);
                                    }
                                    let var263 = var249 + score_sum(var15, var7, var152, var8, var24, var165 - var240, -var151, -1, var246)?;
                                    if var246 != var236 {
                                        var237 = lookbehind_penalty(var15, var7, var152, var8, var24, var165, var246, var50, var151, 1)?;
                                        var238 = best_score_sum(var15, var7, var152, var8, var24, var165, -var151, -1, var246, var193)?;
                                        var236 = var246;
                                    }
                                    if var263 > -10000.0 {
                                        let var266 = var263 + var237 as f32;
                                        if (var266 > 0.0 && var266 > var238) || var266 > var238 + 12.0 {
                                            var260 = var266;
                                        }
                                    }
                                }
                                if var257 > var218 && var257 >= var260 {
                                    var224 = var240;
                                    var218 = var257;
                                    var230 = if var227 > 0 { var240 } else { var240 + 5 };
                                } else if var260 > var221 && var260 >= var257 {
                                    var227 = var240;
                                    var221 = var260;
                                    var230 = if var224 > 0 { var240 } else { var240 + 5 };
                                }
                                var240 += 1;
                            }

                            let mut var241 = 0;
                            // Which lists receive (244) and give back (247) the gap: 53/54 or 54/53.
                            let mut ref_side: Option<bool> = None;
                            if var224 > 0 && var218 > var221 {
                                var241 = var224;
                                var146 += var16.s(var241) as f64;
                                var184 = true;
                                var181 = slide_gap_back(
                                    var36, var181, var7, var152, var151, var8, &var54, var24, var165, 1, false, &var53, var15, var241, var83,
                                    &mut var18,
                                )?;
                                var146 += var18 as f64;
                                ref_side = Some(true);
                            } else if var227 > 0 && var221 > var218 {
                                var241 = var227;
                                var146 += var16.s(var241) as f64;
                                var184 = true;
                                var181 = slide_gap_back(
                                    var36, var181, var24, var165, 1, false, &var53, var7, var152, var151, var8, &var54, var15, var241, var83,
                                    &mut var18,
                                )?;
                                var146 += var18 as f64;
                                ref_side = Some(false);
                            }

                            if var184 {
                                let (var244, var247) = match ref_side {
                                    Some(true) => (&mut var53, &mut var54),
                                    _ => (&mut var54, &mut var53),
                                };
                                while !var23 && var241 > 0 && var247.c > 0 && var247.end()? >= var181 - var79 {
                                    let var251 = var247.last_len()?;
                                    let var258 = var241.min(var251);
                                    var247.shorten(var258)?;
                                    var143 += 2;
                                    var241 -= var258;
                                    var181 -= var258;
                                }
                                if var241 > 0 {
                                    var244.add(var181, var241)?;
                                }
                                if var146 > var138 {
                                    var138 = var146;
                                }
                                var150 -= var84;
                                var141 -= 1;
                                var142 -= 1;
                                if !var35 {
                                    var37 = -1;
                                }
                            }
                        }
                    }

                    let mut var194 = false;
                    if !var184 {
                        let var200 = var15.get(var171, var174)?;
                        var146 += var200 as f64;
                        if var146 > var138 {
                            var138 = var146;
                        }
                        if !positive(var200) {
                            var194 = true;
                        }
                    }
                    let var201 = var194 || var184;
                    if var194 {
                        var142 -= 1;
                        var143 += 1;
                    }
                    if var16.k || var184 || (var146 > var29.a as f64 && var143 <= var16.mismatches((var150 + var57 + var83) as i64)) {
                        var29.a = var146 as f32;
                        var29.b = (var138 - var137 as f64) as f32;
                        var29.l = var143;
                        var140 = if var16.k { 0.0 } else { var140.min((var29.a * var139) as f64) };
                        var59 = var150;
                        var70 = var142;
                        var68 = var141;
                        if !var16.k && var35 {
                            var31 = true;
                        }
                    }
                    if var35 && var201 && var142 > 0 && (var16.k || var184) {
                        var31 = true;
                        var32 = true;
                        break;
                    }
                    if var146 < var140 || var142 < 0 {
                        if var16.k {
                            var29.reset();
                            var29.c = var59 + var53.c;
                            var29.d = var59 + var54.c;
                            var29.g = var59 + var57 + var83 + var52.c + var51.c + var54.c + var53.c;
                            var29.e = first_gap(&var54, &var53, var59);
                            var29.f = first_gap(&var52, &var51, var57 + var83);
                            return Ok(var29);
                        }
                        break;
                    }
                }
                var150 += var84;
            }
        }
    }

    let var1 = if var23 { var10 * 3 } else { var10 };
    var29.c = var59 + var53.c;
    var29.d = var59 + var54.c;
    var29.o = var1 - var29.c;
    var29.g = var59 + var57 + var83 + var52.c + var51.c + var54.c + var53.c;
    let offset = var59 + var54.c + var53.c;
    var29.h = gap_array(&var54, &var52, offset, false);
    var29.i = gap_array(&var53, &var51, offset, false);
    var29.j = gap_array(&var54, &var52, offset, true);
    var29.k = gap_array(&var53, &var51, offset, true);
    var29.e = first_gap(&var54, &var53, var59);
    var29.f = first_gap(&var52, &var51, var57 + var83);
    var29.n = 0;
    Ok(var29)
}
