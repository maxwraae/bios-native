//! Reading a FASTQ without ever holding one.
//!
//! A run's reads are the largest thing Bios opens: hundreds of megabytes compressed, several times
//! that expanded. So the reader takes any [`Read`] and yields records as they arrive.
//!
//! The trap: a record is four lines, but a line is not a delimiter. The quality line may itself
//! begin with '@' (a legal Phred score, 31), so a parser that resynchronises on '@' at the start of
//! a line shifts by one record and mis-pairs every sequence and quality after it. Counting strictly
//! modulo 4 from the top of the file is the only safe rule, so that is what this does.
//!
//! Gzip: a real Illumina `.fastq.gz` is not one gzip stream but many members concatenated (302 of
//! them in a 4 MB well off a MiSeq), because that lets the writer flush as it goes. That is legal
//! gzip, and [`flate2::read::MultiGzDecoder`] reads every member until the bytes run out.

use std::io::{BufRead, BufReader, Read};

use flate2::read::MultiGzDecoder;

/// One read.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FastqRecord {
    /// The header line without its '@'.
    pub id: String,
    pub seq: String,
    pub qual: String,
}

/// True for the names Bios is willing to read as reads.
pub fn is_reads_file(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    [".fastq", ".fq", ".fastq.gz", ".fq.gz"].iter().any(|ext| lower.ends_with(ext))
}

/// True when the name says gzip (`.gz` or `.z`).
pub fn is_gzip_name(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    lower.ends_with(".gz") || lower.ends_with(".z")
}

/// Decompresses if the name says gzip, reading every member.
pub fn maybe_gunzip<'a, R: Read + 'a>(source: R, name: &str) -> Box<dyn Read + 'a> {
    if is_gzip_name(name) { Box::new(MultiGzDecoder::new(source)) } else { Box::new(source) }
}

/// Bytes read and records yielded so far, for a progress bar.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Progress {
    /// Bytes pulled off the (already decompressed) stream.
    pub bytes: u64,
    pub records: u64,
}

/// The longest line a FASTQ may have here: long reads run to hundreds of kilobases, never this.
pub const MAX_LINE: usize = 64 << 20;

/// Yields every record in a FASTQ stream, counting lines modulo 4.
///
/// A line is capped at [`MAX_LINE`] bytes; anything longer is not a read and ends the stream with
/// an error rather than an unbounded allocation.
pub struct FastqReader<R> {
    source: BufReader<R>,
    line: Vec<u8>,
    line_no: u64,
    progress: Progress,
    done: bool,
}

impl<R: Read> FastqReader<R> {
    pub fn new(source: R) -> Self {
        FastqReader { source: BufReader::with_capacity(1 << 16, source), line: Vec::new(), line_no: 0, progress: Progress::default(), done: false }
    }

    pub fn progress(&self) -> Progress {
        self.progress
    }

    /// The next line without its line ending, or `None` at the end.
    fn next_line(&mut self) -> Option<Result<String, String>> {
        self.line.clear();
        let mut limited = (&mut self.source).take(MAX_LINE as u64 + 1);
        match limited.read_until(b'\n', &mut self.line) {
            Ok(0) => None,
            Ok(n) => {
                if self.line.len() > MAX_LINE {
                    return Some(Err(format!("line {} is longer than any read", self.line_no + 1)));
                }
                self.progress.bytes += n as u64;
                while matches!(self.line.last(), Some(b'\n' | b'\r')) {
                    self.line.pop();
                }
                // Latin-1, as the web reads it: every byte is one character, nothing is refused.
                Some(Ok(self.line.iter().map(|&b| b as char).collect()))
            }
            Err(e) => Some(Err(e.to_string())),
        }
    }
}

impl<R: Read> Iterator for FastqReader<R> {
    type Item = Result<FastqRecord, String>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.done {
            return None;
        }
        let (mut id, mut seq) = (String::new(), String::new());
        loop {
            let line = match self.next_line() {
                None => {
                    self.done = true;
                    return None;
                }
                Some(Err(e)) => {
                    self.done = true;
                    return Some(Err(e));
                }
                Some(Ok(line)) => line,
            };
            let at = self.line_no % 4;
            self.line_no += 1;
            match at {
                0 => id = line.chars().skip(1).collect(),
                1 => seq = line,
                3 => {
                    self.progress.records += 1;
                    return Some(Ok(FastqRecord { id, seq, qual: line }));
                }
                _ => {}
            }
        }
    }
}

/// Every record of a FASTQ held in memory, decompressing by name. For tests and small files.
pub fn parse_bytes(bytes: &[u8], name: &str) -> Result<Vec<FastqRecord>, String> {
    FastqReader::new(maybe_gunzip(bytes, name)).collect()
}

#[cfg(test)]
mod tests {
    use std::io::Write;

    use flate2::Compression;
    use flate2::write::GzEncoder;

    use super::*;

    const THREE: &str = "@read1 first\nACGTACGTAC\n+\nIIIIIIIIII\n@read2\nTTTTGGGGCC\n+\n@@@@@@@@@@\n@read3\nGGGGCCCCAA\n+\n!!!!!!!!!!\n";

    /// A reader that hands out at most `size` bytes per call, to put chunk boundaries anywhere.
    struct Chunked<'a> {
        bytes: &'a [u8],
        size: usize,
    }
    impl Read for Chunked<'_> {
        fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
            let n = self.size.min(buf.len()).min(self.bytes.len());
            buf[..n].copy_from_slice(&self.bytes[..n]);
            self.bytes = &self.bytes[n..];
            Ok(n)
        }
    }

    fn collect(text: &str, size: usize) -> Vec<FastqRecord> {
        FastqReader::new(Chunked { bytes: text.as_bytes(), size }).collect::<Result<_, _>>().unwrap()
    }

    fn rec(id: &str, seq: &str, qual: &str) -> FastqRecord {
        FastqRecord { id: id.into(), seq: seq.into(), qual: qual.into() }
    }

    #[test]
    fn reads_every_record_with_sequence_and_quality_correctly_paired() {
        let got = collect(THREE, 1 << 16);
        assert_eq!(got.len(), 3);
        assert_eq!(got[0], rec("read1 first", "ACGTACGTAC", "IIIIIIIIII"));
        assert_eq!(got[1], rec("read2", "TTTTGGGGCC", "@@@@@@@@@@"));
        assert_eq!(got[2], rec("read3", "GGGGCCCCAA", "!!!!!!!!!!"));
    }

    #[test]
    fn a_quality_line_of_at_signs_does_not_resynchronise_the_parser() {
        let got = collect(THREE, 1 << 16);
        assert_eq!(got[2].seq, "GGGGCCCCAA");
        assert_eq!(got[2].qual, "!!!!!!!!!!");
    }

    #[test]
    fn records_survive_chunk_boundaries_falling_anywhere() {
        let expected = collect(THREE, 1 << 16);
        for size in 1..=40 {
            assert_eq!(collect(THREE, size), expected, "chunk size {size} parsed differently");
        }
    }

    #[test]
    fn a_final_record_with_no_trailing_newline_is_still_returned() {
        assert_eq!(collect("@a\nACGT\n+\nIIII", 64), vec![rec("a", "ACGT", "IIII")]);
    }

    #[test]
    fn crlf_line_endings_are_handled() {
        assert_eq!(collect("@a\r\nACGT\r\n+\r\nIIII\r\n", 64)[0], rec("a", "ACGT", "IIII"));
    }

    #[test]
    fn progress_reports_bytes_and_a_running_record_count() {
        let mut reader = FastqReader::new(Chunked { bytes: THREE.as_bytes(), size: 16 });
        let mut seen = Vec::new();
        while let Some(r) = reader.next() {
            r.unwrap();
            seen.push(reader.progress());
        }
        assert!(seen.len() > 1);
        let last = reader.progress();
        assert_eq!(last.bytes, THREE.len() as u64);
        assert_eq!(last.records, 3);
        assert!(seen.windows(2).all(|w| w[1].bytes >= w[0].bytes && w[1].records >= w[0].records));
    }

    #[test]
    fn an_empty_stream_yields_nothing_rather_than_failing() {
        assert!(collect("", 64).is_empty());
    }

    /// Concatenated gzip members: what every sequencer writes.
    pub(crate) fn multi_member(text: &str, parts: usize) -> Vec<u8> {
        let bytes = text.as_bytes();
        let size = bytes.len().div_ceil(parts);
        let mut out = Vec::new();
        for chunk in bytes.chunks(size) {
            let mut gz = GzEncoder::new(Vec::new(), Compression::default());
            gz.write_all(chunk).unwrap();
            out.extend(gz.finish().unwrap());
        }
        out
    }

    fn read_all(bytes: &[u8], name: &str, size: usize) -> Vec<FastqRecord> {
        FastqReader::new(maybe_gunzip(Chunked { bytes, size }, name)).collect::<Result<_, _>>().unwrap()
    }

    #[test]
    fn reads_a_multi_member_gzip_the_shape_every_real_illumina_file_has() {
        let got = read_all(&multi_member(THREE, 3), "reads.fastq.gz", 1 << 16);
        assert_eq!(got.len(), 3);
        assert_eq!(got[2].seq, "GGGGCCCCAA");
        assert_eq!(got[2].qual, "!!!!!!!!!!");
    }

    #[test]
    fn a_member_boundary_falling_mid_record_loses_nothing() {
        for parts in [2, 3, 5, 9, 17] {
            let got = read_all(&multi_member(THREE, parts), "x.fastq.gz", 13);
            assert_eq!(got.len(), 3, "{parts} members");
            assert_eq!(got[1], rec("read2", "TTTTGGGGCC", "@@@@@@@@@@"));
        }
    }

    #[test]
    fn a_single_member_gzip_still_reads_and_matches_the_plain_file_exactly() {
        let plain = read_all(THREE.as_bytes(), "reads.fastq", 1 << 16);
        assert_eq!(read_all(&multi_member(THREE, 1), "reads.fastq.gz", 1 << 16), plain);
    }

    #[test]
    fn an_uncompressed_file_is_not_run_through_the_decompressor() {
        assert_eq!(read_all(THREE.as_bytes(), "reads.fastq", 1 << 16).len(), 3);
    }

    #[test]
    fn a_truncated_gzip_is_an_error_or_a_short_read_never_a_crash() {
        let whole = multi_member(THREE, 1);
        let cut = &whole[..whole.len() / 2];
        let got: Result<Vec<_>, _> = FastqReader::new(maybe_gunzip(cut, "x.fastq.gz")).collect();
        assert!(got.map_or(true, |v| v.len() < 3));
    }

    #[test]
    fn accepts_the_four_names_a_run_actually_produces_and_no_construct_file() {
        for yes in ["x.fastq", "x.fq", "x.fastq.gz", "X_R1_001.FASTQ.GZ", "a.fq.gz"] {
            assert!(is_reads_file(yes), "{yes}");
        }
        for no in ["p.gb", "p.dna", "p.fasta", "p.fa", "notes.txt", "x.fastq.zip"] {
            assert!(!is_reads_file(no), "{no}");
        }
    }

    #[test]
    fn a_fasta_is_not_reads() {
        assert!(!is_reads_file("genome.fasta"));
        assert!(!is_reads_file("genome.fa"));
    }
}
