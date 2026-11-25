//! What floats over the open plasmid: the feature panel, the selection's read-out, the menu on a
//! selection and the Find bar. Each is drawn by its module in `bios-ui`; here is what each one's
//! choices do, always through the engine's commands.

use bios_core::enzymes::OverhangKind;
use bios_core::find_align::describe_hit;
use bios_core::seq::clean_seq;
use bios_engine::editor::edit::Direction;
use bios_ui::ask::{self, Ask};
use bios_ui::binder_card;
use bios_ui::cut_hover;
use bios_ui::feature_card::{self, FeatureCard};
use bios_ui::find_bar::{self, Aligning, FindBar, FindMatch, Mode, find_matches};
use bios_ui::primer_card::{self, PrimerFacts};
use bios_ui::row_menu::{self, Action, CutHere, Outcome as MenuOutcome, RowMenu, Thing, menu_groups};
use bios_ui::rows;
use bios_ui::selection_readout::{self, Outcome, Selected};
use egui::{Rect, vec2};
use serde_json::json;

use crate::{App, menus};

impl App {
    /// Opens the panel of one of the open plasmid's features or primers. A panel already open on
    /// another one lands what was typed in it first, as leaving its field would have.
    pub(crate) fn open_card(&mut self, feature: usize) {
        if let Some(open) = self.ed.card.take()
            && open.changed()
        {
            self.commit_card(&open);
        }
        self.ed.card = self.opened.as_ref().and_then(|o| FeatureCard::of(&o.construct, feature));
    }

    /// Writes what the panel holds, through `update_feature`, naming only what changed. The panel
    /// stays open on the record that was written; refused, it keeps what was typed and the reason
    /// is shown where problems are.
    pub(crate) fn commit_card(&mut self, card: &FeatureCard) {
        let Some(opened) = &self.opened else {
            return;
        };
        let (was, now) = (&card.was, &card.edit);
        let mut params = json!({ "plasmid": opened.id.as_str(), "feature": format!("#{}", card.feature + 1) });
        // An empty name is not a rename (editFeature.ts).
        let name = now.name.trim();
        if !name.is_empty() && name != was.name {
            params["name"] = json!(name);
        }
        if now.category != was.category {
            params["category"] = json!(now.category);
        }
        if now.forward != was.forward {
            params["strand"] = json!(if now.forward { "forward" } else { "reverse" });
        }
        if now.purpose.trim() != was.purpose {
            params["why"] = json!(now.purpose.trim());
        }
        if now.description.trim() != was.description {
            params["description"] = json!(now.description.trim());
        }
        // The span is kept to the bases that exist, and an end before the start is a wrap only on
        // a circular construct.
        let length = opened.construct.len().max(1);
        let (mut start, mut end) = (now.start.clamp(1, length), now.end.clamp(1, length));
        if end < start && !opened.construct.circular {
            std::mem::swap(&mut start, &mut end);
        }
        let moved = (start, end) != (was.start, was.end);
        if params.as_object().is_some_and(|p| p.len() == 2) && !moved {
            return;
        }
        if moved {
            // Moving names the version it is made on, so what was typed is saved first.
            if !self.save() {
                return;
            }
            params["start"] = json!(start);
            params["end"] = json!(end);
            params["version"] = json!(self.opened.as_ref().map(|o| o.token.clone()).unwrap_or_default());
        }
        let index = card.feature;
        if self.run("update_feature", params).is_some() {
            let fresh = self.opened.as_ref().and_then(|o| FeatureCard::of(&o.construct, index));
            self.ed.card = fresh.map(|fresh| {
                let mut kept = card.refreshed(fresh);
                kept.edit.start = start;
                kept.edit.end = end;
                kept
            });
        } else {
            self.ed.card = Some(card.clone());
        }
    }

    /// Writes everything the panel's feature acts on, through `set_feature_actions`. Saying one
    /// turns the Interactions layer on, so it is seen (`PlasmidEditor.tsx` `onSaveActions`), and the
    /// panel stays open on the record just written: stating what something acts on goes a verb at a
    /// time.
    fn write_actions(&mut self, card: &FeatureCard, actions: &[bios_core::acts::Action]) {
        let Some(opened) = &self.opened else {
            return;
        };
        let list: Vec<serde_json::Value> = actions.iter().map(|a| json!({ "verb": a.verb, "targets": a.targets })).collect();
        let params = json!({ "plasmid": opened.id.as_str(), "feature": format!("#{}", card.feature + 1), "actions": list });
        if self.run("set_feature_actions", params).is_none() {
            return;
        }
        if !actions.is_empty() {
            self.ed.pane.layers.interactions = true;
        }
        let fresh = self.opened.as_ref().and_then(|o| FeatureCard::of(&o.construct, card.feature));
        self.ed.card = fresh.map(|fresh| card.refreshed(fresh));
    }

    /// Everything that floats over the rows and the map, drawn after both.
    /// `pane` is the editor's whole area, which the read-out keeps inside.
    pub(crate) fn floating(&mut self, ui: &mut egui::Ui, pane: Rect, rows_rect: Rect) {
        let seen = rows::seen(ui.ctx());
        // The panel first: its Escape gives way to a field of the others that has the keys.
        self.feature_card(ui, rows_rect, &seen);
        self.readout(ui, pane, &seen);
        self.find_bar(ui, pane);
        self.row_menu(ui, &seen);
        self.hover_cards(ui.ctx(), &seen);
        self.primer_drag_frame(ui.ctx());
    }

    /// The primer's card while the pointer is on a primer, and the cut's while it is on a cut
    /// whose name the rows are not showing.
    fn hover_cards(&self, ctx: &egui::Context, seen: &rows::Seen) {
        /// The rows always print each cut's name; the web's card is for when they do not.
        const CUT_NAMES_SHOWN: bool = true;
        let Some(opened) = &self.opened else {
            return;
        };
        // The primer's card follows the pointer alone, menu or no menu (`PrimerCard.tsx`).
        if let Some((index, rect)) = seen.bar
            && let Some(facts) = PrimerFacts::of(&opened.construct, index)
        {
            primer_card::show(ctx, &facts, rect);
        }
        if self.ed.row_menu.is_some() {
            return;
        }
        // A binder says what it is, and everywhere else it lands (`BinderCard.tsx`).
        if let Some((rows::Oligo::Binder(site), rect)) = seen.oligo
            && let Some((_, sites)) = self.ed.oligos.binder_at(&opened.id, site)
        {
            binder_card::show(ctx, &sites, rect);
        }
        // A Find hit says how it fits, on the hit, since the rows no longer caption it (PARITY.md).
        if let Some((rows::Oligo::Probe(index), rect)) = seen.oligo
            && let Some(hit) = self.ed.oligos.aligner.hits(self.revision).get(index)
        {
            let mut parts = vec![bios_core::find_align::probe_name(hit)];
            parts.extend(bios_core::find_align::describe_hit(hit, Some(opened.construct.len())).split(" · ").map(str::to_string));
            cut_hover::show(ctx, &parts, rect);
        }
        // What Exact found says what it is on the ribbon: its length and strand, or the enzyme.
        if let Some((rows::Oligo::Found(index), rect)) = seen.oligo
            && let Some(m) = self.ed.find.as_ref().and_then(|(_, matches, _)| matches.get(index))
        {
            let mut parts = vec!["found".to_string()];
            parts.extend(m.label.split(" · ").map(str::to_string));
            cut_hover::show(ctx, &parts, rect);
        }
        if !CUT_NAMES_SHOWN
            && let Some((site, rect)) = seen.cut
            && let Some(parts) = self.ed.plan.as_ref().and_then(|p| p.site(site)).and_then(|s| cut_hover::describe(s.enzyme))
        {
            cut_hover::show(ctx, &parts, rect);
        }
    }

    /// The menu a right-click on the rows opens: on what is under the pointer, and on the selection.
    fn row_menu(&mut self, ui: &mut egui::Ui, seen: &rows::Seen) {
        let wanted = seen.menu.or_else(|| seen.selection.filter(|_| std::mem::take(&mut self.menu_on_open)).map(|r| r.center()));
        if let (Some(at), Some(opened)) = (wanted, &self.opened) {
            let binder = match seen.oligo {
                Some((rows::Oligo::Binder(site), _)) => self.ed.oligos.binder_at(&opened.id, site).map(|(index, _)| Thing::Binder(index)),
                _ => None,
            };
            let thing = binder.or_else(|| {
                seen.bar.and_then(|(i, _)| {
                    let f = opened.construct.features.get(i)?;
                    Some(if f.kind == "primer_bind" { Thing::Primer(i) } else { Thing::Feature(i) })
                })
            });
            let cuts = self.cuts_here();
            let groups = row_menu::with_translations(menu_groups(thing, self.ed.cursor.selection.is_some(), &cuts), &self.translations_here());
            self.ed.row_menu = RowMenu::new(at, groups);
        }
        let Some(mut menu) = self.ed.row_menu.take() else {
            return;
        };
        match row_menu::show(ui.ctx(), &mut menu) {
            Some(MenuOutcome::Run(action)) => self.menu_action(action, menu.at),
            Some(MenuOutcome::Close) => {}
            None => self.ed.row_menu = Some(menu),
        }
    }

    /// The cuts the rows show whose top-strand cut falls inside the selection, in order
    /// (`SequenceRows.tsx` `cutsHere`).
    fn cuts_here(&self) -> Vec<CutHere> {
        let (Some((start, end)), Some(plan)) = (self.ed.cursor.selection, &self.ed.plan) else {
            return Vec::new();
        };
        let mut sites: Vec<_> = (0..).map_while(|i| plan.site(i)).filter(|c| c.top_cut >= start && c.top_cut <= end + 1).collect();
        sites.sort_by_key(|c| c.top_cut);
        let length = self.opened.as_ref().map_or(0, |o| o.construct.len());
        sites
            .into_iter()
            .map(|c| {
                let len = bios_core::enzymes::get_enzyme(c.enzyme).map_or(6, |e| e.length);
                let overhang = match c.overhang.kind {
                    OverhangKind::Blunt => "blunt".to_string(),
                    OverhangKind::FivePrime => format!("5′ {}", c.overhang.seq),
                    OverhangKind::ThreePrime => format!("3′ {}", c.overhang.seq),
                };
                let last = (c.recognition_start + len.saturating_sub(1)) % length.max(1);
                CutHere { enzyme: c.enzyme.to_string(), at: c.top_cut, overhang, site: (c.recognition_start, last) }
            })
            .collect()
    }

    /// Does what was chosen from the menu, through the same paths the keys and the pill use.
    pub(crate) fn menu_action(&mut self, action: Action, at: egui::Pos2) {
        let selection = self.ed.cursor.selection;
        let bases = match (&self.opened, selection) {
            (Some(o), Some((start, end))) => selection_readout::selected_bases(&o.construct, start, end),
            _ => String::new(),
        };
        match action {
            Action::Card(feature) => {
                self.open_card(feature);
                if let Some(card) = self.ed.card.as_mut() {
                    card.anchor = Some(at);
                }
            }
            Action::RemoveFeature(feature) => {
                if let Some(id) = self.opened.as_ref().map(|o| o.id.clone()) {
                    self.run("remove_feature", json!({ "plasmid": id.as_str(), "feature": format!("#{}", feature + 1) }));
                }
            }
            Action::RemovePrimer(feature) => {
                if let Some(id) = self.opened.as_ref().map(|o| o.id.clone()) {
                    self.run("remove_primer", json!({ "plasmid": id.as_str(), "primer": format!("#{}", feature + 1) }));
                }
            }
            Action::RemoveBinder(index) => self.remove_binder(index),
            Action::Binder => {
                if let Some((start, end)) = selection {
                    self.add_binder_from_selection(start, end);
                }
            }
            Action::Copy => self.copy(false),
            Action::CopyReverse => self.copy(true),
            Action::CopyFasta => {
                if let (Some(o), Some((start, end))) = (&self.opened, selection) {
                    self.to_clipboard = Some(format!(">{} {}-{}\n{bases}\n", o.construct.name, start + 1, end + 1));
                }
            }
            Action::CopyProtein => self.to_clipboard = Some(bios_core::seq::translate(&bases)),
            Action::Delete => self.edit(|editing| editing.delete(Direction::Back)),
            Action::Feature => self.command(menus::NEW_FEATURE),
            Action::Primer => self.command(menus::NEW_PRIMER),
            Action::Find => self.open_find(Some(bases)),
            Action::NewConstruct => {
                if let (Some(o), Some((start, end))) = (&self.opened, selection) {
                    let name = format!("{} {}–{}", o.construct.name, selection_readout::thousands(start + 1), selection_readout::thousands(end + 1));
                    let params =
                        json!({ "name": name, "from_plasmid": o.id.as_str(), "from_start": start + 1, "from_end": end + 1, "circular": false });
                    if let Some(made) = self.run("create_plasmid", params)
                        && let Some(id) = made["id"].as_str().and_then(bios_project::ConstructId::parse)
                    {
                        self.open_construct(&id);
                    }
                }
            }
            Action::Site(start, end) => {
                if let Some(opened) = self.opened.as_mut() {
                    opened.editing.select(start, end);
                }
                self.ed.anchor = None;
                self.show_editing(None, true);
            }
            Action::Translate { forward, frame } => {
                if let (Some((start, end)), Some(key)) = (selection, self.opened.as_ref().map(|o| o.id.as_str().to_string())) {
                    // A selection through the origin reads only up to the end; the rows draw one run.
                    let (start, end) =
                        if end < start { (start, self.opened.as_ref().map_or(start, |o| o.construct.len().saturating_sub(1))) } else { (start, end) };
                    self.ed.pane.add_translation(&key, bios_layout::translations::Translation { start, end, forward, frame });
                }
            }
            Action::RemoveTranslation(index) => {
                if let Some(key) = self.opened.as_ref().map(|o| o.id.as_str().to_string()) {
                    self.ed.pane.remove_translation(&key, index);
                }
            }
            Action::Optimise => {
                if let Some((start, end)) = selection {
                    let ask = ask::Prompt::new("Codon-optimise for (human, mouse, ecoli or yeast)", "", "Optimise");
                    self.prompt = Some((crate::Asked::Optimise(start, end), ask));
                }
            }
        }
    }

    /// Codon-optimises bases of the open plasmid for an organism, keeping the protein.
    pub(crate) fn optimise(&mut self, start: usize, end: usize, organism: &str) {
        if !self.save() {
            return;
        }
        let Some(opened) = &self.opened else {
            return;
        };
        let params =
            json!({ "plasmid": opened.id.as_str(), "version": opened.token, "start": start + 1, "end": end + 1, "organism": organism.trim() });
        self.run("codon_optimise", params);
    }

    fn feature_card(&mut self, ui: &mut egui::Ui, rows_rect: Rect, seen: &rows::Seen) {
        if let Some(mut card) = self.ed.card.take() {
            // Opened by a press: at the press. Opened otherwise: over the feature's place in the
            // rows, which is selected.
            let fallback = ui
                .input(|i| i.pointer.press_origin())
                .filter(|_| card.anchor.is_none())
                .or_else(|| seen.selection.map(|r| r.center_top()))
                .unwrap_or_else(|| rows_rect.center_top() + egui::vec2(0.0, 40.0));
            let outcome = feature_card::show(ui.ctx(), &mut card, fallback);
            self.ed.card = Some(card.clone());
            if let Some(actions) = &outcome.actions {
                self.write_actions(&card, actions);
                card = self.ed.card.clone().unwrap_or(card);
            }
            if outcome.commit && card.changed() {
                self.ed.card = None;
                self.commit_card(&card);
            }
            if outcome.close {
                self.ed.card = None;
            }
        }
    }

    /// Opens ⌘F's bar, or gives its field the keyboard again; with `query`, with that typed.
    pub(crate) fn open_find(&mut self, query: Option<String>) {
        match (&mut self.ed.find, query) {
            (Some((bar, ..)), None) => bar.refocus(),
            (Some((bar, ..)), Some(query)) => {
                bar.query = query;
                bar.refocus();
                self.find_again();
            }
            (None, query) => {
                self.ed.find = Some((FindBar::new(query), Vec::new(), self.revision));
                self.find_again();
            }
        }
        self.show_sequence();
    }

    /// Alignment draws on the rows, so asking to see a duplex raises them if only the map is shown.
    fn show_sequence(&mut self) {
        if self.ed.find.as_ref().is_some_and(|(bar, ..)| bar.mode == Mode::Alignment) && self.ed.pane.focus == bios_layout::pane::Focus::Map {
            self.ed.pane.focus = bios_layout::pane::Focus::Split;
        }
    }

    /// What the bar lists: Find's alignments, best first, while it aligns, else the exact matches.
    fn matches_of(&self, bar: &FindBar) -> Vec<FindMatch> {
        let Some(opened) = &self.opened else {
            return Vec::new();
        };
        if !bar.aligns() {
            return find_matches(&opened.construct, &bar.query);
        }
        let n = opened.construct.len().max(1);
        self.ed
            .oligos
            .aligner
            .hits(self.revision)
            .iter()
            .map(|h| FindMatch {
                start: h.alignment.start % n,
                end: h.alignment.end % n,
                forward: h.alignment.forward,
                label: describe_hit(h, Some(n)),
            })
            .collect()
    }

    /// Finds the query again and goes to the first match.
    pub(crate) fn find_again(&mut self) {
        let Some(found) = self.ed.find.as_ref().map(|(bar, ..)| self.matches_of(bar)) else {
            return;
        };
        if let Some((bar, matches, revision)) = &mut self.ed.find {
            *matches = found;
            *revision = self.revision;
            bar.index = 0;
        }
        self.go_to_match();
    }

    /// Selects the current match and brings it into view.
    fn go_to_match(&mut self) {
        let Some(m) = self.ed.find.as_ref().and_then(|(bar, matches, _)| matches.get(bar.index)).cloned() else {
            return;
        };
        if let Some(opened) = self.opened.as_mut() {
            opened.editing.select(m.start, m.end);
        }
        self.ed.anchor = None;
        self.show_editing(None, true);
    }

    fn find_bar(&mut self, ui: &mut egui::Ui, pane: Rect) {
        let Some((mut bar, mut matches, revision)) = self.ed.find.take() else {
            return;
        };
        // The query goes to the aligner, off this thread; its answer re-seeds the list at the first
        // fit, as a new query does.
        let bases = bar.aligns().then(|| clean_seq(&bar.query));
        let mut aligning = Aligning::default();
        let mut arrived = false;
        if let Some(opened) = &self.opened {
            arrived = self.ed.oligos.aligner.want(ui.ctx(), &opened.construct, self.revision, bases.clone());
            aligning = Aligning {
                pending: self.ed.oligos.aligner.pending(),
                too_long: bases.as_deref().and_then(|b| crate::oligos::too_long(&opened.construct, b)),
                error: None,
                capped: self.ed.oligos.aligner.capped(self.revision),
            };
        }
        // What is drawn changed under the bar: what it found is found again, staying where it was.
        if revision != self.revision && !arrived {
            matches = self.matches_of(&bar);
            bar.index = bar.index.min(matches.len().saturating_sub(1));
        }
        let outcome = find_bar::show(ui, pane, &mut bar, &matches, &aligning);
        self.ed.find = Some((bar, matches, self.revision));
        if arrived {
            self.find_again();
        }
        match outcome {
            Some(find_bar::Outcome::Changed) => self.find_again(),
            Some(find_bar::Outcome::Mode(mode)) => {
                if let Some((bar, ..)) = &mut self.ed.find {
                    bar.mode = mode;
                }
                self.show_sequence();
                self.find_again();
            }
            Some(find_bar::Outcome::Keep) => {
                if let Some(query) = bases {
                    self.add_binder(&query);
                }
                self.ed.oligos.aligner.clear();
                self.ed.find = None;
            }
            Some(find_bar::Outcome::Step(by)) => {
                if let Some((bar, matches, _)) = &mut self.ed.find
                    && !matches.is_empty()
                {
                    let n = matches.len() as i64;
                    bar.index = usize::try_from((bar.index as i64 + i64::from(by)).rem_euclid(n)).unwrap_or(0);
                }
                self.go_to_match();
            }
            Some(find_bar::Outcome::Close) => {
                self.ed.oligos.aligner.clear();
                self.ed.find = None;
            }
            None => {}
        }
    }

    /// The selection's pill, or the sheet it turns into for + Feature. It stands down while the
    /// pointer is over a primer, whose own card says the same things (`primerHover.ts`).
    fn readout(&mut self, ui: &mut egui::Ui, pane: Rect, seen: &rows::Seen) {
        let Some(opened) = &self.opened else {
            return;
        };
        let Some((start, end)) = self.ed.cursor.selection else {
            self.ed.primer_ask = None;
            return;
        };
        if let Some(mut sheet) = self.ed.ask.take() {
            let top_left = selection_readout::placement(seen.selection, None, vec2(ask::WIDTH, 330.0), pane);
            match ask::show(ui, top_left + vec2(ask::WIDTH, 0.0), &mut sheet) {
                Some(ask::Outcome::Done) => self.land(&sheet),
                Some(ask::Outcome::Cancelled) => {}
                None => self.ed.ask = Some(sheet),
            }
            return;
        }
        // A new selection lets go of an ask opened for the last one.
        if self.ed.primer_ask.as_ref().is_some_and(|a| a.span != (start, end)) {
            self.ed.primer_ask = None;
        }
        if !ui.input(|i| i.pointer.primary_down()) {
            self.ed.sweeping = false;
        }
        if self.ed.sweeping && self.ed.primer_ask.is_none() {
            return;
        }
        let on_primer = seen.bar.and_then(|(i, _)| opened.construct.features.get(i)).is_some_and(|f| f.kind == "primer_bind");
        if on_primer && self.ed.primer_ask.is_none() {
            return;
        }
        // Where the Find bar already describes this exact span, the pill would only repeat it.
        let hits: Vec<(usize, usize)> =
            self.ed.find.as_ref().map(|(_, matches, _)| matches.iter().map(|m| (m.start, m.end)).collect()).unwrap_or_default();
        if self.ed.primer_ask.is_none() && find_bar::readout_yields_to_find(Some((start, end)), &hits) {
            return;
        }
        let selected = Selected::of(&opened.construct, start, end);
        match selection_readout::show(ui, &selected, seen.selection, seen.selection_gap, pane, self.ed.primer_ask.as_mut(), true) {
            Some(Outcome::Primer) => self.command(menus::NEW_PRIMER),
            Some(Outcome::Feature) => self.command(menus::NEW_FEATURE),
            Some(Outcome::Make { forward, name }) => {
                self.ed.primer_ask = None;
                let mut primer = Ask::new(ask::What::Primer, forward);
                primer.name = name;
                self.land(&primer);
            }
            Some(Outcome::Cancel) => self.ed.primer_ask = None,
            None => {}
        }
    }
}
