//! The side panels on the editor pane's right: Contents and Enzymes. Each is drawn by its module in
//! `bios-ui`; here is what each one's choices do.

use bios_core::enzymes::find_cut_sites;
use bios_ui::contents_panel::Outcome as ContentsOutcome;
use bios_ui::enzymes_panel::Outcome as EnzymesOutcome;
use bios_ui::side_panel::{self, Side};
use bios_ui::view_tab::Layers;
use egui::Rect;

use crate::App;

/// What the new-feature sheet said beyond the name, as `add_feature` takes it.
pub(crate) fn said_about(ask: &bios_ui::ask::Ask, params: &mut serde_json::Value) {
    if let Some(category) = ask.category() {
        params["category"] = serde_json::json!(category);
    }
    let why = ask.why();
    if !why.is_empty() {
        params["why"] = serde_json::json!(why);
    }
    match &ask.serves {
        Some(bios_ui::ask::Serves::Construct) => params["serves"] = serde_json::json!("construct"),
        Some(bios_ui::ask::Serves::Feature(id)) => params["serves"] = serde_json::json!(id),
        None => {}
    }
}

/// The start-up flags that reach the side panels, the layers' tracks and the drop, for a picture.
pub(crate) struct Flags {
    pub dropping: bool,
    pub drop_files: Option<String>,
    pub panel: Option<String>,
    pub translate: Option<String>,
    pub enzyme_search: Option<String>,
}

impl App {
    /// Puts the window where the flags ask: `--dropping`, `--drop`, `--panel`, `--translate` and
    /// `--enzyme-search`.
    pub(crate) fn flags(&mut self, flags: Flags) {
        self.dropping = flags.dropping;
        if let Some(files) = flags.drop_files {
            self.dropped(files.split(',').map(std::path::PathBuf::from).collect());
        }
        match flags.panel.as_deref() {
            Some("contents") => self.ed.pane.side = Some(Side::Contents),
            Some("enzymes") => self.ed.pane.side = Some(Side::Enzymes),
            Some("layers") => self.ed.pane.panel = Some(bios_ui::canvas_tools::Panel::Layers),
            Some("dial") => self.ed.pane.panel = Some(bios_ui::canvas_tools::Panel::Enzymes),
            Some(other) => self.problem = Some(format!("--panel takes contents, enzymes, layers or dial, not {other}")),
            None => {}
        }
        let key = self.opened.as_ref().map(|o| o.id.as_str().to_string());
        if let (Some(frame), Some((start, end)), Some(key)) = (flags.translate, self.ed.cursor.selection, key) {
            let forward = !frame.starts_with('-');
            match frame.trim_start_matches(['+', '-']).parse::<u8>() {
                Ok(n @ 1..=3) => self.ed.pane.add_translation(&key, bios_layout::translations::Translation { start, end, forward, frame: n - 1 }),
                _ => self.problem = Some(format!("--translate takes +1, +2, +3, -1, -2 or -3, not {frame}")),
            }
        }
        if let Some(query) = flags.enzyme_search {
            // A name that cuts is put on the map, as a press on its row does; anything else is typed.
            match bios_core::enzymes::get_enzyme(&query) {
                Some(e) if query == e.name => {
                    self.ed.pane.toggle_shown(e.name);
                }
                _ => self.ed.pane.enzymes_panel.query = query,
            }
        }
    }

    /// What a new feature could serve: every feature of the open plasmid with a bios id, primers
    /// left out, since a primer serves a feature and not the other way round.
    pub(crate) fn ask_targets(&self) -> Vec<bios_ui::ask::Target> {
        let Some(o) = &self.opened else {
            return Vec::new();
        };
        o.construct
            .features
            .iter()
            .filter(|f| !f.kind.eq_ignore_ascii_case("primer_bind"))
            .filter_map(|f| {
                let id = f.note("note")?.iter().find_map(|n| n.strip_prefix("bios-id:"))?.trim().to_string();
                Some(bios_ui::ask::Target { id, name: f.name.clone(), start: f.start, end: f.end })
            })
            .collect()
    }

    /// Draws the side panel that is open, standing in `pane` down to the tool row's foot.
    pub(crate) fn side_panels(&mut self, ui: &mut egui::Ui, pane: Rect, tools_bottom: f32) {
        let Some(side) = self.ed.pane.side else {
            return;
        };
        let rect = side_panel::place(pane, tools_bottom);
        let revision = self.revision;
        let Some(c) = self.opened.as_ref().map(|o| &o.construct).or(self.loose.as_ref()) else {
            return;
        };
        match side {
            Side::Enzymes => {
                let shown = self.ed.pane.shown_enzymes.clone();
                let outcome = self.ed.pane.enzymes_panel.show(ui, rect, revision, c, &shown);
                // Where an enzyme just put on the map first lands: its first recognition site.
                let first = |name: &str| {
                    let site = find_cut_sites(&c.sequence, &[name], c.circular).into_iter().next()?;
                    let length = bios_core::enzymes::get_enzyme(name).map_or(1, |e| e.site.len().max(1));
                    Some((site.recognition_start, site.recognition_start + length - 1))
                };
                match outcome {
                    Some(EnzymesOutcome::Close) => self.ed.pane.side = None,
                    Some(EnzymesOutcome::Toggle(name)) => {
                        let span = first(&name);
                        if self.ed.pane.toggle_shown(&name)
                            && let Some((start, end)) = span
                        {
                            self.select_span(start, end);
                        }
                    }
                    Some(EnzymesOutcome::GoTo(start, end)) => self.select_span(start, end),
                    None => {}
                }
            }
            Side::Contents => {
                let outcome = self.ed.pane.contents_panel.show(
                    ui,
                    rect,
                    c,
                    self.ed.cursor.selection,
                    self.ed.pane.layers,
                    &bios_ui::contents_panel::listed_parts(c),
                    &self.binder_rows(),
                );
                match outcome {
                    Some(ContentsOutcome::Close) => self.ed.pane.side = None,
                    Some(ContentsOutcome::Layers(layers)) => self.set_layers(layers),
                    Some(ContentsOutcome::Select(start, end)) => self.select_span(start, end),
                    Some(ContentsOutcome::Feature(index)) => {
                        let span = self.opened.as_ref().and_then(|o| o.construct.features.get(index)).map(|f| (f.start, f.end));
                        if let Some((start, end)) = span {
                            self.select_span(start, end);
                            self.open_card(index);
                        }
                    }
                    Some(ContentsOutcome::RemoveBinder(index)) => self.remove_binder(index),
                    None => {}
                }
            }
        }
    }

    /// The binders held against the open plasmid, one row per oligo (`binderRows`): its clean home
    /// site, or else its first, and how many other places it also binds.
    pub(crate) fn binder_rows(&self) -> Vec<bios_ui::contents_panel::BinderRow> {
        let Some(o) = &self.opened else {
            return Vec::new();
        };
        let sites = self.ed.oligos.sites();
        self.ed
            .oligos
            .binders_of(&o.id)
            .map(|b| {
                let mine: Vec<_> = sites.iter().filter(|s| s.binder_id == b.id).collect();
                let home = mine.iter().find(|s| s.on_target).or(mine.first());
                bios_ui::contents_panel::BinderRow {
                    name: b.name.clone(),
                    bases: b.bases.clone(),
                    home: home.map(|s| (s.site.start, s.site.end)),
                    forward: home.is_none_or(|s| s.site.forward),
                    strays: mine.iter().filter(|s| !s.on_target).count(),
                }
            })
            .collect()
    }

    /// The open plasmid's floating translations the selection overlaps, as the menu offers to remove
    /// them: their index and `+2 1–30`.
    pub(crate) fn translations_here(&self) -> Vec<(usize, String)> {
        let (Some(o), Some((start, end))) = (&self.opened, self.ed.cursor.selection) else {
            return Vec::new();
        };
        let (lo, hi) = if end < start { (0, usize::MAX) } else { (start, end) };
        let thousands = bios_ui::selection_readout::thousands;
        self.ed
            .pane
            .translations_of(o.id.as_str())
            .iter()
            .enumerate()
            .filter(|(_, t)| t.start <= hi && t.end >= lo)
            .map(|(i, t)| (i, format!("{} {}–{}", t.frame_label(), thousands(t.start + 1), thousands(t.end + 1))))
            .collect()
    }

    /// Changes what is shown. What is drawn is laid out again, since a layer can change it.
    pub(crate) fn set_layers(&mut self, layers: Layers) {
        if layers != self.ed.pane.layers {
            self.ed.pane.layers = layers;
            self.revision += 1;
        }
    }

    /// Selects bases of the open plasmid, 0-based inclusive, and brings them into view, as a press
    /// on the thing on the map would.
    pub(crate) fn select_span(&mut self, start: usize, end: usize) {
        let Some(opened) = self.opened.as_mut() else {
            return;
        };
        let last = opened.construct.len().saturating_sub(1);
        opened.editing.select(start.min(last), end.min(last));
        self.ed.anchor = None;
        self.show_editing(None, true);
    }
}
