//! A construct as it crosses to the server: its GenBank text, and beside it the same construct
//! as plain values, which is what the server lists, searches and hands to an agent.
//!
//! The shape is the one the Bios web app sends when it saves. Coordinates are 0-based and
//! inclusive at both ends, a strand is `1` or `-1`, and qualifiers are `notes`.

use std::collections::BTreeMap;

use bios_core::genbank_write::{self, WriteError};
use bios_core::locations::normalized_feature_locations;
use bios_core::{Construct, Feature, Fuzzy, LocationOperator, Segment};
use serde::Serialize;

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Snapshot {
    pub name: String,
    /// The construct as `bios_core::genbank_write::write` renders it. The server keeps these
    /// bytes as they are.
    pub text: String,
    /// Uppercase bases.
    pub sequence: String,
    pub circular: bool,
    pub features: Vec<SnapshotFeature>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SnapshotFeature {
    pub name: String,
    /// The GenBank feature key, such as `CDS`.
    #[serde(rename = "type")]
    pub kind: String,
    pub start: usize,
    /// Before `start` when the feature runs through the origin of a circular construct.
    pub end: usize,
    pub strand: i8,
    /// The pieces of a joined location. Absent for one contiguous span.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub locations: Option<Vec<SnapshotLocation>>,
    /// `order` when the file said so. A join is the default and is not written.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub location_operator: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub start_fuzzy: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub end_fuzzy: Option<&'static str>,
    /// A location Bios does not interpret, as the file wrote it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub location_unsupported: Option<String>,
    /// Qualifiers by key. A key that repeats keeps every value, in file order.
    pub notes: BTreeMap<String, Vec<String>>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SnapshotLocation {
    pub start: usize,
    pub end: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub start_fuzzy: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub end_fuzzy: Option<&'static str>,
}

impl Snapshot {
    /// What is sent for `construct`. Refused for a construct with no bases: an empty construct
    /// is never sent anywhere.
    pub fn of(construct: &Construct) -> Result<Snapshot, WriteError> {
        let text = genbank_write::write(construct)?;
        let length = construct.sequence.len();
        Ok(Snapshot {
            name: construct.name.clone(),
            text,
            sequence: construct.sequence.to_ascii_uppercase(),
            circular: construct.circular,
            features: construct.features.iter().map(|f| feature(f, length, construct.circular)).collect(),
        })
    }
}

fn mark(fuzzy: Option<Fuzzy>) -> Option<&'static str> {
    fuzzy.map(|f| match f {
        Fuzzy::Before => "<",
        Fuzzy::After => ">",
    })
}

fn location(segment: &Segment) -> SnapshotLocation {
    SnapshotLocation { start: segment.start, end: segment.end, start_fuzzy: mark(segment.start_fuzzy), end_fuzzy: mark(segment.end_fuzzy) }
}

fn feature(f: &Feature, length: usize, circular: bool) -> SnapshotFeature {
    let pieces = normalized_feature_locations(f, length, circular);
    // One contiguous span carries its uncertain ends on the feature; a joined one on its pieces.
    let single = if let [only] = f.segments.as_slice() { Some(*only) } else { None };
    let (start_fuzzy, end_fuzzy) = match pieces {
        Some(_) => (None, None),
        None => (mark(single.and_then(|s| s.start_fuzzy).or(f.start_fuzzy)), mark(single.and_then(|s| s.end_fuzzy).or(f.end_fuzzy))),
    };
    let mut notes: BTreeMap<String, Vec<String>> = BTreeMap::new();
    // The name is the label; it is not sent a second time among the qualifiers.
    for (key, values) in f.notes.iter().filter(|(key, _)| key != "label") {
        notes.entry(key.clone()).or_default().extend(values.iter().cloned());
    }
    SnapshotFeature {
        name: f.name.clone(),
        kind: f.kind.clone(),
        start: f.start,
        end: f.end,
        strand: f.strand.sign(),
        locations: pieces.map(|p| p.iter().map(location).collect()),
        location_operator: (f.location_operator == Some(LocationOperator::Order)).then_some("order"),
        start_fuzzy,
        end_fuzzy,
        location_unsupported: f.location_unsupported.clone(),
        notes,
    }
}
