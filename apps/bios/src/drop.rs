//! Files dropped on the window: every one of them, said while they are still in the air, and one
//! sentence afterwards for what could not be opened. Ported from Bios `layout/fileDrop.ts`
//! (`partitionDropped`, `dropSummary`) and the shell's drop overlay (`AppLayout.tsx` 512-525,
//! `.shell-drop` in `styles.css` 425-447).

use std::path::{Path, PathBuf};

use bios_project::ConstructId;
use bios_ui::{icon, lucide, theme};
use egui::{CornerRadius, Rect, Stroke, pos2, vec2};
use serde_json::json;

use crate::App;

/// A file that was looked at and will not be opened, with the sentence to show for it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Refused {
    pub name: String,
    pub message: String,
}

/// Sequencing reads arrive through the same gesture as constructs and must not be read as one
/// (`isReadsFile`): a FASTQ opened as a construct is its first read, named after the run.
pub(crate) fn is_reads_file(path: &Path) -> bool {
    let name = path.file_name().map(|n| n.to_string_lossy().to_lowercase()).unwrap_or_default();
    let name = name.strip_suffix(".gz").unwrap_or(&name);
    name.ends_with(".fastq") || name.ends_with(".fq")
}

/// One sentence for a drop that could not open everything (`dropSummary`): nothing when all of it
/// opened, the one refusal's own sentence, or how many opened and the first clause of each refusal.
pub(crate) fn drop_summary(opened: usize, refused: &[Refused]) -> Option<String> {
    match refused {
        [] => None,
        [one] => Some(one.message.clone()),
        many => {
            let total = opened + many.len();
            let lead = if opened > 0 { format!("Opened {opened} of {total}. ") } else { String::new() };
            let first_clause = |m: &str| m.find(". ").map_or(m.to_string(), |at| m[..=at].to_string());
            let why: Vec<String> = many.iter().map(|r| first_clause(&r.message)).collect();
            Some(lead + &why.join(" "))
        }
    }
}

impl App {
    /// What a drop does: inside a project, every file joins it as a copy and the last one opens;
    /// a project folder, or a file with no project open, opens as before.
    pub(crate) fn dropped(&mut self, paths: Vec<PathBuf>) {
        let Some(first) = paths.first().cloned() else {
            return;
        };
        if self.session.is_none() || first.is_dir() {
            self.open_path(&first);
            return;
        }
        let mut refused = Vec::new();
        let mut opened: Option<ConstructId> = None;
        let mut count = 0;
        for path in &paths {
            let name = path.file_name().map_or_else(|| path.display().to_string(), |n| n.to_string_lossy().into_owned());
            if is_reads_file(path) {
                refused.push(Refused { name: name.clone(), message: format!("{name} is sequencing reads, not a construct. Open it in Verify.") });
                continue;
            }
            let Some(session) = self.session.as_ref() else { break };
            match session.execute("import_plasmid", &json!({ "file": path.display().to_string() })) {
                Ok(result) => {
                    count += 1;
                    opened = result["id"].as_str().and_then(ConstructId::parse).or(opened);
                }
                Err(failure) => refused.push(Refused { name: name.clone(), message: format!("{name} could not be opened. {}", failure.0) }),
            }
        }
        self.refresh();
        if let Some(id) = opened {
            self.open_construct(&id);
        }
        // Said last: opening a plasmid clears what was said before it.
        if let Some(said) = drop_summary(count, &refused) {
            if count == 0 {
                self.problem = Some(said);
            } else {
                self.notice = Some(said);
            }
        }
    }

    /// While files are held over the window, what letting go will do (`.shell-drop`).
    pub(crate) fn drop_overlay(&self, ui: &egui::Ui, full: Rect) {
        if !self.dropping && ui.ctx().input(|i| i.raw.hovered_files.is_empty()) {
            return;
        }
        let painter = ui.ctx().layer_painter(egui::LayerId::new(egui::Order::Tooltip, egui::Id::unique("shell-drop")));
        painter.rect_filled(full, 0.0, theme::alpha(theme::CANVAS, 0.72));
        let said = if self.session.is_some() { "Drop to add it to this project" } else { "Drop to open it" };
        let galley = painter.layout_no_wrap(said.to_string(), theme::sans(14.0), theme::INK);
        let size = vec2(22.0 + 18.0 + 9.0 + galley.size().x + 22.0, 16.0 + galley.size().y.max(18.0) + 16.0);
        let card = Rect::from_center_size(full.center(), size);
        bios_ui::primitives::shadow(&painter, card, theme::RADIUS, theme::SHADOW_2);
        painter.rect_filled(card, CornerRadius::same(theme::RADIUS as u8), theme::CELL);
        let dashes = [card.left_top(), card.right_top(), card.right_bottom(), card.left_bottom(), card.left_top()];
        painter.extend(egui::Shape::dashed_line(&dashes, Stroke::new(1.0, theme::INK), 4.0, 3.0));
        icon::paint(&painter, pos2(card.left() + 22.0 + 9.0, card.center().y), 18.0, lucide::UPLOAD, theme::INK);
        painter.galley(pos2(card.left() + 22.0 + 18.0 + 9.0, card.center().y - galley.size().y / 2.0), galley, theme::INK);
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::{Refused, drop_summary, is_reads_file};

    fn refused(message: &str) -> Refused {
        Refused { name: "x".into(), message: message.into() }
    }

    #[test]
    fn a_drop_that_opened_everything_says_nothing_and_one_refusal_says_itself() {
        assert_eq!(drop_summary(3, &[]), None);
        assert_eq!(
            drop_summary(0, &[refused("gel.png is a picture. Bios reads sequences.")]).as_deref(),
            Some("gel.png is a picture. Bios reads sequences.")
        );
        assert_eq!(drop_summary(2, &[refused("a.pdf is a PDF. Not a sequence.")]).as_deref(), Some("a.pdf is a PDF. Not a sequence."));
    }

    #[test]
    fn several_refusals_say_how_many_opened_and_the_first_clause_of_each() {
        let said = drop_summary(1, &[refused("a.pdf is a PDF. Not a sequence."), refused("b.zip is an archive. Unzip it first.")]);
        assert_eq!(said.as_deref(), Some("Opened 1 of 3. a.pdf is a PDF. b.zip is an archive."));
        let none = drop_summary(0, &[refused("a.pdf is a PDF. More."), refused("b.zip is an archive")]);
        assert_eq!(none.as_deref(), Some("a.pdf is a PDF. b.zip is an archive"));
    }

    #[test]
    fn reads_are_told_apart_by_name_gzipped_or_not() {
        assert!(is_reads_file(Path::new("/r/A1_R1.fastq.gz")) && is_reads_file(Path::new("x.FQ")));
        assert!(!is_reads_file(Path::new("pX.gb")) && !is_reads_file(Path::new("gz")));
    }
}
