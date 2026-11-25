//! Sequence model, file formats and sequence maths for Bios. Nothing here depends on the UI.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![forbid(unsafe_code)]

pub mod acts;
pub mod align;
pub mod annealing;
pub mod binders;
pub mod build;
pub mod coding;
pub mod coding_edit;
pub mod compare;
pub mod digest;
pub mod duplex;
pub mod edits;
pub mod embl;
pub mod enzyme_list;
pub mod enzymes;
pub mod enzymes_data;
pub mod find_align;
pub mod formats;
pub mod genbank;
pub mod genbank_write;
pub mod genetic_codes_data;
pub mod gibson;
pub mod golden_gate;
pub mod interchange;
pub mod locations;
pub mod model;
pub mod parts;
pub mod pcr;
pub mod primer;
pub mod primer_binding;
pub mod primer_design;
pub mod primer_quality;
pub mod quantities;
pub mod reads;
pub mod search;
pub mod seq;
pub mod sequencing;
pub mod snapgene;
pub mod verify;
pub mod zip_write;

pub use model::{Construct, Feature, Fuzzy, LocationOperator, Segment, Strand};
