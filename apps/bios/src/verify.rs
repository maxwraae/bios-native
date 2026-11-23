//! The Verify place in the window: its panel, the front door, a verification's board and one
//! alignment, with every change made through the engine's Verify commands. Long runs go off the
//! window's thread, through `Session::execute_with`, and report back as they go.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use bios_core::Construct;
use bios_core::reads::fastq::is_reads_file;
use bios_core::verify::folder::{Call, FolderMeta, RunSummary, run_stale};
use bios_core::verify::mapper::{self, MapperOutput, Reading, reference_columns};
use bios_core::verify::names::{run_label, sibling_labels, unplaced_hint};
use bios_core::verify::natural_cmp;
use bios_core::verify::runs::{call_order, first_of_map, next_to_call, run_note, run_tone};
use bios_engine::Session;
use bios_engine::verify::{Control, default_mapper, load_run, verifications};
use bios_project::{Actor, ConstructId, Project, Who};
use bios_ui::verify_align::{AlignAction, AlignModel, AlignView, ColumnFeature};
use bios_ui::verify_board::{Board, BoardAction, BoardModel, MapRow, Stored, Waiting};
use bios_ui::verify_front::{Front, FrontAction};
use bios_ui::verify_nav::{NavChosen, NavFolder, NavMap, NavOpen, NavRun, VerifyNav};
use craft_dialogs::FileDialog;
use egui::Rect;
use serde_json::{Value, json};

use super::{App, Then};

/// What the work column shows in Verify.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Screen {
    Front,
    Board(String),
    Alignment { folder: String, name: String, map: Option<String> },
}

/// What a file dialog opened from Verify is for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Pick {
    /// Reads or maps into a verification (a new one from the front door when `folder` is `None`),
    /// onto `map` when given.
    Files { folder: Option<String>, map: Option<String> },
    /// One of an alignment's files, saved where the person chose.
    Download { folder: String, name: String, map: Option<String>, what: &'static str },
}

/// A run going on off the window's thread.
struct Worker {
    folder: String,
    cancel: Arc<AtomicBool>,
    progress: Arc<Mutex<String>>,
    handle: JoinHandle<Result<Value, String>>,
    /// The alignment to open again when it is done (Run again).
    reopen: Option<Screen>,
}

/// An alignment read back, with what the view needs from it.
struct Loaded {
    key: (String, String, Option<String>),
    output: MapperOutput,
    reading: Reading,
    features: Vec<ColumnFeature>,
}

pub struct Verify {
    pub screen: Screen,
    front: Front,
    board: Board,
    nav: VerifyNav,
    align: AlignView,
    /// Every verification and its record, as last read.
    snapshot: Vec<(String, FolderMeta)>,
    /// The maps in them, by id.
    maps: HashMap<String, Construct>,
    worker: Option<Worker>,
    busy: Option<String>,
    error: Option<String>,
    copied_at: Option<Instant>,
    /// The placements the last "take every sample off" undid, to put back.
    undo: Option<(String, Vec<(String, String)>)>,
    loaded: Option<Loaded>,
    /// Calls made here that the record may not show yet.
    called_here: HashSet<String>,
    refreshed: Instant,
}

impl Default for Verify {
    fn default() -> Self {
        Verify {
            screen: Screen::Front,
            front: Front::new(bios_project::folder_stamp()),
            board: Board::default(),
            nav: VerifyNav::default(),
            align: AlignView::default(),
            snapshot: Vec::new(),
            maps: HashMap::new(),
            worker: None,
            busy: None,
            error: None,
            copied_at: None,
            undo: None,
            loaded: None,
            called_here: HashSet::new(),
            refreshed: Instant::now(),
        }
    }
}

impl Verify {
    fn meta(&self, folder: &str) -> Option<&FolderMeta> {
        self.snapshot.iter().find(|(f, _)| f == folder).map(|(_, m)| m)
    }

    fn map_name(&self, id: &str) -> Option<String> {
        self.maps.get(id).map(|c| c.name.clone())
    }
}

impl App {
    /// `--verify-open`: a verification's board, or `folder::sample` for one of its alignments.
    pub(crate) fn verify_open(&mut self, what: &str) {
        self.verify_refresh();
        let (what, step) = match what.rsplit_once('@') {
            Some((w, n)) if n.parse::<usize>().is_ok() => (w, n.parse::<usize>().ok()),
            _ => (what, None),
        };
        if let Some(n) = step {
            self.verify.align.land_on(n.saturating_sub(1));
        }
        self.verify.screen = match what.split_once("::") {
            Some((folder, name)) => Screen::Alignment { folder: folder.into(), name: name.into(), map: None },
            None => Screen::Board(what.into()),
        };
    }

    /// Reads every verification again, with its maps.
    pub(crate) fn verify_refresh(&mut self) {
        let Some(session) = &self.session else {
            self.verify.snapshot.clear();
            return;
        };
        let project = session.project();
        self.verify.snapshot = verifications(project);
        let mut maps = HashMap::new();
        let listed = project.list().unwrap_or_default();
        for (_, meta) in &self.verify.snapshot {
            for m in meta.maps() {
                let Some(id) = ConstructId::parse(&m.construct_id) else { continue };
                if listed.iter().any(|l| l.id == id)
                    && let Ok(loaded) = project.read(&id)
                {
                    maps.insert(m.construct_id.clone(), loaded.construct);
                }
            }
        }
        self.verify.maps = maps;
        self.verify.refreshed = Instant::now();
        // A folder renamed or deleted elsewhere takes its screen with it.
        let gone = match &self.verify.screen {
            Screen::Board(f) | Screen::Alignment { folder: f, .. } => self.verify.meta(f).is_none(),
            Screen::Front => false,
        };
        if gone {
            self.verify.screen = Screen::Front;
        }
    }

    /// Runs a Verify command, saying a failure where the screen shows it, and reads the result back.
    fn vrun(&mut self, command: &str, params: Value) -> Option<Value> {
        let result = self.session.as_ref()?.execute(command, &params);
        let out = match result {
            Ok(v) => {
                self.verify.error = None;
                Some(v)
            }
            Err(e) => {
                self.verify.error = Some(e.0);
                None
            }
        };
        self.refresh();
        out
    }

    pub(crate) fn verify_panel(&mut self, ui: &mut egui::Ui, rect: Rect) {
        let folders: Vec<NavFolder> = self
            .verify
            .snapshot
            .iter()
            .map(|(folder, meta)| NavFolder {
                folder: folder.clone(),
                maps: meta
                    .maps()
                    .iter()
                    .filter_map(|m| {
                        let c = self.verify.maps.get(&m.construct_id)?;
                        let mut runs: Vec<&RunSummary> =
                            meta.runs.iter().filter(|r| meta.map_id_of(r).as_deref() == Some(m.construct_id.as_str())).collect();
                        runs.sort_by(|a, b| natural_cmp(&a.name, &b.name));
                        let labels = sibling_labels(&runs.iter().map(|r| r.name.clone()).collect::<Vec<_>>());
                        Some(NavMap {
                            id: m.construct_id.clone(),
                            name: c.name.clone(),
                            circular: c.circular,
                            runs: runs
                                .iter()
                                .map(|r| NavRun {
                                    name: r.name.clone(),
                                    label: labels.get(&r.name).cloned().unwrap_or_else(|| run_label(&r.name)),
                                    tone: run_tone(r),
                                    note: run_note(r),
                                    stale: run_stale(r.ref_length, r.ref_hash.as_deref(), Some(&c.sequence)),
                                })
                                .collect(),
                        })
                    })
                    .collect(),
            })
            .collect();
        let open = match &self.verify.screen {
            Screen::Front => NavOpen::Nothing,
            Screen::Board(f) => NavOpen::Folder(f.clone()),
            Screen::Alignment { folder, name, map } => NavOpen::Run { folder: folder.clone(), name: name.clone(), map: map.clone() },
        };
        for chosen in self.verify.nav.show(ui, rect, &folders, &open) {
            match chosen {
                NavChosen::NewRun => self.verify.screen = Screen::Front,
                NavChosen::Folder(f) => self.open_board(f),
                NavChosen::Map { folder, map } => self.open_map(&folder, &map),
                NavChosen::Run { folder, name, map } => self.verify.screen = Screen::Alignment { folder, name, map: Some(map) },
            }
        }
    }

    fn open_board(&mut self, folder: String) {
        if self.verify.screen != Screen::Board(folder.clone()) {
            self.verify.board.reset();
            self.verify.undo = None;
        }
        self.verify.screen = Screen::Board(folder);
    }

    /// A map's first file nobody has called, or its board when it has no alignments yet.
    fn open_map(&mut self, folder: &str, map: &str) {
        let first = self.verify.meta(folder).and_then(|m| first_of_map(m, map));
        match first {
            Some(r) => self.verify.screen = Screen::Alignment { folder: folder.to_string(), name: r.name, map: Some(map.to_string()) },
            None => self.open_board(folder.to_string()),
        }
    }

    /// Files dropped on the window while Verify is in front.
    pub(crate) fn verify_dropped(&mut self, paths: Vec<PathBuf>) {
        let folder = match &self.verify.screen {
            Screen::Board(f) | Screen::Alignment { folder: f, .. } => Some(f.clone()),
            Screen::Front => None,
        };
        self.verify_take(folder, None, paths);
    }

    pub(crate) fn verify_picked(&mut self, pick: Pick, paths: Vec<PathBuf>) {
        match pick {
            Pick::Files { folder, map } => self.verify_take(folder, map, paths),
            Pick::Download { folder, name, map, what } => {
                if let Some(path) = paths.first() {
                    let params =
                        json!({ "folder": folder, "sample": name, "map": map, "what": what, "path": path.display().to_string(), "overwrite": true });
                    if self.vrun("export_alignment", params).is_some() {
                        self.verify.busy = Some(format!("Saved {}", path.display()));
                    }
                }
            }
        }
    }

    /// One rule for every drop: maps become rows, reads are kept and placed.
    fn verify_take(&mut self, folder: Option<String>, map: Option<String>, paths: Vec<PathBuf>) {
        if paths.is_empty() || self.session.is_none() {
            return;
        }
        let folder = match folder {
            Some(f) => f,
            None => {
                let name = Some(self.verify.front.name.trim().to_string()).filter(|n| !n.is_empty()).unwrap_or_else(bios_project::folder_stamp);
                let Some(made) = self.vrun("create_verification", json!({ "name": name })) else { return };
                let Some(folder) = made["created"].as_str().map(str::to_string) else { return };
                self.verify.front = Front::new(bios_project::folder_stamp());
                folder
            }
        };
        let (reads, maps): (Vec<PathBuf>, Vec<PathBuf>) =
            paths.into_iter().partition(|p| p.file_name().is_some_and(|n| is_reads_file(&n.to_string_lossy())));
        let mut failures = Vec::new();
        for path in maps {
            if self.vrun("add_verification_map", json!({ "folder": folder, "file": path.display().to_string() })).is_none() {
                failures.extend(self.verify.error.take());
            }
        }
        if !reads.is_empty() {
            let files: Vec<String> = reads.iter().map(|p| p.display().to_string()).collect();
            if let Some(added) = self.vrun("add_reads", json!({ "folder": folder, "files": files, "map": map })) {
                failures.extend(added["refused"].as_array().into_iter().flatten().filter_map(Value::as_str).map(str::to_string));
            } else {
                failures.extend(self.verify.error.take());
            }
        }
        self.verify.error = failures.first().cloned();
        self.open_board(folder);
    }

    /// Starts a command off the window's thread. Its progress is shown on the board.
    fn verify_spawn(&mut self, folder: &str, command: &'static str, params: Value, reopen: Option<Screen>) {
        let Some(session) = &self.session else { return };
        if self.verify.worker.is_some() {
            return;
        }
        let root = session.project().root().to_path_buf();
        let cancel = Arc::new(AtomicBool::new(false));
        let progress = Arc::new(Mutex::new("Starting…".to_string()));
        let (c, p) = (cancel.clone(), progress.clone());
        let handle = std::thread::spawn(move || {
            let project = Project::open(&root).map_err(|e| e.to_string())?;
            let session = Session::new(project, Who { actor: Actor::Human, source: "window".into(), session: None }).with_mapper(default_mapper());
            let control = Control::new(c, move |text| {
                if let Ok(mut slot) = p.lock() {
                    *slot = text.to_string();
                }
            });
            session.execute_with(command, &params, control).map_err(|e| e.0)
        });
        self.verify.worker = Some(Worker { folder: folder.to_string(), cancel, progress, handle, reopen });
        self.verify.error = None;
    }

    /// Picks up a finished run.
    fn verify_poll(&mut self, ctx: &egui::Context) {
        let done = self.verify.worker.as_ref().is_some_and(|w| w.handle.is_finished());
        if !done {
            if self.verify.worker.is_some() {
                ctx.request_repaint_after(Duration::from_millis(150));
                // Alignments are kept as they finish: show them landing.
                if self.verify.refreshed.elapsed() > Duration::from_millis(700) {
                    self.verify_refresh();
                }
            }
            return;
        }
        let Some(worker) = self.verify.worker.take() else { return };
        match worker.handle.join() {
            Ok(Ok(result)) => {
                let failed: Vec<String> = result["failed"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(|f| Some(format!("{}: {}", f["sample"].as_str()?, f["error"].as_str()?)))
                    .collect();
                self.verify.error = failed.first().cloned();
                if result["cancelled"] == json!(true) {
                    self.verify.busy = Some("Cancelled; the samples not aligned are still on the board.".into());
                }
                if let Some(screen) = worker.reopen {
                    self.verify.loaded = None;
                    self.verify.screen = screen;
                }
            }
            Ok(Err(e)) => self.verify.error = Some(e),
            Err(_) => self.verify.error = Some("the run stopped inside Bios; this is a bug in Bios".into()),
        }
        self.refresh();
    }

    pub(crate) fn verify_work(&mut self, ui: &mut egui::Ui, work: Rect, dropping: bool) {
        self.verify_poll(ui.ctx());
        if self.verify.copied_at.is_some_and(|t| t.elapsed() > Duration::from_millis(1600)) {
            self.verify.copied_at = None;
        }
        match self.verify.screen.clone() {
            Screen::Front => {
                let (busy, error) = (self.verify.busy.clone(), self.verify.error.clone());
                if let Some(FrontAction::ChooseFiles) = self.verify.front.show(ui, work, dropping, busy.as_deref(), error.as_deref()) {
                    self.ask(FileDialog::open_many().title("Choose reads and maps"), Then::Verify(Pick::Files { folder: None, map: None }));
                }
            }
            Screen::Board(folder) => {
                let model = self.board_model(&folder);
                let actions = self.verify.board.show(ui, work, &model, dropping);
                for action in actions {
                    self.board_action(&folder, action, ui.ctx());
                }
            }
            Screen::Alignment { folder, name, map } => self.alignment(ui, work, &folder, &name, map),
        }
    }

    fn board_model(&self, folder: &str) -> BoardModel {
        let v = &self.verify;
        let Some(meta) = v.meta(folder) else { return BoardModel::default() };
        let rows: Vec<MapRow> = meta
            .maps()
            .iter()
            .filter_map(|m| {
                let c = v.maps.get(&m.construct_id)?;
                let stored: Vec<&RunSummary> = meta.runs.iter().filter(|r| meta.map_id_of(r).as_deref() == Some(m.construct_id.as_str())).collect();
                let waiting: Vec<_> = meta.staged.iter().filter(|s| s.map_id.as_deref() == Some(m.construct_id.as_str())).collect();
                let names: Vec<String> = stored.iter().map(|r| r.name.clone()).chain(waiting.iter().map(|s| s.name.clone())).collect();
                let labels = sibling_labels(&names);
                let label = |n: &str| labels.get(n).cloned().unwrap_or_else(|| run_label(n));
                Some(MapRow {
                    id: m.construct_id.clone(),
                    name: c.name.clone(),
                    length: c.sequence.len(),
                    waiting: waiting.iter().map(|s| Waiting { name: s.name.clone(), label: label(&s.name), files: s.files.clone() }).collect(),
                    stored: stored
                        .iter()
                        .map(|r| Stored {
                            run: (*r).clone(),
                            label: label(&r.name),
                            stale: run_stale(r.ref_length, r.ref_hash.as_deref(), Some(&c.sequence)),
                        })
                        .collect(),
                })
            })
            .collect();
        let tray: Vec<Waiting> = meta
            .staged
            .iter()
            .filter(|s| s.map_id.is_none())
            .map(|s| Waiting { name: s.name.clone(), label: s.files.first().cloned().unwrap_or_else(|| s.name.clone()), files: s.files.clone() })
            .collect();
        let placed: Vec<String> =
            meta.staged.iter().filter(|s| s.map_id.is_some()).map(|s| s.name.clone()).chain(meta.runs.iter().map(|r| r.name.clone())).collect();
        let hint = unplaced_hint(
            &tray.iter().map(|t| t.name.clone()).collect::<Vec<_>>(),
            &placed,
            &rows.iter().map(|r| r.name.clone()).collect::<Vec<_>>(),
        );
        let here: HashSet<&str> = rows.iter().map(|r| r.id.as_str()).collect();
        let uncalled: Vec<RunSummary> =
            call_order(meta).into_iter().filter(|r| r.call.is_none() && meta.map_id_of(r).is_some_and(|id| here.contains(id.as_str()))).collect();
        let folder_path = PathBuf::from(folder);
        let candidates: Vec<(String, String, usize)> = self
            .listing
            .iter()
            .filter(|l| !l.path.starts_with(&folder_path))
            .map(|l| (l.id.as_str().to_string(), l.name.clone(), l.length))
            .collect();
        let running = v.worker.as_ref().filter(|w| w.folder == folder).map(|w| {
            let text = w.progress.lock().map(|p| p.clone()).unwrap_or_default();
            format!("Aligning · {text}")
        });
        BoardModel {
            name: folder.rsplit('/').next().unwrap_or(folder).to_string(),
            rows,
            tray,
            hint,
            candidates,
            running,
            busy: v.busy.clone(),
            error: v.error.clone(),
            first_uncalled: uncalled.first().map(|r| (r.name.clone(), meta.map_id_of(r))),
            uncalled: uncalled.len(),
            good: meta.runs.iter().filter(|r| r.call == Some(Call::Good)).count(),
            copied: v.copied_at.is_some(),
            can_undo_clear: v.undo.as_ref().is_some_and(|(f, _)| f == folder),
        }
    }

    fn board_action(&mut self, folder: &str, action: BoardAction, ctx: &egui::Context) {
        self.verify.busy = None;
        if !matches!(action, BoardAction::UndoClear) && !matches!(action, BoardAction::ClearAll) {
            self.verify.undo = None;
        }
        match action {
            BoardAction::UploadFiles => {
                self.ask(FileDialog::open_many().title("Upload reads or maps"), Then::Verify(Pick::Files { folder: Some(folder.into()), map: None }))
            }
            BoardAction::AddReadsTo(map) => {
                let name = self.verify.map_name(&map).unwrap_or_default();
                self.ask(
                    FileDialog::open_many().title(format!("Add reads to {name}")),
                    Then::Verify(Pick::Files { folder: Some(folder.into()), map: Some(map) }),
                );
            }
            BoardAction::AddMap(id) => {
                self.vrun("add_verification_map", json!({ "folder": folder, "plasmid": id }));
            }
            BoardAction::ChangeMap { map, plasmid } => {
                self.vrun("change_verification_map", json!({ "folder": folder, "map": map, "plasmid": plasmid }));
            }
            BoardAction::AutoMatch => {
                self.vrun("match_samples", json!({ "folder": folder }));
            }
            BoardAction::ClearAll => {
                let placed: Vec<(String, String)> = self
                    .verify
                    .meta(folder)
                    .map(|m| m.staged.iter().filter_map(|s| Some((s.name.clone(), s.map_id.clone()?))).collect())
                    .unwrap_or_default();
                let names: Vec<&String> = placed.iter().map(|(n, _)| n).collect();
                if self.vrun("place_samples", json!({ "folder": folder, "samples": names })).is_some() {
                    self.verify.undo = Some((folder.to_string(), placed));
                }
            }
            BoardAction::UndoClear => {
                if let Some((_, placed)) = self.verify.undo.take() {
                    let mut by_map: HashMap<String, Vec<String>> = HashMap::new();
                    for (name, map) in placed {
                        by_map.entry(map).or_default().push(name);
                    }
                    for (map, names) in by_map {
                        self.vrun("place_samples", json!({ "folder": folder, "samples": names, "map": map }));
                    }
                }
            }
            BoardAction::Rename(name) => {
                if let Some(done) = self.vrun("rename_verification", json!({ "folder": folder, "name": name }))
                    && let Some(to) = done["folder"].as_str()
                {
                    self.verify.screen = Screen::Board(to.to_string());
                }
            }
            BoardAction::DeleteVerification => {
                if self.vrun("delete_verification", json!({ "folder": folder, "confirm": true })).is_some() {
                    self.verify.screen = Screen::Front;
                }
            }
            BoardAction::Place { sample, map } => {
                self.vrun("place_samples", json!({ "folder": folder, "samples": [sample], "map": map }));
            }
            BoardAction::Remove(sample) => {
                self.vrun("remove_sample", json!({ "folder": folder, "sample": sample }));
            }
            BoardAction::OpenRun { name, map } => {
                if name.is_empty() {
                    if let Some(map) = map {
                        self.open_map(folder, &map);
                    }
                } else {
                    self.verify.screen = Screen::Alignment { folder: folder.into(), name, map };
                }
            }
            BoardAction::DeleteRun { name, map } => {
                self.vrun("delete_alignment", json!({ "folder": folder, "sample": name, "map": map }));
            }
            BoardAction::Run => self.verify_spawn(folder, "run_verification", json!({ "folder": folder }), None),
            BoardAction::Cancel => {
                if let Some(w) = &self.verify.worker {
                    w.cancel.store(true, Ordering::Relaxed);
                }
            }
            BoardAction::Call => {
                let first = self.board_model(folder).first_uncalled;
                if let Some((name, map)) = first {
                    self.verify.screen = Screen::Alignment { folder: folder.into(), name, map };
                }
            }
            BoardAction::Copy => {
                if let Some(picks) = self.vrun("copy_picks", json!({ "folder": folder }))
                    && let Some(text) = picks["text"].as_str()
                {
                    ctx.copy_text(text.to_string());
                    self.verify.copied_at = Some(Instant::now());
                }
            }
        }
    }

    fn alignment(&mut self, ui: &mut egui::Ui, work: Rect, folder: &str, name: &str, map: Option<String>) {
        let key = (folder.to_string(), name.to_string(), map.clone());
        let Some(meta) = self.verify.meta(folder).cloned() else {
            self.verify.screen = Screen::Front;
            return;
        };
        let map = map.or_else(|| meta.runs.iter().find(|r| r.name == name).and_then(|r| meta.map_id_of(r)));
        if self.verify.loaded.as_ref().is_none_or(|l| l.key != key) {
            let Some(session) = &self.session else { return };
            match load_run(session.project(), folder, name, map.as_deref()) {
                Ok(output) => {
                    let reading = mapper::read(&output);
                    let columns = reference_columns(&output.padded_reference);
                    let features = output
                        .reference
                        .features
                        .iter()
                        .map(|f| ColumnFeature {
                            name: f.name.clone(),
                            kind: f.kind.clone(),
                            start: columns.get(f.start).copied().unwrap_or(f.start),
                            end: columns.get(f.end).copied().unwrap_or(f.end),
                        })
                        .collect();
                    self.verify.loaded = Some(Loaded { key: key.clone(), output, reading, features });
                }
                Err(e) => {
                    self.verify.error = Some(e);
                    self.verify.screen = Screen::Board(folder.to_string());
                    return;
                }
            }
        }
        let Some(loaded) = &self.verify.loaded else { return };
        let run = meta.run(name, map.as_deref());
        let map_construct = map.as_ref().and_then(|m| self.verify.maps.get(m));
        let mut siblings: Vec<&RunSummary> = meta.runs.iter().filter(|r| meta.map_id_of(r) == map).collect();
        siblings.sort_by(|a, b| natural_cmp(&a.name, &b.name));
        let at = siblings.iter().position(|r| r.name == name);
        let files = run.and_then(|r| r.read_files.clone()).unwrap_or_default();
        let can_rerun = !files.is_empty() && files.iter().all(|f| meta.reads.iter().any(|r| &r.name == f)) && map_construct.is_some();
        let mismatch = map_construct.filter(|c| c.sequence.to_uppercase() != loaded.output.reference.bases).map(|c| {
            format!(
                "These reads were aligned against {} bp and the map in this folder is {} bp. Something has edited the copy — the positions below no longer describe it.",
                bios_core::verify::grouped(loaded.reading.ref_length as i64),
                bios_core::verify::grouped(c.sequence.len() as i64)
            )
        });
        let map_name = map_construct.map_or_else(|| loaded.output.reference.name.clone(), |c| c.name.clone());
        let model = AlignModel {
            output: &loaded.output,
            reading: &loaded.reading,
            sample: name,
            map_name: &map_name,
            run,
            features: &loaded.features,
            file_at: at.map(|i| (i, siblings.len())),
            can_rerun,
            rerunning: self.verify.worker.is_some(),
            error: self.verify.error.as_deref(),
            mismatch,
        };
        let actions = self.verify.align.show(ui, work, &model);
        let sibling_names: Vec<String> = siblings.iter().map(|r| r.name.clone()).collect();
        for action in actions {
            match action {
                AlignAction::Overview => self.verify.screen = Screen::Board(folder.to_string()),
                AlignAction::BrowseFile(by) => {
                    if let Some(i) = at
                        && sibling_names.len() > 1
                    {
                        let to = (i as i64 + i64::from(by)).rem_euclid(sibling_names.len() as i64) as usize;
                        if let Some(n) = sibling_names.get(to) {
                            self.verify.screen = Screen::Alignment { folder: folder.into(), name: n.clone(), map: map.clone() };
                        }
                    }
                }
                AlignAction::Call(call) => {
                    let params = json!({ "folder": folder, "sample": name, "map": map, "call": call.as_str() });
                    if self.vrun("call_sample", params).is_some() {
                        self.verify.called_here.insert(format!("{}\u{0}{name}", map.clone().unwrap_or_default()));
                        let fresh = self.verify.meta(folder).cloned().unwrap_or(meta.clone());
                        let here = &self.verify.called_here;
                        let next = next_to_call(&fresh, name, map.as_deref(), |r| {
                            here.contains(&format!("{}\u{0}{}", fresh.map_id_of(r).unwrap_or_default(), r.name))
                        });
                        self.verify.screen = match next {
                            Some(r) => Screen::Alignment { folder: folder.into(), map: fresh.map_id_of(&r), name: r.name },
                            None => Screen::Board(folder.to_string()),
                        };
                    }
                }
                AlignAction::Rerun => {
                    let back = Screen::Alignment { folder: folder.into(), name: name.into(), map: map.clone() };
                    self.verify_spawn(folder, "rerun_alignment", json!({ "folder": folder, "sample": name, "map": map }), Some(back));
                }
                AlignAction::Download(what) => {
                    let file = match what {
                        "consensus" => "consensus.fasta",
                        "sam" => "alignment.sam",
                        "coverage" => "coverage.tsv",
                        "differences" => "column-differences.tsv",
                        _ => "alignment.zip",
                    };
                    let suggested = format!("{}-{file}", run_label(name));
                    self.ask(
                        FileDialog::save().title("Save").file_name(suggested),
                        Then::Verify(Pick::Download { folder: folder.into(), name: name.into(), map: map.clone(), what }),
                    );
                }
            }
        }
    }
}
