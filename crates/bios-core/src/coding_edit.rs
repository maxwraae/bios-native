//! What an edit does to the reading frame of a coding feature.

use crate::coding::{StartCodonMode, read_coding_settings};
use crate::edits::EditError;
use crate::model::{Feature, Strand};
use crate::parts::{cut, feature_to_parts};

/// Remove `[ws, we)` from the old sequence and put `wl` new bases at `ws`. `ws == we` is an
/// insertion.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EditWindow {
    pub ws: usize,
    pub we: usize,
    pub wl: usize,
}

/// Keeps the frame of the surviving codons when an edit clips a coding feature's biological ends.
/// Interior indels still change the reading frame. Windows are on the original plus axis.
///
/// No base-by-base arrays: work scales with feature segments and edit windows.
pub fn coding_after_edit(feature: &Feature, length: usize, circular: bool, windows: &[EditWindow], substitution: bool) -> Result<Feature, EditError> {
    if !["cds", "gene", "mrna"].contains(&feature.kind.to_lowercase().as_str()) {
        return Ok(feature.clone());
    }
    let mut parts = feature_to_parts(feature, length, circular)?;
    let reverse = feature.strand == Strand::Reverse;
    if reverse {
        parts.reverse();
    }
    // Positions along the feature's own 5'->3' bases. Signed and wide because a span that is not
    // a real interval (an end before a start on a linear construct) has a negative length here.
    let wide = |n: usize| n as i128;
    let (mut offset, mut first, mut last, mut changed) = (0i128, None::<i128>, -1i128, false);
    for &(start, end) in &parts {
        let mut kept = vec![(start, end)];
        for w in windows {
            if w.ws == w.we {
                if w.wl != 0 && start < w.ws && w.ws < end {
                    changed = true;
                }
                continue;
            }
            if start.max(w.ws) < end.min(w.we) {
                changed = true;
            }
            kept = cut(&kept, w.ws, w.we);
        }
        for (s, e) in kept {
            let head = offset + if reverse { wide(end) - wide(e) } else { wide(s) - wide(start) };
            let tail = offset + if reverse { wide(end) - wide(s) } else { wide(e) - wide(start) } - 1;
            first = Some(first.map_or(head, |f| f.min(head)));
            last = last.max(tail);
        }
        offset += wide(end) - wide(start);
    }
    if !changed || (last < 0 && !substitution) {
        return Ok(feature.clone());
    }

    let mut out = feature.clone();
    out.notes.retain(|(key, _)| key != "translation");
    if !substitution {
        // `last >= 0` here, so a piece survived and `first` is set.
        if let Some(first) = first.filter(|&f| f > 0) {
            let settings = read_coding_settings(&feature.notes);
            if settings.phase <= 2 && !settings.unavailable.as_deref().is_some_and(|reason| reason.contains("codon_start")) {
                let frame = 1 + (i128::from(settings.phase) - first).rem_euclid(3);
                set_note(&mut out.notes, "codon_start", &frame.to_string());
            } else {
                set_note(&mut out.notes, "bios_translation_unavailable", "true");
            }
            set_note(&mut out.notes, "bios_cds_partial_start", "true");
            // A forced initiator belonged to the old first codon, which was removed.
            if settings.start_mode == Some(StartCodonMode::Methionine) {
                set_note(&mut out.notes, "bios_start_codon", "auto");
            }
        }
        if last < offset - 1 {
            set_note(&mut out.notes, "bios_cds_partial_end", "true");
        }
    }
    Ok(out)
}

/// Gives `key` the single value `value`, where the key already stands or else at the end.
fn set_note(notes: &mut Vec<(String, Vec<String>)>, key: &str, value: &str) {
    match notes.iter_mut().find(|(k, _)| k == key) {
        Some((_, values)) => *values = vec![value.to_string()],
        None => notes.push((key.to_string(), vec![value.to_string()])),
    }
}
