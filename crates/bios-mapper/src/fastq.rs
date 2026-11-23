//! `mapping_io.py`: strict FASTQ records and read pairs.

use std::io::Read;

use flate2::read::MultiGzDecoder;

use crate::MapError;
use crate::py::is_py_space;

/// One FASTQ record, as text: the name without its `@`, the bases and the qualities.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FastqRead {
    pub name: String,
    pub sequence: String,
    pub quality: String,
}

/// The largest FASTQ text accepted after decompression, so a hostile file cannot exhaust memory.
pub const MAX_FASTQ_BYTES: u64 = 8 << 30;

/// The text of a FASTQ file, decompressing gzip (including multi-member files) when the bytes
/// start with the gzip magic number. Text is UTF-8, with `\r\n` and `\r` read as line ends, as
/// Python's text mode reads it.
pub fn decode(bytes: &[u8]) -> Result<String, MapError> {
    let raw: Vec<u8> = if bytes.starts_with(&[0x1f, 0x8b]) {
        let mut out = Vec::new();
        MultiGzDecoder::new(bytes)
            .take(MAX_FASTQ_BYTES)
            .read_to_end(&mut out)
            .map_err(|e| MapError::new(format!("Could not decompress FASTQ: {e}")))?;
        out
    } else {
        bytes.to_vec()
    };
    let text = String::from_utf8(raw).map_err(|_| MapError::new("FASTQ is not UTF-8 text"))?;
    Ok(text.replace("\r\n", "\n").replace('\r', "\n"))
}

/// The records of one FASTQ text (`read_fastq`).
pub fn parse(text: &str, label: &str) -> Result<Vec<FastqRead>, MapError> {
    let mut lines = text.split_inclusive('\n');
    let mut out = Vec::new();
    let mut index = 0;
    let strip = |s: &str| s.trim_end_matches(['\r', '\n']).to_string();
    while let Some(header) = lines.next() {
        let sequence = strip(lines.next().unwrap_or(""));
        let plus = lines.next().unwrap_or("");
        let quality = strip(lines.next().unwrap_or(""));
        index += 1;
        if !header.starts_with('@') || !plus.starts_with('+') || sequence.is_empty() || sequence.chars().count() != quality.chars().count() {
            return Err(MapError::new(format!("Malformed FASTQ record {index} in {label}")));
        }
        out.push(FastqRead { name: strip(&header[1..]), sequence, quality });
    }
    Ok(out)
}

fn pair_key<'a>(name: &'a str, suffix: &str) -> Result<&'a str, MapError> {
    let token = name.split(is_py_space).find(|t| !t.is_empty()).ok_or_else(|| MapError::new("FASTQ record has an empty name"))?;
    Ok(token.strip_suffix(suffix).unwrap_or(token))
}

/// Pairs R1 with R2, or consecutive records of one interleaved file (`read_pairs`).
pub fn pairs(first: Vec<FastqRead>, second: Option<Vec<FastqRead>>) -> Result<Vec<(FastqRead, FastqRead)>, MapError> {
    let joined: Vec<(Option<FastqRead>, Option<FastqRead>)> = match second {
        Some(second) => {
            let n = first.len().max(second.len());
            let mut a = first.into_iter();
            let mut b = second.into_iter();
            (0..n).map(|_| (a.next(), b.next())).collect()
        }
        None => {
            let mut it = first.into_iter();
            let mut v = Vec::new();
            while let Some(r) = it.next() {
                v.push((Some(r), it.next()));
            }
            v
        }
    };
    let mut out = Vec::with_capacity(joined.len());
    for (index, (read, mate)) in joined.into_iter().enumerate() {
        let (Some(read), Some(mate)) = (read, mate) else {
            return Err(MapError::new("Paired FASTQ inputs contain different record counts"));
        };
        if pair_key(&read.name, "/1")? != pair_key(&mate.name, "/2")? {
            return Err(MapError::new(format!("FASTQ pair {index} names disagree: {:?}, {:?}", read.name, mate.name)));
        }
        out.push((read, mate));
    }
    Ok(out)
}
