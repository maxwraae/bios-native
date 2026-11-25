//! Pure geometry for the Bios views: where every base, lane and rule sits. No drawing, no UI types.
//!
//! Numbers are `f64` and base positions `i64` so the arithmetic matches the Bios web app exactly.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![forbid(unsafe_code)]

pub mod cuts;
pub mod fold_target;
pub mod interactions;
pub mod overview;
pub mod pane;
pub mod pileup;
pub mod projects;
pub mod row_features;
pub mod rows;
pub mod strip;
pub mod translations;
