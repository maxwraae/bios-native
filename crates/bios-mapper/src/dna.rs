//! `dna_sequence.py` and the input normalization of `candidate_search._normalize`.

use crate::MapError;

fn complement_char(c: char) -> char {
    const FROM: &str = "ACGTRYSWKMBDHVNUacgtryswkmbdhvnu";
    const TO: &str = "TGCAYRSWMKVHDBNAtgcayrswmkvhdbna";
    match FROM.chars().position(|x| x == c) {
        Some(i) => TO.chars().nth(i).unwrap_or(c),
        None => c,
    }
}

fn complement_byte(b: u8) -> u8 {
    match b {
        b'A' => b'T',
        b'C' => b'G',
        b'G' => b'C',
        b'T' => b'A',
        b'R' => b'Y',
        b'Y' => b'R',
        b'S' => b'S',
        b'W' => b'W',
        b'K' => b'M',
        b'M' => b'K',
        b'B' => b'V',
        b'D' => b'H',
        b'H' => b'D',
        b'V' => b'B',
        b'N' => b'N',
        b'U' => b'A',
        b'a' => b't',
        b'c' => b'g',
        b'g' => b'c',
        b't' => b'a',
        b'r' => b'y',
        b'y' => b'r',
        b's' => b's',
        b'w' => b'w',
        b'k' => b'm',
        b'm' => b'k',
        b'b' => b'v',
        b'd' => b'h',
        b'h' => b'd',
        b'v' => b'b',
        b'n' => b'n',
        b'u' => b'a',
        other => other,
    }
}

/// Reverse-complements IUPAC DNA, keeping case and any other character.
pub fn reverse_complement(sequence: &[u8]) -> Vec<u8> {
    sequence.iter().rev().map(|&b| complement_byte(b)).collect()
}

/// [`reverse_complement`] over text that may hold any character.
pub fn reverse_complement_text(sequence: &str) -> String {
    sequence.chars().rev().map(complement_char).collect()
}

/// Uppercases, turns U into T, removes `-` and turns every unsupported symbol into N.
pub fn normalize(sequence: &str) -> Result<Vec<u8>, MapError> {
    let upper = sequence.to_uppercase().replace('U', "T").replace('-', "");
    if upper.contains('I') {
        return Err(MapError::new("Inosine word encoding has not been validated"));
    }
    Ok(upper.chars().map(|c| if "ACGTRYMWSKBDHVN".contains(c) { c as u8 } else { b'N' }).collect())
}

/// [`normalize`] for a sequence already made of bytes.
pub fn normalize_bytes(sequence: &[u8]) -> Result<Vec<u8>, MapError> {
    normalize(&String::from_utf8_lossy(sequence))
}
