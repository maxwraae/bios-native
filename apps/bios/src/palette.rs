//! ⌘P's palette in the window: the project's plasmids to choose from, and opening the one chosen
//! in Edit (FilePalette.tsx, `pick`).

use bios_ui::quick_open::{Entry, Outcome};
use bios_ui::sidebar::Place;
use egui::Rect;

use crate::App;

impl App {
    /// The plasmids as the palette lists them: each one's name and folder.
    pub(crate) fn palette_entries(&self) -> Vec<Entry> {
        self.listing.iter().map(|l| Entry { name: l.name.clone(), dir: l.path.parent().map(crate::label).unwrap_or_default() }).collect()
    }

    /// Draws the palette while it is open, and opens what was picked.
    pub(crate) fn quick_open_palette(&mut self, ui: &mut egui::Ui, area: Rect) {
        if self.quick_open.is_none() {
            return;
        }
        let entries = self.palette_entries();
        let project_open = self.session.is_some();
        let Some(palette) = self.quick_open.as_mut() else {
            return;
        };
        match palette.show(ui, area, &entries, project_open) {
            Some(Outcome::Pick(i)) => {
                self.quick_open = None;
                self.palette_pick(i);
            }
            Some(Outcome::Close) => self.quick_open = None,
            None => {}
        }
    }

    /// Opens the `i`th plasmid of the listing in Edit.
    pub(crate) fn palette_pick(&mut self, i: usize) {
        if let Some(id) = self.listing.get(i).map(|l| l.id.clone()) {
            self.go_to_place(Place::Edit);
            self.open_construct(&id);
        }
    }
}
