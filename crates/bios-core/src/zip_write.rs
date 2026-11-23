//! Writing a zip archive in memory: the alignment's files in one download, the way the web's
//! "All alignment files" hands them over. Deflated entries, no directories, no zip64 (an archive
//! past 4 GB is refused rather than written wrong).

use std::io::Write;

use flate2::Compression;
use flate2::write::DeflateEncoder;

/// The bytes of a zip holding `files` as `(name, contents)`, in that order.
pub fn zip(files: &[(&str, &[u8])]) -> Result<Vec<u8>, String> {
    let mut out: Vec<u8> = Vec::new();
    let mut central: Vec<u8> = Vec::new();
    let small = |n: usize, what: &str| u32::try_from(n).map_err(|_| format!("{what} is too large for a zip"));
    for (name, data) in files {
        let mut enc = DeflateEncoder::new(Vec::new(), Compression::default());
        enc.write_all(data).map_err(|e| e.to_string())?;
        let packed = enc.finish().map_err(|e| e.to_string())?;
        let crc = crc32fast::hash(data);
        let offset = small(out.len(), "the archive")?;
        let (size, packed_size) = (small(data.len(), name)?, small(packed.len(), name)?);
        let name_len = u16::try_from(name.len()).map_err(|_| format!("{name} is too long a name"))?;
        // Local file header.
        out.extend(0x0403_4b50u32.to_le_bytes());
        out.extend(20u16.to_le_bytes()); // version needed
        out.extend(0x0800u16.to_le_bytes()); // UTF-8 names
        out.extend(8u16.to_le_bytes()); // deflate
        out.extend(0u16.to_le_bytes()); // time
        out.extend(0x0021u16.to_le_bytes()); // date: 1980-01-01
        out.extend(crc.to_le_bytes());
        out.extend(packed_size.to_le_bytes());
        out.extend(size.to_le_bytes());
        out.extend(name_len.to_le_bytes());
        out.extend(0u16.to_le_bytes());
        out.extend(name.as_bytes());
        out.extend(&packed);
        // Central directory entry.
        central.extend(0x0201_4b50u32.to_le_bytes());
        central.extend(20u16.to_le_bytes()); // made by
        central.extend(20u16.to_le_bytes());
        central.extend(0x0800u16.to_le_bytes());
        central.extend(8u16.to_le_bytes());
        central.extend(0u16.to_le_bytes());
        central.extend(0x0021u16.to_le_bytes());
        central.extend(crc.to_le_bytes());
        central.extend(packed_size.to_le_bytes());
        central.extend(size.to_le_bytes());
        central.extend(name_len.to_le_bytes());
        central.extend([0u8; 12]); // extra, comment, disk, internal attrs, external attrs
        central.extend(offset.to_le_bytes());
        central.extend(name.as_bytes());
    }
    let central_at = small(out.len(), "the archive")?;
    let central_len = small(central.len(), "the archive")?;
    let count = u16::try_from(files.len()).map_err(|_| "too many files for a zip".to_string())?;
    out.extend(&central);
    out.extend(0x0605_4b50u32.to_le_bytes());
    out.extend([0u8; 4]); // disk numbers
    out.extend(count.to_le_bytes());
    out.extend(count.to_le_bytes());
    out.extend(central_len.to_le_bytes());
    out.extend(central_at.to_le_bytes());
    out.extend(0u16.to_le_bytes());
    Ok(out)
}

#[cfg(test)]
mod tests {
    use std::io::Read;

    use flate2::read::DeflateDecoder;

    use super::*;

    fn u32_at(b: &[u8], at: usize) -> u32 {
        u32::from_le_bytes([b[at], b[at + 1], b[at + 2], b[at + 3]])
    }

    fn u16_at(b: &[u8], at: usize) -> usize {
        u16::from_le_bytes([b[at], b[at + 1]]) as usize
    }

    /// Reads the archive back through its central directory, as an unzip tool does.
    fn unzip(bytes: &[u8]) -> Vec<(String, Vec<u8>)> {
        let end = bytes.len() - 22;
        assert_eq!(u32_at(bytes, end), 0x0605_4b50);
        let count = u16_at(bytes, end + 10);
        let mut at = u32_at(bytes, end + 16) as usize;
        let mut out = Vec::new();
        for _ in 0..count {
            assert_eq!(u32_at(bytes, at), 0x0201_4b50);
            let (crc, packed, size) = (u32_at(bytes, at + 16), u32_at(bytes, at + 20) as usize, u32_at(bytes, at + 24) as usize);
            let name_len = u16_at(bytes, at + 28);
            let local = u32_at(bytes, at + 42) as usize;
            let name = String::from_utf8(bytes[at + 46..at + 46 + name_len].to_vec()).unwrap();
            let data_at = local + 30 + u16_at(bytes, local + 26) + u16_at(bytes, local + 28);
            let mut data = Vec::new();
            DeflateDecoder::new(&bytes[data_at..data_at + packed]).read_to_end(&mut data).unwrap();
            assert_eq!(data.len(), size);
            assert_eq!(crc32fast::hash(&data), crc);
            out.push((name, data));
            at += 46 + name_len;
        }
        out
    }

    #[test]
    fn an_archive_reads_back_every_file_exactly() {
        let files: &[(&str, &[u8])] = &[("consensus.fasta", b">p\nACGT\n"), ("alignment.sam", b"@HD\tVN:1.6\n"), ("empty.tsv", b"")];
        let back = unzip(&zip(files).unwrap());
        assert_eq!(back.len(), 3);
        for ((name, data), (n, d)) in files.iter().zip(&back) {
            assert_eq!(*name, n);
            assert_eq!(*data, d.as_slice());
        }
    }
}
