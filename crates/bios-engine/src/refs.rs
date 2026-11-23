//! How a caller names a construct or a feature, and how that name is resolved.

use std::path::Path;

use bios_core::{Construct, Feature};
use bios_project::Listed;

use crate::{Failure, fail};

/// A construct's path as it is shown and typed: folders and name, no extension.
pub fn label(path: &Path) -> String {
    path.with_extension("").components().map(|c| c.as_os_str().to_string_lossy()).collect::<Vec<_>>().join("/")
}

/// Finds the construct `reference` names: its id, its path as [`label`] shows it, or its name
/// when only one construct has it.
pub fn plasmid<'a>(listed: &'a [Listed], reference: &str) -> Result<&'a Listed, Failure> {
    let wanted = reference.trim().trim_end_matches(".gb");
    if let Some(hit) = listed.iter().find(|l| l.id.as_str() == wanted) {
        return Ok(hit);
    }
    if let Some(hit) = listed.iter().find(|l| label(&l.path) == wanted) {
        return Ok(hit);
    }
    let lower = wanted.to_lowercase();
    let hits: Vec<&Listed> = listed.iter().filter(|l| label(&l.path).to_lowercase() == lower || l.name.to_lowercase() == lower).collect();
    match hits.as_slice() {
        [one] => Ok(one),
        [] => {
            let all: Vec<String> = listed.iter().map(|l| label(&l.path)).collect();
            fail(format!("no plasmid called \"{wanted}\". Plasmids here: {}", if all.is_empty() { "none".into() } else { all.join(", ") }))
        }
        many => fail(format!(
            "\"{wanted}\" matches {} plasmids: {}. Use the path or the id",
            many.len(),
            many.iter().map(|l| label(&l.path)).collect::<Vec<_>>().join(", ")
        )),
    }
}

/// The value of a `bios-<key>: value` note, which is where Bios keeps its own facts about a feature.
pub fn bios_note<'a>(feature: &'a Feature, key: &str) -> Option<&'a str> {
    let prefix = format!("bios-{key}:");
    feature.note("note")?.iter().find_map(|n| n.strip_prefix(&prefix)).map(str::trim)
}

/// Finds the feature `reference` names: `#3` or `3` for the third feature, a bios id, or a name
/// when only one feature has it. Gives its index in `construct.features`.
pub fn feature(construct: &Construct, reference: &str) -> Result<usize, Failure> {
    let wanted = reference.trim();
    let features = &construct.features;
    if let Ok(n) = wanted.trim_start_matches('#').parse::<usize>() {
        return if n >= 1 && n <= features.len() { Ok(n - 1) } else { fail(format!("no feature #{n} (there are {})", features.len())) };
    }
    let by = |test: &dyn Fn(&Feature) -> bool| -> Vec<usize> { features.iter().enumerate().filter(|(_, f)| test(f)).map(|(i, _)| i).collect() };
    let mut hits = by(&|f| bios_note(f, "id") == Some(wanted));
    if hits.is_empty() {
        hits = by(&|f| f.name == wanted);
    }
    if hits.is_empty() {
        hits = by(&|f| f.name.to_lowercase() == wanted.to_lowercase());
    }
    match hits.as_slice() {
        [one] => Ok(*one),
        [] => {
            let all: Vec<String> = features.iter().enumerate().map(|(i, f)| format!("#{} {}", i + 1, f.name)).collect();
            fail(format!("no feature called \"{wanted}\". Features: {}", if all.is_empty() { "none".into() } else { all.join(", ") }))
        }
        many => {
            let places: Vec<String> =
                many.iter().filter_map(|i| features.get(*i).map(|f| format!("#{} ({}..{})", i + 1, f.start + 1, f.end + 1))).collect();
            fail(format!("\"{wanted}\" matches {} features: {}. Use the #index or the bios id", many.len(), places.join(", ")))
        }
    }
}
