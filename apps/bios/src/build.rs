//! The Build place in the window: the map that answers "where do we cut", the console that asks
//! "what goes in", and the folder's page once something has been built.
//!
//! Ported from the web app's `build/useBuild.tsx` (the state and everything derived from it),
//! `build/BuildConsole.tsx` and `build/FolderBuild.tsx`. The drawing is in `bios-ui`
//! (`build_console`, `build_folder`); this is what is being built and what happens when something
//! is pressed — and every one of those presses is an engine command, the same ones an agent calls.
//!
//! THE DESIGN IS COMPUTED WHEN YOU ASK, NOT ON EVERY KEYSTROKE, and not on the drawing thread. A
//! hundred inserts is a hundred designs, each a melting-temperature walk, a uniqueness scan over
//! the whole assembled plasmid and a split search; done between frames that is the window gone for
//! the duration. So `Compute` hands the work to a thread of its own, which opens its own session
//! on the same project and runs one `design_builds` per insert, and the rows fill in as each
//! lands.

use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, channel};

use bios_core::Construct;
use bios_core::build::{EnzymeOption, common_enzyme_options, is_opening, is_sheet_file, parse_insert_text};
use bios_core::enzymes::find_cut_sites;
use bios_engine::Session;
use bios_layout::overview::bands::{BandSeam, BandSegment, OuterBand};
use bios_project::{Actor, Listed, Project, Who};
use bios_ui::build_console::{self as console, Action, Arc, CutMode, EnzymePick, Inputs, Phase, Row, Stage};
use bios_ui::build_folder::{self as folder_page, DesignRow, PieceRow};
use bios_ui::rail_menu::{Item, Menu, Shown};
use bios_ui::{map, theme, view_tab};
use egui::{Align2, Rect, Ui, pos2, vec2};
use serde_json::{Value, json};

/// What the window must do after the place has drawn: everything that is not Build's own business.
#[derive(Clone, Debug, PartialEq)]
pub enum Request {
    /// The project changed: read it again.
    Refresh,
    /// Open this plasmid, by the path a listing shows.
    Open(String),
    /// Put this on the clipboard.
    Copy(String),
    /// Say this where problems are said.
    Say(String),
    /// Ask for sequence files or sheets from this machine; they come back through [`Build::take_files`].
    AskForFiles,
}

/// One thing to put in the opening. ONE PLASMID EACH: five inserts is five designs.
#[derive(Clone, Debug, Default)]
struct Insert {
    name: String,
    bases: String,
}

/// What a design came back saying, for the row that asked for it.
#[derive(Clone, Debug, Default)]
struct Designed {
    pieces: Vec<usize>,
    chip: String,
    warn: bool,
    order_line: String,
    /// Every piece as FASTA, for "Copy pieces".
    fasta: String,
    ok: bool,
    /// WHAT THE DESIGN LOOKS LIKE, for the band on the map: the arms the end pieces carry, each
    /// piece's span in the laid-out frame, and the junctions. Where the backbone opens is NOT
    /// kept here — the band follows the pickers live, so it reads that from the cut on screen.
    arms: (i64, i64),
    /// Name, start and end of each piece, in `left arm + insert + right arm` coordinates.
    spans: Vec<(String, i64, i64)>,
    /// Each junction's place along the insert and the temperature it melts at.
    seams: Vec<(i64, f64, String)>,
    /// How many bases go in: what the band's body is a picture of.
    insert_bp: i64,
}

/// A compute in flight.
struct Computing {
    results: Receiver<(usize, Designed)>,
    done: usize,
    total: usize,
}

/// A menu standing open, and what picking an entry means.
enum Picker {
    Enzyme {
        second: bool,
        menu: Menu<String>,
    },
    /// Inserts from the project: a construct, or every construct in a folder.
    Inserts {
        menu: Menu<String>,
    },
}

pub struct Build {
    pub inputs: Inputs,
    inserts: Vec<Insert>,
    designs: Vec<Option<Designed>>,
    phase: Phase,
    preview: usize,
    all_cutters: bool,
    /// The folder's page instead of the console, once a build has landed in one.
    open_folder: Option<String>,
    /// What `read_build_folder` last said about it.
    page: Option<Value>,
    /// The plasmid each of its designs made, in the page's own order, for the map under each row.
    /// Read once with the page: they are ordinary constructs in the folder, not something rebuilt.
    page_products: Vec<Option<Construct>>,
    /// The design being asked about before its row is forgotten.
    confirming: Option<String>,
    note: Option<String>,
    busy: bool,
    computing: Option<Computing>,
    /// What the designs on screen were computed from; anything else means they are stale.
    computed_for: Option<String>,
    picker: Option<Picker>,
    options: Vec<EnzymeOption>,
    /// The plasmid the design makes, for the scrub's "after": which row it belongs to, and it.
    ///
    /// Fetched for ONE row, when it is asked for. A product is as big as a plasmid and a hundred
    /// inserts is a hundred of them; a window that shows one wants exactly one.
    product: Option<(usize, Construct)>,
    /// The backbone those options were worked out for.
    options_for: String,
    /// Where an order sheet is written: beside the project, so it can be found again.
    order_to: PathBuf,
    /// Compute as soon as there is a backbone to compute against (`--build-compute`, for a picture).
    compute_when_ready: bool,
}

impl Default for Build {
    fn default() -> Build {
        Build {
            // The two settings arrive with the numbers a design is made with, so the fields say
            // what is being used rather than standing empty and meaning the same thing.
            inputs: Inputs { target_tm: "65".into(), max_piece: "3000".into(), ..Inputs::default() },
            inserts: Vec::new(),
            designs: Vec::new(),
            phase: Phase::default(),
            preview: 0,
            all_cutters: false,
            open_folder: None,
            page: None,
            page_products: Vec::new(),
            confirming: None,
            note: None,
            busy: false,
            computing: None,
            computed_for: None,
            picker: None,
            options: Vec::new(),
            product: None,
            options_for: String::new(),
            order_to: PathBuf::new(),
            compute_when_ready: false,
        }
    }
}

/// What the place is drawn against: the plasmid in front, and the project around it.
pub struct Ctx<'a> {
    pub backbone: Option<&'a Construct>,
    /// The backbone's path, as a listing shows it.
    pub plasmid: Option<String>,
    pub listing: &'a [Listed],
    pub folders: &'a [PathBuf],
    pub root: &'a Path,
}

impl Build {
    /// The inputs are held against ONE backbone: an enzyme name means a different position on the
    /// next plasmid, and a region means different bases. Carried over, every field would lie.
    pub fn backbone_changed(&mut self) {
        // OPENING A CONSTRUCT LEAVES THE FOLDER'S PAGE: asking for a plasmid is asking to see it.
        let settings = (self.tm_text(), self.piece_text(), std::mem::take(&mut self.order_to));
        *self = Build::default();
        self.order_to = settings.2;
        self.inputs.target_tm = settings.0;
        self.inputs.max_piece = settings.1;
    }

    fn tm_text(&self) -> String {
        if self.inputs.target_tm.is_empty() { "65".to_string() } else { self.inputs.target_tm.clone() }
    }

    /// 3000 is a supplier's eBlock ceiling. It is a vendor's number, not a law, which is why the
    /// console names it beside the field instead of making anyone remember which integer is theirs.
    fn piece_text(&self) -> String {
        if self.inputs.max_piece.is_empty() { "3000".to_string() } else { self.inputs.max_piece.clone() }
    }

    /// FILES IN, BY THE TWO ROADS THE WEB TAKES: a sheet (csv, tsv, txt) is text and goes through
    /// the intake parser; anything else is a sequence file and is read the way a dropped construct
    /// is — the format decided from the bytes, refusals as sentences.
    ///
    /// Dropped on the window or chosen from the dialog, it is the same door.
    pub fn take_files(&mut self, paths: &[PathBuf]) {
        let mut rows: Vec<(String, String)> = Vec::new();
        let mut refused: Vec<String> = Vec::new();
        for path in paths {
            let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            let Ok(bytes) = std::fs::read(path) else {
                refused.push(format!("{name} could not be read"));
                continue;
            };
            if is_sheet_file(&name) {
                match String::from_utf8(bytes) {
                    Ok(text) => rows.extend(parse_insert_text(&text).into_iter().map(|r| (r.name, r.seq))),
                    Err(_) => refused.push(format!("{name} is not text")),
                }
                continue;
            }
            match bios_core::formats::read(&bytes, &path.to_string_lossy()) {
                // A file's own name when the sequence inside it has none: it is what a person
                // called the thing, and a row with no name would be numbered instead.
                Ok(opened) => {
                    let stem = path.file_stem().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                    let named = if opened.construct.name.trim().is_empty() { stem } else { opened.construct.name.clone() };
                    rows.push((named, opened.construct.sequence));
                }
                Err(why) => refused.push(why),
            }
        }
        let had = !rows.is_empty();
        if had {
            self.add_rows(rows);
        }
        if !refused.is_empty() {
            self.note = Some(refused.join(" "));
        } else if !had {
            self.note = Some("Nothing in that could be an insert.".into());
        }
    }

    /// Rows and the phase they put the screen in, from text — what a paste does, for a picture
    /// of the console taken from the command line.
    pub fn seed(&mut self, text: &str, compute: bool) {
        let rows = parse_insert_text(text).into_iter().map(|r| (r.name, r.seq)).collect();
        self.add_rows(rows);
        self.compute_when_ready = compute;
    }

    /// Answer the cut and give the list the screen, as pressing the bar's own button does.
    pub fn fill(&mut self) {
        self.phase = Phase::Fill;
    }

    /// Scrub to what the design makes, as pressing "after" does.
    pub fn show_product(&mut self) {
        self.inputs.stage = Stage::Product;
    }

    /// Show a folder's page rather than the console: the two ends of one object's life.
    pub fn show_folder(&mut self, folder: &str) {
        self.open_folder = Some(folder.to_string());
        self.page = None;
        self.page_products.clear();
    }

    pub fn folder_open(&self) -> Option<&str> {
        self.open_folder.as_deref()
    }

    /// Everything the designs were made from. A change to any of it makes them stale.
    fn signature(&self, backbone: &str) -> String {
        let cut = match self.inputs.cut_mode {
            CutMode::Enzymes => format!("{}|{}|{:?}", self.inputs.enzyme_a, self.inputs.enzyme_b, self.inputs.arc),
            CutMode::Region => format!("{}..{}", self.inputs.region_start, self.inputs.region_end),
        };
        let rows: String = self.inserts.iter().map(|i| format!("{}:{};", i.name, i.bases)).collect();
        format!("{backbone}|{cut}|{rows}|{}|{}", self.tm_text(), self.piece_text())
    }

    /// The parameters that say where the backbone opens, for every command that takes them.
    fn opening_params(&self) -> Option<Value> {
        match self.inputs.cut_mode {
            CutMode::Enzymes => {
                let (a, b) = (self.inputs.enzyme_a.trim(), self.inputs.enzyme_b.trim());
                if a.is_empty() || b.is_empty() || a == b {
                    return None;
                }
                Some(json!({ "enzymes": [a, b], "arc": if self.inputs.arc == Arc::Origin { "origin" } else { "between" } }))
            }
            CutMode::Region => {
                let start: i64 = self.inputs.region_start.parse().ok()?;
                let end: i64 = self.inputs.region_end.parse().ok()?;
                (start > 0 && end > 0).then(|| json!({ "replace_start": start, "replace_end": end }))
            }
        }
    }

    fn cut(&self, backbone: &Construct) -> Option<bios_core::gibson::GibsonCut> {
        match self.inputs.cut_mode {
            CutMode::Region => {
                let start: i64 = self.inputs.region_start.parse().ok()?;
                let end: i64 = self.inputs.region_end.parse().ok()?;
                Some(bios_core::gibson::GibsonCut::Region { start: start - 1, end: end - 1 })
            }
            CutMode::Enzymes => {
                let (a, b) = (self.inputs.enzyme_a.clone(), self.inputs.enzyme_b.clone());
                if a.is_empty() || b.is_empty() || a == b {
                    return None;
                }
                let found = find_cut_sites(&backbone.sequence, &[a.as_str(), b.as_str()], backbone.circular);
                for name in [&a, &b] {
                    if found.iter().filter(|s| s.enzyme.eq_ignore_ascii_case(name)).count() != 1 {
                        return None;
                    }
                }
                let sites = found
                    .iter()
                    .map(|s| bios_core::gibson::GibsonCutSite {
                        enzyme: s.enzyme.to_string(),
                        position: s.top_cut as i64,
                        recognition_start: Some(s.recognition_start as i64),
                    })
                    .collect();
                let arc = if self.inputs.arc == Arc::Origin { bios_core::gibson::OpeningArc::Origin } else { bios_core::gibson::OpeningArc::Between };
                Some(bios_core::gibson::GibsonCut::Enzymes { enzymes: [a, b], sites, arc: Some(arc), keep_sites: true })
            }
        }
    }

    /// Where the backbone opens, as the bar draws it: how much leaves, and what the other arc is.
    fn opening(&self, backbone: &Construct) -> Option<console::Opening> {
        let cut = self.cut(backbone)?;
        let mut warnings = Vec::new();
        let opened =
            bios_core::gibson::resolve_gibson_opening(backbone.len() as i64, backbone.circular, &cut, &mut warnings, bios_core::gibson::SITE_MARGIN)
                .ok()?;
        Some(console::Opening {
            removed: opened.removed_len.max(0) as usize,
            between: opened.arcs.between.max(0) as usize,
            origin: opened.arcs.origin.max(0) as usize,
        })
    }

    /// The enzymes that can open this backbone, worked out once per backbone rather than per frame.
    fn refresh_options(&mut self, backbone: &Construct, key: &str) {
        if self.options_for != key {
            self.options = common_enzyme_options(&backbone.sequence, backbone.circular);
            self.options_for = key.to_string();
            // Open on the first two enzymes that ARE openings, so the place arrives with something
            // on the map rather than an empty pair of pickers. Only ever fills blanks.
            if self.inputs.enzyme_a.is_empty() && self.inputs.enzyme_b.is_empty() {
                let unique: Vec<&EnzymeOption> = self.options.iter().filter(|o| is_opening(o)).collect();
                if let [a, b, ..] = unique.as_slice() {
                    self.inputs.enzyme_a = a.enzyme.clone();
                    self.inputs.enzyme_b = b.enzyme.clone();
                }
            }
        }
    }

    /// THE DESIGN ON SCREEN, DRAWN ON THE BACKBONE IT GOES INTO.
    ///
    /// One band, for the row that is open — never the whole list. The list is ALTERNATIVES: each
    /// row is its own plasmid, so summing them would scale the band's insert to the total of every
    /// row while its segments spanned only one. Nothing would throw; the map would simply lie, and
    /// it would lie the moment you did the thing this place is for.
    ///
    /// One segment per EBLOCK, not per insert: a single 4 kb insert is one fragment and several
    /// pieces, and drawing it as one solid band says nothing about what will be ordered. They are
    /// given spans rather than lengths, and alternate rows, because the pieces OVERLAP.
    fn band(&self, backbone: &Construct) -> Option<OuterBand> {
        let shown = self.designs.get(self.preview).and_then(Option::as_ref).filter(|d| d.ok);
        let insert = self.inserts.get(self.preview);
        let opening = self.cut(backbone).and_then(|cut| {
            let mut warnings = Vec::new();
            bios_core::gibson::resolve_gibson_opening(backbone.len() as i64, backbone.circular, &cut, &mut warnings, bios_core::gibson::SITE_MARGIN)
                .ok()
        })?;
        let (left_arm, right_arm) = shown.map_or((0, 0), |d| d.arms);
        let insert_bp = shown.map_or_else(|| insert.map_or(0, |i| i.bases.len() as i64), |d| d.insert_bp);
        if insert_bp == 0 && shown.is_none() {
            // Before anything is pasted there is a cut and nothing going into it. The wash says
            // what leaves; a band of nothing would be a picture of a design that does not exist.
            return None;
        }
        // EACH ROW OWNS A HUE, and its pieces are that hue onward — so the row in the list, the
        // bar on it and the fragments drawn here are the same colours for the same design.
        let segments = shown
            .map(|d| {
                d.spans
                    .iter()
                    .enumerate()
                    .map(|(i, (name, start, end))| BandSegment {
                        name: name.clone(),
                        from_bp: start - d.arms.0,
                        to_bp: end - d.arms.0,
                        color: hex_of(console::frag_color(self.preview + i)),
                        tier: Some(i % 2),
                    })
                    .collect()
            })
            .unwrap_or_default();
        let seams =
            shown.map(|d| d.seams.iter().map(|(at, tm, label)| BandSeam { at_bp: *at, tm: *tm, label: label.clone() }).collect()).unwrap_or_default();
        Some(OuterBand {
            up: opening.up,
            down: opening.down,
            left_arm_bp: left_arm,
            right_arm_bp: right_arm,
            insert_bp,
            color: hex_of(theme::INSERT),
            arm_color: Some(hex_of(theme::ARM)),
            label: insert.map(|i| i.name.clone()),
            segments,
            seams,
        })
    }

    /// Every site you could open at, not only the two chosen — you cannot choose from a list you
    /// cannot see. Unique cutters by default: an enzyme that cuts eighty times is not an opening,
    /// and drawing all eighty buries the eight that are.
    fn candidate_sites(&self, backbone: &Construct) -> Vec<bios_core::enzymes::CutSite> {
        let names: Vec<&str> = self
            .options
            .iter()
            .filter(|o| self.all_cutters || is_opening(o) || o.enzyme == self.inputs.enzyme_a || o.enzyme == self.inputs.enzyme_b)
            .map(|o| o.enzyme.as_str())
            .collect();
        find_cut_sites(&backbone.sequence, &names, backbone.circular)
    }

    /// Picking a site picks it, or unpicks it if it was already one of the two. Two slots; with
    /// both taken the older is pushed out. Which slot is which does not matter — the opening is
    /// resolved from the two positions and the arc, not from their order.
    fn choose_cut(&mut self, enzyme: &str) {
        if self.inputs.enzyme_a == enzyme {
            self.inputs.enzyme_a.clear();
        } else if self.inputs.enzyme_b == enzyme {
            self.inputs.enzyme_b.clear();
        } else if self.inputs.enzyme_a.is_empty() {
            self.inputs.enzyme_a = enzyme.to_string();
        } else if self.inputs.enzyme_b.is_empty() {
            self.inputs.enzyme_b = enzyme.to_string();
        } else {
            self.inputs.enzyme_b = std::mem::take(&mut self.inputs.enzyme_a);
            self.inputs.enzyme_a = enzyme.to_string();
        }
    }

    /// Rows in. Empty sequences are dropped here, so a row always has bases behind it.
    fn add_rows(&mut self, rows: Vec<(String, String)>) {
        let before = self.inserts.len();
        for (name, bases) in rows {
            let bases: String = bases.chars().filter(char::is_ascii_alphabetic).collect::<String>().to_uppercase();
            if !bases.is_empty() {
                self.inserts.push(Insert { name, bases });
            }
        }
        self.note = (self.inserts.len() == before).then(|| "Nothing in that reads as a sequence.".to_string());
        self.designs.clear();
        self.computed_for = None;
        self.product = None;
    }

    /// The plasmid the previewed design makes, read through the engine the one time it is needed.
    ///
    /// It is a design, not a saved construct: nothing has been built yet, so the only place it
    /// exists is in the answer to `design_builds`, which carries it when asked.
    fn fetch_product(&mut self, session: &Session, ctx: &Ctx) {
        if self.product.as_ref().is_some_and(|(row, _)| *row == self.preview) {
            return;
        }
        let (Some(plasmid), Some(opening), Some(insert)) = (ctx.plasmid.clone(), self.opening_params(), self.inserts.get(self.preview).cloned())
        else {
            return;
        };
        if !self.designs.get(self.preview).and_then(Option::as_ref).is_some_and(|d| d.ok) {
            return;
        }
        let mut params = opening;
        let Some(map) = params.as_object_mut() else { return };
        map.insert("plasmid".into(), json!(plasmid));
        map.insert("inserts".into(), json!([{ "name": insert.name, "sequence": insert.bases }]));
        map.insert("with_product".into(), json!(true));
        if let Ok(tm) = self.tm_text().parse::<i64>() {
            map.insert("target_tm".into(), json!(tm));
        }
        if let Ok(cap) = self.piece_text().parse::<i64>() {
            map.insert("max_piece_length".into(), json!(cap));
        }
        match session.execute("design_builds", &params) {
            Ok(out) => {
                let name = self.product_name();
                if let Some(product) = read_product(&out["designs"][0]["product"], &name) {
                    self.product = Some((self.preview, product));
                }
            }
            Err(why) => self.note = Some(why.0),
        }
    }

    /// What the previewed product is called: the insert's name on the backbone's.
    fn product_name(&self) -> String {
        let stem = self.options_for.rsplit('/').next().unwrap_or_default().to_string();
        let part = self.inserts.get(self.preview).map(|i| i.name.trim().replace(' ', "")).unwrap_or_default();
        if part.is_empty() { format!("{stem}_{}", self.preview + 1) } else { format!("{stem}_{part}") }
    }

    /// The rows as the console draws them.
    fn rows(&self) -> Vec<Row> {
        self.inserts
            .iter()
            .enumerate()
            .map(|(i, insert)| {
                let design = self.designs.get(i).and_then(Option::as_ref);
                Row {
                    name: insert.name.clone(),
                    bases: insert.bases.clone(),
                    chip: design.map(|d| d.chip.clone()).unwrap_or_default(),
                    warn: design.is_some_and(|d| d.warn),
                    pieces: design.map(|d| d.pieces.clone()).unwrap_or_default(),
                    arms: (0, 0),
                    order_line: design.map(|d| d.order_line.clone()).unwrap_or_default(),
                }
            })
            .collect()
    }

    /// The order the rows are drawn in. Once computed, the ones with a problem come first, so at a
    /// hundred rows nobody scrolls to find the three that need them. Stable inside each half.
    fn order(&self) -> Vec<usize> {
        let mut idx: Vec<usize> = (0..self.inserts.len()).collect();
        if self.designs.iter().flatten().count() == 0 {
            return idx;
        }
        idx.sort_by_key(|i| {
            let design = self.designs.get(*i).and_then(Option::as_ref);
            let bad = design.is_none_or(|d| !d.ok || d.warn);
            (!bad, *i)
        });
        idx
    }

    fn picks(&self) -> Vec<EnzymePick> {
        self.options.iter().map(|o| EnzymePick { enzyme: o.enzyme.clone(), count: o.count }).collect()
    }

    /// The overview along the bottom: what Compute made, summed, for reading before committing.
    fn summary(&self, key: &str) -> String {
        if self.inserts.is_empty() {
            return "Paste the sequences to put in, or take them from the project.".to_string();
        }
        if self.computed_for.as_deref() != Some(key) {
            let n = self.inserts.len();
            let changed = if self.designs.is_empty() { "" } else { " · inputs changed, recompute" };
            return format!("{n} insert{}{changed}", if n == 1 { "" } else { "s" });
        }
        let made: Vec<&Designed> = self.designs.iter().flatten().collect();
        let buildable = made.iter().filter(|d| d.ok).count();
        let pieces: usize = made.iter().filter(|d| d.ok).map(|d| d.pieces.len()).sum();
        let longest = made.iter().flat_map(|d| d.pieces.iter().copied()).max().unwrap_or(0);
        let warned = made.iter().filter(|d| d.ok && d.warn).count();
        let failed = made.len() - buildable;
        let mut said =
            format!("{buildable} {} · {pieces} {}", if buildable == 1 { "design" } else { "designs" }, if pieces == 1 { "piece" } else { "pieces" });
        if longest > 0 {
            said.push_str(&format!(" · longest {longest} bp"));
        }
        if warned > 0 {
            said.push_str(&format!(" · {warned} to check"));
        }
        if failed > 0 {
            said.push_str(&format!(" · {failed} could not be designed"));
        }
        said
    }

    // ── COMPUTE, OFF THE DRAWING THREAD ─────────────────────────────────────────────────────

    fn compute(&mut self, ctx: &Ctx) {
        let (Some(plasmid), Some(opening)) = (ctx.plasmid.clone(), self.opening_params()) else {
            self.note = Some("Pick two sites on the backbone first.".into());
            return;
        };
        if self.inserts.is_empty() || self.computing.is_some() {
            return;
        }
        self.note = None;
        self.preview = 0;
        self.designs = vec![None; self.inserts.len()];
        self.computed_for = Some(self.signature(&plasmid));
        self.product = None;
        let (send, results) = channel();
        let (root, inserts) = (ctx.root.to_path_buf(), self.inserts.clone());
        let (tm, cap) = (self.tm_text(), self.piece_text());
        self.computing = Some(Computing { results, done: 0, total: inserts.len() });
        // Its own session on the same project: a read command, so nothing is written and nothing
        // can be half-done if the window is closed while it runs.
        std::thread::spawn(move || {
            let Ok(project) = Project::open(&root) else {
                return;
            };
            let session = Session::new(project, Who { actor: Actor::Human, source: "window".into(), session: None });
            for (i, insert) in inserts.iter().enumerate() {
                let mut params = opening.clone();
                if let Some(map) = params.as_object_mut() {
                    map.insert("plasmid".into(), json!(plasmid));
                    map.insert("inserts".into(), json!([{ "name": insert.name, "sequence": insert.bases }]));
                    if let Ok(tm) = tm.parse::<i64>() {
                        map.insert("target_tm".into(), json!(tm));
                    }
                    if let Ok(cap) = cap.parse::<i64>() {
                        map.insert("max_piece_length".into(), json!(cap));
                    }
                }
                let designed = match session.execute("design_builds", &params) {
                    Ok(value) => read_design(&value["designs"][0]),
                    Err(why) => Designed { chip: why.0, warn: true, ..Designed::default() },
                };
                if send.send((i, designed)).is_err() {
                    return;
                }
            }
        });
    }

    /// The designs that have landed since the last frame.
    fn take_results(&mut self) {
        let Some(running) = &mut self.computing else {
            return;
        };
        let mut finished = false;
        loop {
            match running.results.try_recv() {
                Ok((i, designed)) => {
                    if let Some(slot) = self.designs.get_mut(i) {
                        *slot = Some(designed);
                    }
                    running.done += 1;
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => break,
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    finished = true;
                    break;
                }
            }
        }
        if finished || running.done >= running.total {
            self.computing = None;
        }
    }

    // ── WHAT A PRESS DOES ───────────────────────────────────────────────────────────────────

    fn act(&mut self, action: Action, session: &Session, ctx: &Ctx) -> Vec<Request> {
        match action {
            Action::PickEnzyme { second, enzyme } => {
                if second {
                    self.inputs.enzyme_b = enzyme;
                } else {
                    self.inputs.enzyme_a = enzyme;
                }
            }
            Action::OpenPicker { second, at } => {
                let items =
                    console::enzyme_items(&self.picks(), self.all_cutters, if second { &self.inputs.enzyme_b } else { &self.inputs.enzyme_a });
                self.picker = Some(Picker::Enzyme { second, menu: Menu::new(at, vec![items]) });
            }
            Action::SwapArc => self.inputs.arc = if self.inputs.arc == Arc::Between { Arc::Origin } else { Arc::Between },
            Action::SetStage(stage) => self.inputs.stage = stage,
            Action::AllCutters(all) => self.all_cutters = all,
            Action::SetPhase(phase) => self.phase = phase,
            Action::Take(text) => {
                let rows = parse_insert_text(&text).into_iter().map(|r| (r.name, r.seq)).collect();
                self.add_rows(rows);
            }
            Action::AddConstruct(at) => {
                let items: Vec<Item<String>> = ctx
                    .listing
                    .iter()
                    .map(|l| Item::new(format!("{} · {} bp", crate::label(&l.path), l.length), format!("plasmid:{}", l.id.as_str())))
                    .collect();
                self.picker = Some(Picker::Inserts { menu: Menu::new(at, vec![items]) });
            }
            Action::AddFolder(at) => {
                // A folder of forty-six promoters becomes forty-six rows in one gesture.
                let items: Vec<Item<String>> = ctx
                    .folders
                    .iter()
                    .map(|f| {
                        let name = crate::label(f);
                        let held = ctx.listing.iter().filter(|l| l.path.parent().is_some_and(|p| p == f.as_path())).count();
                        Item::new(format!("{name} · {held} construct{}", if held == 1 { "" } else { "s" }), format!("folder:{name}"))
                    })
                    .collect();
                self.picker = Some(Picker::Inserts { menu: Menu::new(at, vec![items]) });
            }
            Action::Upload => return vec![Request::AskForFiles],
            Action::ClearRows => {
                self.inserts.clear();
                self.designs.clear();
                self.computed_for = None;
            }
            Action::RemoveRow(i) => {
                if i < self.inserts.len() {
                    self.inserts.remove(i);
                    self.designs.clear();
                    self.computed_for = None;
                }
            }
            Action::OpenRow(i) => self.preview = i,
            Action::Compute => self.compute(ctx),
            Action::CopyPieces => {
                let fasta: Vec<String> = self.designs.iter().flatten().filter(|d| d.ok).map(|d| d.fasta.clone()).collect();
                return vec![Request::Copy(fasta.join("\n"))];
            }
            Action::Build => return self.build(session, ctx),
            Action::Edited => {}
        }
        Vec::new()
    }

    /// THE ONE ACT: every design becomes a plasmid, and they all land in one folder.
    fn build(&mut self, session: &Session, ctx: &Ctx) -> Vec<Request> {
        let (Some(plasmid), Some(opening)) = (ctx.plasmid.clone(), self.opening_params()) else {
            return vec![Request::Say("Pick two sites on the backbone first.".into())];
        };
        let mut params = opening;
        let Some(map) = params.as_object_mut() else {
            return Vec::new();
        };
        map.insert("plasmid".into(), json!(plasmid));
        map.insert("inserts".into(), json!(self.inserts.iter().map(|i| json!({ "name": i.name, "sequence": i.bases })).collect::<Vec<_>>()));
        if let Ok(tm) = self.tm_text().parse::<i64>() {
            map.insert("target_tm".into(), json!(tm));
        }
        if let Ok(cap) = self.piece_text().parse::<i64>() {
            map.insert("max_piece_length".into(), json!(cap));
        }
        // The name field only names something when there is one thing.
        if self.inserts.len() == 1 && !self.inputs.name.trim().is_empty() {
            map.insert("names".into(), json!([self.inputs.name.trim()]));
        }
        self.busy = true;
        let out = session.execute("build_plasmids", &params);
        self.busy = false;
        match out {
            // YOU LAND IN THE FOLDER. The map shows what you made and the console becomes the
            // folder's page.
            Ok(value) => {
                let folder = value["folder"].as_str().unwrap_or_default().to_string();
                self.show_folder(&folder);
                vec![Request::Refresh]
            }
            Err(why) => {
                self.note = Some(why.0.clone());
                vec![Request::Say(why.0)]
            }
        }
    }

    /// The menu standing open, and what was picked from it.
    fn picked(&mut self, ui: &mut Ui, ctx: &Ctx) -> Option<Action> {
        let picker = self.picker.as_mut()?;
        let (shown, second) = match picker {
            Picker::Enzyme { second, menu } => (menu.show(ui), Some(*second)),
            Picker::Inserts { menu } => (menu.show(ui), None),
        };
        match shown {
            Shown::StillOpen => None,
            Shown::Closed => {
                self.picker = None;
                None
            }
            Shown::Picked(what) => {
                self.picker = None;
                match second {
                    Some(second) => Some(Action::PickEnzyme { second, enzyme: what }),
                    None => {
                        self.take_from_project(&what, ctx);
                        None
                    }
                }
            }
        }
    }

    /// A construct, or every construct in a folder, as rows.
    fn take_from_project(&mut self, what: &str, ctx: &Ctx) {
        let bases_of = |listed: &Listed| -> Option<(String, String)> {
            let project = Project::open(ctx.root).ok()?;
            let loaded = project.read(&listed.id).ok()?;
            Some((loaded.construct.name.clone(), loaded.construct.sequence.clone()))
        };
        if let Some(id) = what.strip_prefix("plasmid:") {
            let rows = ctx.listing.iter().filter(|l| l.id.as_str() == id).filter_map(bases_of).collect();
            self.add_rows(rows);
        } else if let Some(folder) = what.strip_prefix("folder:") {
            let rows = ctx.listing.iter().filter(|l| l.path.parent().map(crate::label).as_deref() == Some(folder)).filter_map(bases_of).collect();
            self.add_rows(rows);
        }
    }

    // ── DRAWING ─────────────────────────────────────────────────────────────────────────────

    /// Draws the place and runs what was pressed. Everything it cannot do itself comes back as a
    /// [`Request`] for the window.
    pub fn show(&mut self, ui: &mut Ui, rect: Rect, session: &Session, ctx: &Ctx) -> Vec<Request> {
        self.take_results();
        if self.open_folder.is_some() {
            return self.folder(ui, rect, session);
        }
        let Some(backbone) = ctx.backbone else {
            ui.painter().rect_filled(rect, 0.0, theme::CELL);
            let said = "Open a plasmid to build from: Build puts new DNA into the one in front.";
            ui.painter().text(rect.center(), Align2::CENTER_CENTER, said, theme::sans(13.0), theme::MUTED);
            return Vec::new();
        };
        let key = ctx.plasmid.clone().unwrap_or_default();
        self.refresh_options(backbone, &key);
        if std::mem::take(&mut self.compute_when_ready) {
            self.compute(ctx);
        }

        // The map owns the pane while the cut is the question, and is a strip once it is answered.
        let tall = (rect.height() - 260.0).max(180.0);
        // The strip still has to hold the band, which floats above the axis: a strip that cuts
        // it off answers the question "what is going in?" with a picture of the ruler.
        let map_h = if self.phase == Phase::Cut { tall } else { 164.0 };
        let map_rect = Rect::from_min_size(rect.min, vec2(rect.width(), map_h));
        ui.painter().rect_filled(map_rect, 0.0, theme::CANVAS);
        if self.inputs.stage == Stage::Product {
            self.fetch_product(session, ctx);
        }
        // AFTER: the plasmid this makes, drawn as itself. No band and no wash — on the product
        // there is nothing going in and nothing leaving; it is already what it will be.
        let after = (self.inputs.stage == Stage::Product).then(|| self.product.as_ref().filter(|(row, _)| *row == self.preview)).flatten();
        let drawn = after.map_or(backbone, |(_, product)| product);
        let sites = if after.is_some() { Vec::new() } else { self.candidate_sites(backbone) };
        let mut view = map::MapView::whole(drawn);
        let bands: Vec<OuterBand> = if after.is_some() { Vec::new() } else { self.band(backbone).into_iter().collect() };
        let dim = after.is_none().then(|| self.opening(backbone)).flatten().and_then(|_| self.cut(backbone)).and_then(|cut| {
            let mut warnings = Vec::new();
            bios_core::gibson::resolve_gibson_opening(backbone.len() as i64, backbone.circular, &cut, &mut warnings, bios_core::gibson::SITE_MARGIN)
                .ok()
                .filter(|o| o.removed_len > 0)
                .map(|o| (o.up, o.down))
        });
        let overlay = map::Overlay { bands: &bands, dim };
        let gesture = map::show_overlay(ui, map_rect, drawn, &map::MapCursor::default(), &mut view, &sites, view_tab::Layers::default(), overlay);
        // A press by a site on the map is that site being picked: the one screen for deciding
        // where to cut must let you point at it.
        if let Some(map::MapGesture::Press { gap, .. }) = gesture
            && self.inputs.cut_mode == CutMode::Enzymes
            && after.is_none()
        {
            let reach = (backbone.len() / 50).max(25);
            if let Some(site) = sites.iter().min_by_key(|s| s.top_cut.abs_diff(gap)).filter(|s| s.top_cut.abs_diff(gap) <= reach) {
                let enzyme = site.enzyme.to_string();
                self.choose_cut(&enzyme);
            }
        }

        let (rows, order, picks, note) = (self.rows(), self.order(), self.picks(), self.note.clone());
        let model = console::Model {
            rows: &rows,
            order: &order,
            options: &picks,
            opening: self.opening(backbone),
            phase: self.phase,
            all_cutters: self.all_cutters,
            progress: self.computing.as_ref().map(|c| (c.done, c.total)),
            stale: self.computed_for.as_deref() != Some(self.signature(&key).as_str()),
            preview: self.preview,
            designed: self.designs.get(self.preview).and_then(Option::as_ref).is_some_and(|d| d.ok),
            summary: self.summary(&self.signature(&key)),
            buildable: self.designs.iter().flatten().filter(|d| d.ok).count(),
            busy: self.busy,
            note: note.as_deref(),
        };
        let mut action = console::cut_bar(ui, map_rect, &model, &mut self.inputs);
        let console_rect = Rect::from_min_max(pos2(rect.left(), map_rect.bottom()), rect.max);
        if let Some(from_console) = console::console(ui, console_rect, &model, &mut self.inputs) {
            action = Some(from_console);
        }

        let mut requests = Vec::new();
        if let Some(picked) = self.picked(ui, ctx) {
            requests.extend(self.act(picked, session, ctx));
        }
        if let Some(action) = action {
            requests.extend(self.act(action, session, ctx));
        }
        requests
    }

    /// The folder's page: what is on disk about this build, and the verbs on it.
    fn folder(&mut self, ui: &mut Ui, rect: Rect, session: &Session) -> Vec<Request> {
        let Some(folder) = self.open_folder.clone() else {
            return Vec::new();
        };
        if self.page.is_none() {
            match session.execute("read_build_folder", &json!({ "folder": folder })) {
                Ok(page) => {
                    self.page_products = page["designs"]
                        .as_array()
                        .map(|rows| {
                            rows.iter()
                                .map(|d| {
                                    let id = d["plasmid"].as_str().and_then(bios_project::ConstructId::parse)?;
                                    session.project().read(&id).ok().map(|loaded| loaded.construct)
                                })
                                .collect()
                        })
                        .unwrap_or_default();
                    self.page = Some(page);
                }
                Err(why) => {
                    self.open_folder = None;
                    return vec![Request::Say(why.0)];
                }
            }
        }
        let Some(page) = self.page.clone() else {
            return Vec::new();
        };
        let designs: Vec<DesignRow> = page["designs"]
            .as_array()
            .map(|rows| {
                rows.iter()
                    .enumerate()
                    .map(|(i, d)| DesignRow {
                        product: self.page_products.get(i).and_then(Option::as_ref),
                        name: d["name"].as_str().unwrap_or_default().to_string(),
                        bp: d["length"].as_u64().unwrap_or_default() as usize,
                        recipe: d["recipe"]
                            .as_array()
                            .map(|ns| ns.iter().filter_map(|n| n.as_u64().map(|n| n as usize)).collect())
                            .unwrap_or_default(),
                        warnings: d["warnings"].as_array().map(Vec::len).unwrap_or_default(),
                        note: d["note"].as_str().unwrap_or_default().to_string(),
                    })
                    .collect()
            })
            .unwrap_or_default();
        let pieces: Vec<PieceRow> = page["to_order"]
            .as_array()
            .map(|rows| {
                rows.iter()
                    .map(|p| PieceRow {
                        n: p["n"].as_u64().unwrap_or_default() as usize,
                        name: p["name"].as_str().unwrap_or_default().to_string(),
                        length: p["length"].as_u64().unwrap_or_default() as usize,
                        sequence: p["sequence"].as_str().unwrap_or_default().to_string(),
                    })
                    .collect()
            })
            .unwrap_or_default();
        let clipped: Vec<String> =
            page["cut_clips"].as_array().map(|xs| xs.iter().filter_map(|x| x.as_str().map(str::to_string)).collect()).unwrap_or_default();
        let backbone = page["backbone"].as_str().map(|name| (name, page["backbone_length"].as_u64().unwrap_or_default() as usize));
        let digest = page["digest"].as_object().and_then(|d| Some((d.get("comes_out")?.as_u64()? as usize, d.get("stays")?.as_u64()? as usize)));
        let model = folder_page::Model {
            folder: &folder,
            backbone,
            opening: page["opening"].as_str().unwrap_or_default(),
            digest,
            clipped: &clipped,
            designs: &designs,
            pieces: &pieces,
            confirming: self.confirming.as_deref(),
            note: self.note.as_deref(),
        };
        let action = folder_page::show(ui, rect, &model);
        self.folder_act(action, session, &folder, &pieces)
    }

    fn folder_act(&mut self, action: Option<folder_page::Action>, session: &Session, folder: &str, pieces: &[PieceRow]) -> Vec<Request> {
        let Some(page) = self.page.clone() else {
            return Vec::new();
        };
        match action {
            Some(folder_page::Action::OpenBackbone) | Some(folder_page::Action::BuildMore) => {
                // BUILD MORE GOES INTO THE SAME BACKBONE, opened the same way: the folder's copy
                // is opened and the cut it uses comes back with it.
                let backbone = page["backbone"].as_str().unwrap_or_default().to_string();
                self.open_folder = None;
                self.page = None;
                self.page_products.clear();
                vec![Request::Open(backbone)]
            }
            Some(folder_page::Action::OpenDesign(name)) => {
                self.open_folder = None;
                self.page = None;
                self.page_products.clear();
                vec![Request::Open(format!("{folder}/{name}"))]
            }
            Some(folder_page::Action::ConfirmDrop(name)) => {
                self.confirming = Some(name);
                Vec::new()
            }
            Some(folder_page::Action::KeepDesign) => {
                self.confirming = None;
                Vec::new()
            }
            Some(folder_page::Action::DropDesign(name)) => {
                self.confirming = None;
                match session.execute("drop_design", &json!({ "folder": folder, "name": name })) {
                    Ok(_) => {
                        self.page = None;
                        self.page_products.clear();
                        vec![Request::Refresh]
                    }
                    Err(why) => vec![Request::Say(why.0)],
                }
            }
            Some(folder_page::Action::CopyOrder) => match session.execute("build_order_table", &json!({ "folder": folder })) {
                Ok(out) => vec![Request::Copy(out["text"].as_str().unwrap_or_default().to_string())],
                Err(why) => vec![Request::Say(why.0)],
            },
            Some(folder_page::Action::SaveOrder) => {
                // Beside the project, under the folder's own name: an order sheet is something you
                // take somewhere, so it is written where it can be found again.
                let file = self.order_to.join(format!("{folder} order.xlsx"));
                let params = json!({ "folder": folder, "file": file.display().to_string(), "overwrite": true });
                match session.execute("build_order_table", &params) {
                    Ok(_) => vec![Request::Say(format!("{} pieces written to {}", pieces.len(), file.display()))],
                    Err(why) => vec![Request::Say(why.0)],
                }
            }
            None => Vec::new(),
        }
    }
}

/// What `design_builds` said about one insert, as a row.
fn read_design(value: &Value) -> Designed {
    if let Some(why) = value["could_not_design"].as_str() {
        return Designed { chip: why.to_string(), warn: true, ..Designed::default() };
    }
    let order = value["order"].as_array().cloned().unwrap_or_default();
    let pieces: Vec<usize> = order.iter().map(|p| p["length"].as_u64().unwrap_or_default() as usize).collect();
    let warnings = value["warnings"].as_array().map(Vec::len).unwrap_or_default();
    let chip = {
        let n = pieces.len();
        let mut said = format!("{n} {}", if n == 1 { "piece" } else { "pieces" });
        if warnings > 0 {
            said.push_str(&format!(" · {warnings} warning{}", if warnings == 1 { "" } else { "s" }));
        }
        said
    };
    // WHAT A DESIGN COSTS TO ORDER, in one line: the pieces, and the temperatures their overlaps
    // melt at. These are the numbers a person checks before spending money.
    let tms: Vec<f64> = value["overlaps"].as_array().map(|xs| xs.iter().filter_map(|j| j["tm"].as_f64()).collect()).unwrap_or_default();
    let sizes: Vec<String> = pieces.iter().map(usize::to_string).collect();
    let mut order_line = format!("{} bp", sizes.join(" · "));
    if let (Some(lo), Some(hi)) = (tms.iter().cloned().reduce(f64::min), tms.iter().cloned().reduce(f64::max)) {
        order_line.push_str(&if hi - lo < 0.5 { format!(" · at {lo:.0} °C") } else { format!(" · at {lo:.0}–{hi:.0} °C") });
    }
    let fasta: Vec<String> =
        order.iter().map(|p| format!(">{}\n{}", p["name"].as_str().unwrap_or_default(), p["sequence"].as_str().unwrap_or_default())).collect();
    let arms = (value["arms"]["left"].as_i64().unwrap_or_default(), value["arms"]["right"].as_i64().unwrap_or_default());
    let spans: Vec<(String, i64, i64)> = order
        .iter()
        .map(|p| (p["name"].as_str().unwrap_or_default().to_string(), p["start"].as_i64().unwrap_or_default(), p["end"].as_i64().unwrap_or_default()))
        .collect();
    // The insert is what the pieces hold between the two arms.
    let laid = spans.iter().map(|(_, _, end)| *end).max().unwrap_or_default();
    let insert_bp = (laid - arms.0 - arms.1).max(0);
    // EVERY JUNCTION, PLACED BY DISTANCE ALONG THE INSERT — which puts the two outer homology arms
    // at 0 and at the end, with no special case. A piece split into several eBlocks adds its own
    // seams where each piece after the first begins.
    let mut seams: Vec<(i64, f64, String)> = Vec::new();
    for j in value["overlaps"].as_array().cloned().unwrap_or_default() {
        let tm = j["tm"].as_f64().unwrap_or_default();
        let at = match j["kind"].as_str() {
            Some("plasmid to insert") => 0,
            Some("insert to plasmid") => insert_bp,
            // An insert-to-insert junction begins where the piece after it begins.
            _ => spans.get(seams.len()).map_or(0, |(_, start, _)| start - arms.0),
        };
        seams.push((at, tm, j["between"].as_str().unwrap_or_default().to_string()));
    }
    for (i, (name, start, _)) in spans.iter().enumerate().skip(1) {
        let at = start - arms.0;
        if !seams.iter().any(|(there, _, _)| *there == at) {
            let tm = value["overlaps"][i]["tm"].as_f64().unwrap_or_default();
            seams.push((at, tm, format!("to {name}")));
        }
    }
    seams.sort_by_key(|(at, _, _)| *at);
    // A feature the CUT clips is one fact about the cut, said once on the folder's page; it is
    // not a problem on every row, which is how summing it read as nine problems in the web.
    Designed { pieces, chip, warn: warnings > 0, order_line, fasta: fasta.join("\n"), ok: true, arms, spans, seams, insert_bp }
}

/// A colour as the band records it. `bios-layout` is numbers only and knows no palette, so a band
/// carries its colours as text and the map turns them back.
fn hex_of(color: egui::Color32) -> String {
    format!("#{:02x}{:02x}{:02x}", color.r(), color.g(), color.b())
}

/// The product as the map draws it, from what `design_builds` reported.
fn read_product(value: &Value, name: &str) -> Option<Construct> {
    let sequence = value["sequence"].as_str()?.to_string();
    let features = value["features"]
        .as_array()?
        .iter()
        .filter_map(|f| {
            // 1-based and inclusive on the wire, as a person counts them; 0-based in the model.
            let start = usize::try_from(f["start"].as_i64()?.max(1) - 1).ok()?;
            let end = usize::try_from(f["end"].as_i64()?.max(1) - 1).ok()?;
            Some(bios_core::Feature {
                name: f["name"].as_str().unwrap_or_default().to_string(),
                kind: f["type"].as_str().unwrap_or("misc_feature").to_string(),
                start,
                end,
                strand: if f["strand"].as_str() == Some("reverse") { bios_core::Strand::Reverse } else { bios_core::Strand::Forward },
                segments: vec![bios_core::Segment::new(start, end)],
                notes: Vec::new(),
                start_fuzzy: None,
                end_fuzzy: None,
                location_operator: None,
                location_unsupported: None,
            })
        })
        .collect();
    Some(Construct { name: name.to_string(), circular: value["circular"].as_bool().unwrap_or(true), sequence, features, ..Construct::default() })
}
