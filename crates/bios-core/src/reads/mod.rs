//! Sequencing reads: FASTQ, gzip as sequencers write it, and which files are the two halves of one
//! sample. Ported from the web app's `reads/fastq.ts` and `reads/pairs.ts`.

pub mod fastq;
pub mod pairs;
