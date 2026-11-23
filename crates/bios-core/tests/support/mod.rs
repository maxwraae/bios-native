//! What the ported edit tests share.
//!
//! The web app's tests read a feature's bases and its protein through a module that is not ported
//! yet (`bios/model.ts`). The few functions those tests call are ported here, as test code only,
//! so that the expectations can be kept whole. The translation itself is the crate's own.
#![allow(dead_code)]

use bios_core::coding::{CodingSettings, CodingTranslation, read_coding_settings, translate_coding};
use bios_core::model::{Feature, LocationOperator, Segment, Strand};
use bios_core::seq::revcomp;

pub type Notes = Vec<(String, Vec<String>)>;

/// A simple feature with no qualifiers.
pub fn feature(name: &str, kind: &str, start: usize, end: usize, strand: Strand) -> Feature {
    Feature {
        name: name.to_string(),
        kind: kind.to_string(),
        start,
        end,
        strand,
        segments: vec![Segment::new(start, end)],
        notes: Vec::new(),
        start_fuzzy: None,
        end_fuzzy: None,
        location_operator: None,
        location_unsupported: None,
    }
}

/// `{ ...f, locations }`: the same feature with these pieces, its `start` and `end` untouched.
pub fn with_locations(mut f: Feature, locations: &[Segment]) -> Feature {
    f.segments = locations.to_vec();
    f
}

pub fn segments(spans: &[(usize, usize)]) -> Vec<Segment> {
    spans.iter().map(|&(start, end)| Segment::new(start, end)).collect()
}

pub fn notes(pairs: &[(&str, &[&str])]) -> Notes {
    pairs.iter().map(|(key, values)| (key.to_string(), values.iter().map(|v| v.to_string()).collect())).collect()
}

pub fn strand(sign: i8) -> Strand {
    if sign < 0 { Strand::Reverse } else { Strand::Forward }
}

pub fn named<'a>(features: &'a [Feature], name: &str) -> Option<&'a Feature> {
    features.iter().find(|f| f.name == name)
}

/// The feature's own 5'->3' sequence, as `own` in `bios/model.ts` gives it for the model
/// `buildModel` makes of this feature.
pub fn own(sequence: &str, f: &Feature, circular: bool) -> Result<String, String> {
    let locations = f.locations();
    let crosses = !circular && (f.end < f.start || locations.is_some_and(|l| l.iter().any(|p| p.end < p.start)));
    let unsupported = f.location_unsupported.clone().or_else(|| crosses.then(|| "a location crosses the origin on a linear molecule".to_string()));
    if let Some(reason) = unsupported {
        return Err(format!("Cannot derive local sequence for {}: {reason}", f.name));
    }
    if f.location_operator == Some(LocationOperator::Order) {
        return Err(format!("{} has ordered segments whose connectivity is unspecified; they cannot be treated as a joined sequence", f.name));
    }
    let uncertain = f.start_fuzzy.is_some()
        || f.end_fuzzy.is_some()
        || locations.is_some_and(|l| l.iter().any(|p| p.start_fuzzy.is_some() || p.end_fuzzy.is_some()));
    if uncertain {
        return Err(format!("{} has uncertain boundaries; its complete sequence is not established", f.name));
    }
    let sequence = sequence.to_uppercase();
    let spans: Vec<(usize, usize)> = match locations {
        Some(l) => l.iter().map(|p| (p.start, p.end)).collect(),
        None => vec![(f.start, f.end)],
    };
    let mut bases = String::new();
    for (start, end) in spans {
        if end >= start {
            bases.push_str(&sequence[start..=end]);
        } else {
            bases.push_str(&sequence[start..]);
            bases.push_str(&sequence[..=end]);
        }
    }
    Ok(if f.strand == Strand::Reverse { revcomp(&bases) } else { bases })
}

pub fn bases(sequence: &str, f: &Feature, circular: bool) -> String {
    own(sequence, f, circular).unwrap()
}

/// `codingTranslation` of `bios/model.ts` for the first coding feature, which is what the web
/// app's `proteinOf` test helper asks the model for.
pub fn protein_of(sequence: &str, features: &[Feature], circular: bool) -> CodingTranslation {
    let mut ordered: Vec<&Feature> = features.iter().collect();
    ordered.sort_by(|a, b| (a.start, a.end, &a.name).cmp(&(b.start, b.end, &b.name)));
    let f = ordered.into_iter().find(|f| ["cds", "gene", "mrna"].contains(&f.kind.to_lowercase().as_str())).unwrap();
    let settings = read_coding_settings(&f.notes);
    let fuzzy = f.start_fuzzy.is_some() || f.end_fuzzy.is_some() || f.segments.iter().any(|p| p.start_fuzzy.is_some() || p.end_fuzzy.is_some());
    let crosses = !circular && (f.end < f.start || f.locations().is_some_and(|l| l.iter().any(|p| p.end < p.start)));
    let undefined = fuzzy || crosses || f.location_unsupported.is_some() || f.location_operator == Some(LocationOperator::Order);
    let issue =
        settings.unavailable.clone().or_else(|| undefined.then(|| "the feature location does not define a complete coding sequence".to_string()));
    match issue {
        Some(issue) => translate_coding("", CodingSettings { unavailable: Some(issue), ..settings }),
        None => translate_coding(&own(sequence, f, circular).unwrap(), settings),
    }
}
