//! `mapper_primitives.py`: the per-pair random number generator, the nucleotide score matrix, the
//! gap cost, single-precision rounding and the pair-distance penalty.

use crate::MapError;

/// Rounds a double to the nearest single-precision value, as the original's `float32` does.
pub fn float32(value: f64) -> f64 {
    value as f32 as f64
}

pub const FLOAT_MAX: f64 = f32::MAX as f64;
pub const INT_MAX: i64 = 2147483647;

fn signed32(value: i64) -> i64 {
    value as i32 as i64
}

/// The Java-int hash that seeds each pair's tie-breaking generator from its index.
pub fn read_pair_seed(index: i64) -> i64 {
    let mut n = signed32(index);
    n = signed32(n.wrapping_add(2127912214).wrapping_add(n << 12));
    n = signed32(n ^ 0xc761c23c ^ (n >> 19));
    n = signed32(n.wrapping_add(374761393).wrapping_add(n << 5));
    n = signed32((n - 744332180) ^ (n << 9));
    n = signed32(n.wrapping_sub(42973499).wrapping_add(n << 3));
    signed32(n ^ 0xb55a4f09 ^ (n >> 16))
}

/// Java's 48-bit linear congruential `Random`, including `nextInt`'s rejection rule.
#[derive(Clone, Debug)]
pub struct JavaRandom {
    state: u64,
}

impl JavaRandom {
    const MULTIPLIER: u64 = 0x5deece66d;
    const MASK: u64 = (1 << 48) - 1;

    pub fn new(seed: i64) -> Self {
        JavaRandom { state: ((seed as u64) ^ Self::MULTIPLIER) & Self::MASK }
    }

    fn next_bits(&mut self, bits: u32) -> i64 {
        self.state = self.state.wrapping_mul(Self::MULTIPLIER).wrapping_add(11) & Self::MASK;
        (self.state >> (48 - bits)) as i64
    }

    pub fn next_int(&mut self, bound: i64) -> Result<i64, MapError> {
        if !(0 < bound && bound <= 0x7fffffff) {
            return Err(MapError::new("Java nextInt bound must be a positive signed int"));
        }
        if bound & (bound - 1) == 0 {
            return Ok((bound * self.next_bits(31)) >> 31);
        }
        loop {
            let bits = self.next_bits(31);
            let value = bits % bound;
            if signed32(bits - value + bound - 1) >= 0 {
                return Ok(value);
            }
        }
    }
}

/// The bases each IUPAC symbol stands for, as the original's `IUPAC` table.
pub fn iupac(symbol: u8) -> Option<&'static [u8]> {
    Some(match symbol {
        b'A' => b"A",
        b'C' => b"C",
        b'G' => b"G",
        b'T' => b"T",
        b'R' => b"AG",
        b'Y' => b"CT",
        b'S' => b"CG",
        b'W' => b"AT",
        b'K' => b"GT",
        b'M' => b"AC",
        b'B' => b"CGT",
        b'D' => b"AGT",
        b'H' => b"ACT",
        b'V' => b"ACG",
        b'N' | b'?' => b"ACGT",
        _ => return None,
    })
}

/// The adjusted nucleotide score for two supported symbols.
pub fn nucleotide_score(left: u8, right: u8) -> Result<f64, MapError> {
    let (Some(a), Some(b)) = (iupac(left), iupac(right)) else {
        return Err(MapError::new(format!("No nucleotide score for {:?} and {:?}", left as char, right as char)));
    };
    let mut total = 0i64;
    for x in a {
        for y in b {
            total += if x == y { 5 } else { -4 };
        }
    }
    let mut value = total as f64 / (a.len() * b.len()) as f64;
    if left == right && left != b'N' && left != b'?' {
        value = value.max(1.0);
    }
    Ok(value)
}

/// The concordance penalty for two normalized mates (`pair_distance_penalty`, MapAll mode).
pub struct Location {
    pub reference: i64,
    pub start: i64,
    pub reference_span: i64,
    pub reverse: bool,
}

pub fn pair_distance_penalty(first: &Location, second: &Location, expected: i64, circular_length: Option<i64>) -> Result<f64, MapError> {
    if first.reference != second.reference {
        return Ok(60.0);
    }
    if expected < 0 {
        return Err(MapError::new("Expected normalized distance must be nonnegative"));
    }
    let expected = expected.max(1);
    let mut span = if first.reverse { first.start + first.reference_span - second.start } else { second.start + second.reference_span - first.start };
    let mut deviation = span - expected;
    if let Some(length) = circular_length {
        if deviation > length / 2 {
            deviation -= length;
            span -= length;
        }
        if deviation < -(length / 2) {
            deviation += length;
            span += length;
        }
    }
    let deviation = deviation.abs();
    let mut penalty = float32(float32(deviation as f64) / float32(expected as f64));
    if span < 0 {
        penalty = float32(penalty + 1.0);
    }
    if first.reverse != second.reverse {
        penalty = float32(penalty + 2.0);
    }
    if penalty > 5.0 {
        penalty = float32(5.0 + float32(float32(float32(deviation as f64) / 1000.0) / 1000.0));
        penalty = penalty.min(6.0);
    }
    Ok(float32(penalty * 10.0).min(60.0))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn java_random_matches_java() {
        // new java.util.Random(42).nextInt(10) yields 0, then 3; nextInt(16) uses the power-of-two path.
        let mut r = JavaRandom::new(42);
        assert_eq!(r.next_int(10).unwrap(), 0);
        assert_eq!(r.next_int(10).unwrap(), 3);
        let mut r = JavaRandom::new(0);
        assert_eq!(r.next_int(16).unwrap(), 11);
    }

    #[test]
    fn scores_are_the_adjusted_matrix() {
        assert_eq!(nucleotide_score(b'A', b'A').unwrap(), 5.0);
        assert_eq!(nucleotide_score(b'R', b'A').unwrap(), 0.5);
        assert_eq!(nucleotide_score(b'N', b'N').unwrap(), -1.75);
        assert_eq!(nucleotide_score(b'B', b'B').unwrap(), 1.0);
    }
}
