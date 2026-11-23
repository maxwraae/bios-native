//! The sidebar's list in Edit and Build: a search once the list outgrows the eye, and the
//! project's folders as collapsible sections with their plasmids under them. The head above it
//! (the project's name, its quick actions, the places) is `sidebar`.
//!
//! A deliberate departure from the web (Max, 2026-11-22), modelled on the Claude desktop app's
//! grouped list: the web's explorer of folder rows with a chevron, a folder icon and a rule down
//! their contents became sections, a small muted header with the chevron on its right and the
//! plasmids listed under it, nested folders as sub-sections and plasmids in no folder under
//! "No folder" at the end. Sections are open until shut, and the shut ones are remembered.
//!
//! Ported from the web's `layout/SessionNav.tsx` 100-440 (`ConstructSearch`, `AddConstructs`,
//! `RailHead`, `RailTrail`, `NewFolderInput`) and 949-1624 (the menus, `renderFolder`,
//! `renderConstruct`, `ProjectConstructs`' body), `layout/RailMenu.tsx`, `layout/RailResizer.tsx`,
//! `layout/railWidth.ts`, `components/ConstructName.tsx` with `workspace/constructName.ts`
//! (`splitConstructName`), `layout/sessionConstructs.ts` 255-300 and 670-700 (the tree's order),
//! and the styles in `layout/explorer.css` and `styles.css` 1035-2085 and 10200-10265.
//!
//! Left out, because Bios on your own machine has no counterpart: the assistant's focus button on
//! a row, the share button in the head, and Build's and Verify's typed folders. The native
//! "Recently deleted" section, which the web has not got, closes the list, drawn as a folder row.

use std::cmp::Ordering;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use bios_project::{ConstructId, Deleted, Listed};
use egui::{Align2, Color32, CornerRadius, CursorIcon, FontId, Key, Pos2, Rect, Sense, Stroke, StrokeKind, Ui, UiBuilder, pos2, vec2};

use crate::primitives::text_width;
use crate::theme::{self, Weight};
use crate::{icon, kind_icons, lucide, rail_menu};

/// Below this many plasmids a project is short enough to read, so there is no search field
/// (`SEARCH_AT`).
pub const SEARCH_AT: usize = 8;
/// A row's slot: 30 for `.rail-file`, and its 1px margin above and below.
const ROW: f32 = theme::SIDE_ROW;
/// `.rail-file-row`'s indent per level (`--depth * 13px`).
const INDENT: f32 = 13.0;
/// A section header's slot.
const HEADER: f32 = 28.0;
/// The space above every section but the first.
const SECTION_GAP: f32 = 6.0;
/// The key "No folder" is remembered under; no folder can have an empty path.
const LOOSE: &str = "";
/// The key of the Recently deleted section, which is no folder and takes no drop.
const DELETED: &str = "\u{0}deleted";

/// One thing a search of the project's features and bases found (the native search, kept behind
/// the web's name filter).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Found {
    pub id: ConstructId,
    /// The bases the hit is on, first and last, 0-based; `None` for a hit on the plasmid itself.
    pub span: Option<(usize, usize)>,
    /// The construct's path, or the feature's name.
    pub title: String,
    /// What kind of hit it is and where: `feature in backbones/pUC19, 866..1454`.
    pub detail: String,
}

/// What the panel shows.
pub struct Model<'a> {
    pub folders: &'a [PathBuf],
    pub listing: &'a [Listed],
    pub open: Option<&'a ConstructId>,
    /// What the project-wide search found for the query, beyond names.
    pub found: Option<&'a [Found]>,
    /// Put the keyboard in the search box this frame (Edit > Find in Project).
    pub focus_search: bool,
    /// Plasmids that were deleted and can be brought back, most recent last.
    pub deleted: &'a [Deleted],
    /// Folders a feature made and what kind each is (`build`, `verification`), by path.
    ///
    /// A typed folder is not organisation: it was filled by one feature and its contents are only
    /// comparable to each other, so the rail says so at a glance — the feature's own icon in the
    /// feature's own colour. A kind this version does not know draws as an ordinary folder, which
    /// matters: a project opened in an older Bios must show a folder it does not understand as a
    /// folder holding real plasmids, rather than as a gap.
    pub folder_kinds: &'a [(String, String)],
    /// The window is in the Build place, so the head offers a new build.
    pub building: bool,
}

/// What the person asked for in the panel. Each is one engine command, or a window action.
#[derive(Clone, Debug, PartialEq)]
pub enum Chosen {
    /// Open a plasmid, at the bases a search hit named.
    Open {
        id: ConstructId,
        span: Option<(usize, usize)>,
    },
    /// Bring a deleted plasmid back.
    Restore(ConstructId),
    Import,
    NewPlasmid,
    /// A new plasmid in this folder (a section header's +).
    NewPlasmidIn(String),
    /// Make this folder (a path inside the project).
    NewFolder(String),
    RenameFolder {
        from: String,
        to: String,
    },
    Rename {
        id: ConstructId,
        name: String,
    },
    Duplicate(ConstructId),
    Download(ConstructId),
    /// Move a plasmid to a folder; `""` is the top of the project.
    MoveTo {
        id: ConstructId,
        folder: String,
    },
    Delete(ConstructId),
    /// The sidebar was dragged to this width (`sidebar::edge`); `done` once the drag ends and it is
    /// worth remembering.
    Width {
        width: f32,
        done: bool,
    },
    /// The sections the person has shut, to remember for this project.
    ShutFolders(Vec<String>),
    /// Build: leave whatever folder's page is open and start a build from the plasmid in front.
    NewBuild,
    /// Build: show this build folder's page. A typed folder is a thing with a page, not only a
    /// drawer, so pressing it opens the page as well as the rows under it.
    ShowFolder(String),
}

/// What is being renamed in place.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Target {
    Plasmid(ConstructId),
    Folder(String),
}

/// A name being typed into a row.
struct Typing {
    text: String,
    focused: bool,
}

/// One entry of a row's right-click menu.
type Item = rail_menu::Item<Does>;

/// What a menu entry does.
#[derive(Clone, Debug)]
enum Does {
    Choose(Chosen),
    Rename(Target),
    ConfirmDelete(ConstructId),
    NewFolderIn(String),
    OpenFolder(String),
}

/// The panel's own state between frames: what is open, being named or being asked about.
#[derive(Default)]
pub struct Nav {
    /// The listing in name order, with what it was sorted from (see `by_name`).
    by_name: (u64, Vec<usize>),
    /// Sections shut on purpose, by folder path (`LOOSE` for "No folder"). Remembered per project.
    shut: BTreeSet<String>,
    renaming: Option<(Target, Typing)>,
    confirming: Option<ConstructId>,
    /// A new folder being named, under this parent (`""` at the top).
    naming: Option<(String, Typing)>,
    menu: Option<rail_menu::Menu<Does>>,
    /// Find in Project asked for the field while the list is short.
    search_open: bool,
    deleted_open: bool,
    last_open: Option<ConstructId>,
    dragging: Option<ConstructId>,
    /// Where the plasmid being dragged would land: a folder, or `""` for the top.
    drop_on: Option<String>,
}

/// A row of the tree.
enum Node<'a> {
    Folder { path: String, name: String, children: Vec<Node<'a>> },
    Plasmid(&'a Listed),
}

fn slash(path: &Path) -> String {
    path.components().map(|c| c.as_os_str().to_string_lossy()).collect::<Vec<_>>().join("/")
}

fn parent_of(path: &str) -> &str {
    path.rfind('/').map_or("", |i| &path[..i])
}

fn base_of(path: &str) -> &str {
    path.rfind('/').map_or(path, |i| &path[i + 1..])
}

fn folder_of(listed: &Listed) -> String {
    listed.path.parent().map(slash).unwrap_or_default()
}

/// `localeCompare(…, { numeric: true })`: letters without regard to case, runs of digits by value.
fn natural(a: &str, b: &str) -> Ordering {
    let (mut a, mut b) = (a.chars().peekable(), b.chars().peekable());
    loop {
        match (a.peek().copied(), b.peek().copied()) {
            (None, None) => return Ordering::Equal,
            (None, Some(_)) => return Ordering::Less,
            (Some(_), None) => return Ordering::Greater,
            (Some(x), Some(y)) if x.is_ascii_digit() && y.is_ascii_digit() => {
                let run = |it: &mut std::iter::Peekable<std::str::Chars>| {
                    let mut digits = String::new();
                    while let Some(c) = it.peek().copied().filter(char::is_ascii_digit) {
                        digits.push(c);
                        it.next();
                    }
                    digits
                };
                let (x, y) = (run(&mut a), run(&mut b));
                let (tx, ty) = (x.trim_start_matches('0'), y.trim_start_matches('0'));
                let order = tx.len().cmp(&ty.len()).then_with(|| tx.cmp(ty));
                if order != Ordering::Equal {
                    return order;
                }
            }
            (Some(x), Some(y)) => {
                let order = x.to_lowercase().cmp(y.to_lowercase());
                if order != Ordering::Equal {
                    return order;
                }
                a.next();
                b.next();
            }
        }
    }
}

/// The rows as a tree: folders first and by name, then the plasmids by name, numbers in order
/// (`railTree`, then `explorerTree`). A folder only a plasmid's path names is drawn anyway.
fn tree<'a>(listing: &'a [Listed], folders: &[PathBuf], by_name: &[usize]) -> Vec<Node<'a>> {
    let mut dirs: BTreeSet<String> = folders.iter().map(|f| slash(f)).filter(|f| !f.is_empty()).collect();
    for listed in listing {
        let mut dir = folder_of(listed);
        while !dir.is_empty() {
            let up = parent_of(&dir).to_string();
            dirs.insert(dir);
            dir = up;
        }
    }
    fn build<'a>(dir: &str, dirs: &BTreeSet<String>, listing: &'a [Listed], by_name: &[usize], depth: usize) -> Vec<Node<'a>> {
        let mut sub: Vec<&String> = dirs.iter().filter(|d| parent_of(d) == dir).collect();
        sub.sort_by(|a, b| natural(base_of(a), base_of(b)));
        let mut out: Vec<Node<'a>> = Vec::new();
        for d in sub {
            // Folders inside folders are bounded by the project's own depth; this guards the walk.
            let children = if depth < 64 { build(d, dirs, listing, by_name, depth + 1) } else { Vec::new() };
            out.push(Node::Folder { path: d.clone(), name: base_of(d).to_string(), children });
        }
        // Already in name order: `by_name` is the listing sorted once, not on every frame.
        out.extend(by_name.iter().filter_map(|&i| listing.get(i)).filter(|l| folder_of(l) == dir).map(Node::Plasmid));
        out
    }
    build("", &dirs, listing, by_name, 0)
}

/// The listing's places in name order, numbers in order: what each folder's plasmids are drawn
/// in. Kept until a name changes, since sorting hundreds of names by hand-made order every frame
/// was a good part of what a frame cost.
fn by_name(kept: &mut (u64, Vec<usize>), listing: &[Listed]) -> Vec<usize> {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    for l in listing {
        l.name.hash(&mut hasher);
    }
    let key = hasher.finish();
    if kept.0 != key || kept.1.len() != listing.len() {
        let mut order: Vec<usize> = (0..listing.len()).collect();
        order.sort_by(|&a, &b| natural(&listing[a].name, &listing[b].name));
        *kept = (key, order);
    }
    kept.1.clone()
}

/// Every folder in the tree, for Move to.
fn folders_in(nodes: &[Node<'_>], out: &mut Vec<String>) {
    for node in nodes {
        if let Node::Folder { path, children, .. } = node {
            out.push(path.clone());
            folders_in(children, out);
        }
    }
}

/// `splitConstructName`: the part of a name that may be clipped, and the end that must survive.
pub fn split_name(name: &str) -> (&str, &str) {
    const SHORT_ENOUGH: usize = 18;
    const MAX_TAIL: usize = 14;
    let chars: Vec<(usize, char)> = name.char_indices().collect();
    let n = chars.len();
    if n <= SHORT_ENOUGH {
        return (name, "");
    }
    let from = 1.max(n - MAX_TAIL);
    let mut cut = (from..n).rev().find(|&i| chars.get(i).is_some_and(|(_, c)| matches!(c, '_' | '-' | '.' | ' ' | '(' | '['))).unwrap_or(0);
    if cut == 0 {
        cut = n - 8;
    }
    let at = chars.get(cut).map_or(name.len(), |(i, _)| *i);
    (&name[..at], &name[at..])
}

/// A name fitted into `width`, with the ellipsis spent in the middle so its end survives
/// (`ConstructName`): the head clips, the tail does not.
pub fn fit_name(ui: &Ui, name: &str, font: &FontId, width: f32) -> String {
    if text_width(ui, name, font.clone()) <= width {
        return name.to_string();
    }
    let (head, tail) = split_name(name);
    let room = width - text_width(ui, tail, font.clone());
    format!("{}{tail}", clip(ui, head, font, room))
}

/// `text-overflow: ellipsis`: as much of `text` as fits in `width`, then "…".
pub fn clip(ui: &Ui, text: &str, font: &FontId, width: f32) -> String {
    if text_width(ui, text, font.clone()) <= width {
        return text.to_string();
    }
    let chars: Vec<char> = text.chars().collect();
    let (mut lo, mut hi) = (0usize, chars.len());
    while lo < hi {
        let mid = (lo + hi).div_ceil(2);
        let candidate: String = chars.iter().take(mid).collect::<String>() + "…";
        if text_width(ui, &candidate, font.clone()) <= width { lo = mid } else { hi = mid - 1 }
    }
    chars.iter().take(lo).collect::<String>() + "…"
}

/// `color-mix(in srgb, var(--ink) <amount>, transparent)`.
fn ink_wash(amount: f32) -> Color32 {
    theme::alpha(theme::INK, amount)
}

fn radius() -> CornerRadius {
    CornerRadius::same(theme::RADIUS_SM as u8)
}

/// A name box, as `.rail-file-rename` draws one: no frame, 13px, in the row it names.
fn name_box(ui: &mut Ui, rect: Rect, typing: &mut Typing, hint: &str) -> Option<bool> {
    let edit = egui::TextEdit::singleline(&mut typing.text)
        .frame(egui::Frame::NONE)
        .margin(egui::Margin::ZERO)
        .font(theme::sans(13.0))
        .text_color(theme::INK)
        .hint_text(egui::RichText::new(hint).color(theme::MUTED))
        .desired_width(rect.width());
    let response = ui.put(rect, edit);
    if !typing.focused {
        response.request_focus();
        typing.focused = true;
    }
    // Enter commits, and so does clicking away: a name you just typed means you meant it.
    // Escape lets it go.
    if ui.input(|i| i.key_pressed(Key::Escape)) && (response.has_focus() || response.lost_focus()) {
        return Some(false);
    }
    response.lost_focus().then_some(true)
}

impl Nav {
    /// Starts on a project: nothing being named or asked, its remembered shut sections shut.
    pub fn arrive(&mut self, shut_folders: Vec<String>) {
        *self = Nav { shut: shut_folders.into_iter().collect(), ..Nav::default() };
    }

    /// Starts naming a new folder at the top of the project (the head's new-folder action).
    pub fn start_new_folder(&mut self) {
        self.naming = Some((String::new(), Typing { text: String::new(), focused: false }));
    }

    /// The sections the plasmid in front sits in, which open when it comes to the front.
    fn holding(open: Option<&ConstructId>, listing: &[Listed]) -> BTreeSet<String> {
        let mut out = BTreeSet::new();
        if let Some(listed) = open.and_then(|id| listing.iter().find(|l| &l.id == id)) {
            let mut dir = folder_of(listed);
            if dir.is_empty() {
                out.insert(LOOSE.to_string());
            }
            while !dir.is_empty() {
                let up = parent_of(&dir).to_string();
                out.insert(dir);
                dir = up;
            }
        }
        out
    }

    fn set_open(&mut self, path: &str, open: bool, out: &mut Vec<Chosen>) {
        let changed = if open { self.shut.remove(path) } else { self.shut.insert(path.to_string()) };
        if changed {
            out.push(Chosen::ShutFolders(self.shut.iter().cloned().collect()));
        }
    }

    fn plasmid_menu(listed: &Listed, folders: &[String]) -> Vec<Vec<Item>> {
        let here = folder_of(listed);
        let mut targets = Vec::new();
        if !here.is_empty() {
            targets.push(Item::new("Top level", Does::Choose(Chosen::MoveTo { id: listed.id.clone(), folder: String::new() })));
        }
        for f in folders.iter().filter(|f| **f != here) {
            targets.push(Item::new(f.clone(), Does::Choose(Chosen::MoveTo { id: listed.id.clone(), folder: f.clone() })));
        }
        let mut edits = vec![
            Item::new("Rename", Does::Rename(Target::Plasmid(listed.id.clone()))),
            Item::new("Duplicate", Does::Choose(Chosen::Duplicate(listed.id.clone()))),
            Item::new("Download", Does::Choose(Chosen::Download(listed.id.clone()))),
        ];
        if !targets.is_empty() {
            edits.push(Item::submenu("Move to", targets));
        }
        let delete = Item::new("Delete…", Does::ConfirmDelete(listed.id.clone())).danger();
        vec![vec![Item::new("Open", Does::Choose(Chosen::Open { id: listed.id.clone(), span: None }))], edits, vec![delete]]
    }

    fn folder_menu(path: &str) -> Vec<Vec<Item>> {
        // The engine has no command that deletes a folder, so the window cannot either; the item
        // says why rather than going missing (the web's rule for a delete that cannot happen).
        let delete = Item::disabled("Delete…", "Bios cannot delete a folder yet");
        vec![
            vec![Item::new("Open", Does::OpenFolder(path.to_string()))],
            vec![
                Item::new("New folder inside", Does::NewFolderIn(path.to_string())),
                Item::new("Rename", Does::Rename(Target::Folder(path.to_string()))),
            ],
            vec![delete],
        ]
    }

    fn blank_menu() -> Vec<Vec<Item>> {
        vec![vec![
            Item::new("Import files…", Does::Choose(Chosen::Import)),
            Item::new("New plasmid", Does::Choose(Chosen::NewPlasmid)),
            Item::new("New folder", Does::NewFolderIn(String::new())),
        ]]
    }

    fn open_menu(&mut self, at: Option<Pos2>, groups: Vec<Vec<Item>>) {
        if let Some(at) = at {
            self.menu = Some(rail_menu::Menu::new(at, groups));
        }
    }

    /// Does what a menu entry says.
    fn act(&mut self, does: Does, out: &mut Vec<Chosen>) {
        match does {
            Does::Choose(chosen) => out.push(chosen),
            Does::Rename(target) => {
                let text = match &target {
                    Target::Plasmid(_) => String::new(),
                    Target::Folder(path) => base_of(path).to_string(),
                };
                self.renaming = Some((target, Typing { text, focused: false }));
            }
            Does::ConfirmDelete(id) => self.confirming = Some(id),
            Does::NewFolderIn(parent) => {
                if !parent.is_empty() {
                    self.set_open(&parent, true, out);
                }
                self.naming = Some((parent, Typing { text: String::new(), focused: false }));
            }
            Does::OpenFolder(path) => self.set_open(&path, true, out),
        }
    }

    /// Draws the list in `rect`, under the sidebar's head, and says what was asked for.
    pub fn show(&mut self, ui: &mut Ui, rect: Rect, model: &Model<'_>, query: &mut String) -> Vec<Chosen> {
        let mut out = Vec::new();
        let painter = ui.painter().with_clip_rect(rect);

        let order = by_name(&mut self.by_name, model.listing);
        let nodes = tree(model.listing, model.folders, &order);
        let mut folder_paths = Vec::new();
        folders_in(&nodes, &mut folder_paths);
        // The plasmid in front moved: the sections it sits in open.
        if model.open != self.last_open.as_ref() {
            for path in Nav::holding(model.open, model.listing) {
                self.set_open(&path, true, &mut out);
            }
            self.last_open = model.open.cloned();
        }
        let mut top = rect.top();

        // The search, once the list outgrows the eye, and for as long as it holds a query.
        let count = model.listing.len();
        if model.focus_search {
            self.search_open = true;
        }
        if count >= SEARCH_AT || !query.is_empty() || self.search_open {
            let field = Rect::from_min_size(pos2(rect.left() + 4.0, top), vec2(rect.width() - 8.0, 30.0));
            painter.rect_filled(field, radius(), theme::CELL);
            icon::paint(&painter, pos2(field.left() + 9.0 + 7.0, field.center().y), 14.0, lucide::SEARCH, theme::MUTED);
            let input =
                Rect::from_min_max(pos2(field.left() + 9.0 + 14.0 + 7.0, field.center().y - 9.0), pos2(field.right() - 9.0, field.center().y + 9.0));
            let edit = egui::TextEdit::singleline(query)
                .frame(egui::Frame::NONE)
                .margin(egui::Margin::ZERO)
                .font(theme::sans(13.0))
                .text_color(theme::INK)
                .hint_text(egui::RichText::new("Search this project").color(theme::MUTED))
                .desired_width(input.width());
            let response = ui.put(input, edit);
            if model.focus_search {
                response.request_focus();
            }
            if response.has_focus() && ui.input(|i| i.key_pressed(Key::Escape)) {
                query.clear();
                self.search_open = false;
            }
            if response.lost_focus() && query.is_empty() {
                self.search_open = false;
            }
            top = field.bottom() + 6.0;
        }

        // The list.
        let body = Rect::from_min_max(pos2(rect.left(), top), rect.max);
        let searching = !query.trim().is_empty();
        let mut folder_rows: Vec<(Rect, String)> = Vec::new();
        let mut row_menu: Option<Vec<Vec<Item>>> = None;
        ui.scope_builder(UiBuilder::new().max_rect(body), |ui| {
            ui.set_clip_rect(body.intersect(ui.clip_rect()));
            egui::ScrollArea::vertical().auto_shrink([false, false]).id_salt("session-nav").show(ui, |ui| {
                ui.spacing_mut().item_spacing = vec2(0.0, 0.0);
                let mut ctx =
                    Rows { model, folders: &folder_paths, out: &mut out, folder_rows: &mut folder_rows, menu: &mut row_menu, started: false };
                if searching {
                    self.hits(ui, model, query, ctx.out);
                } else if nodes.is_empty() && self.naming.is_none() {
                    hint(ui, "Nothing here yet.");
                } else {
                    self.sections(ui, &nodes, &mut ctx);
                }
                if !searching && !model.deleted.is_empty() {
                    self.deleted_rows(ui, model, &mut ctx);
                }
                ui.add_space(2.0);
                // The rest of the column: right-click here offers the ways to add something.
                let rest = ui.available_rect_before_wrap();
                let rest = Rect::from_min_max(rest.min, pos2(rest.max.x, rest.max.y.max(body.bottom())));
                let response = ui.interact(rest, ui.scope_id().with("rail-blank"), Sense::click());
                if response.secondary_clicked() {
                    *ctx.menu = Some(Nav::blank_menu());
                }
            });
        });
        if let Some(groups) = row_menu {
            self.open_menu(ui.ctx().pointer_interact_pos(), groups);
        }
        self.drag(ui, body, &folder_rows, &mut out);

        if let Some(menu) = self.menu.as_mut() {
            match menu.show(ui) {
                rail_menu::Shown::StillOpen => {}
                rail_menu::Shown::Closed => self.menu = None,
                rail_menu::Shown::Picked(does) => {
                    self.menu = None;
                    self.act(does, &mut out);
                }
            }
        }
        out
    }

    /// The top of the list: a section for each folder, a new folder being named, and the plasmids
    /// in no folder, under "No folder" when there are folders to tell them from.
    fn sections(&mut self, ui: &mut Ui, nodes: &[Node<'_>], ctx: &mut Rows<'_, '_>) {
        let loose: Vec<&Listed> = nodes.iter().filter_map(|n| if let Node::Plasmid(l) = n { Some(*l) } else { None }).collect();
        let mut any_folder = false;
        for node in nodes {
            if let Node::Folder { path, name, children } = node {
                any_folder = true;
                self.section(ui, path, name, children, 0, ctx);
            }
        }
        if self.naming.as_ref().is_some_and(|(parent, _)| parent.is_empty()) {
            self.new_folder_row(ui, 0, ctx.out);
        }
        if loose.is_empty() {
            return;
        }
        if any_folder {
            let open = !self.shut.contains(LOOSE);
            let pressed = self.header(ui, LOOSE, "No folder", None, open, 0, false, ctx);
            if pressed.toggle {
                self.set_open(LOOSE, !open, ctx.out);
            }
            if pressed.menu {
                *ctx.menu = Some(Nav::blank_menu());
            }
            if !open {
                return;
            }
        }
        for listed in loose {
            self.plasmid_row(ui, listed, 0, ctx);
        }
    }

    /// One folder's section at `depth`: its header, and while open its plasmids, then its folders
    /// as sub-sections.
    fn section(&mut self, ui: &mut Ui, path: &str, name: &str, children: &[Node<'_>], depth: usize, ctx: &mut Rows<'_, '_>) {
        let open = !self.shut.contains(path);
        let glyph = match ctx.model.folder_kinds.iter().find(|(f, _)| f == path).map(|(_, kind)| kind.as_str()) {
            // Build borrows BUILD's own icon and colour, so the section and the place that filled
            // it read as one thing. Verification does the same with Verify's.
            Some("build") => Some((lucide::BLOCKS, theme::PLACE_BUILD)),
            Some("verification") => Some((lucide::CIRCLE_CHECK, theme::PLACE_VERIFY)),
            _ => None,
        };
        if let Some((Target::Folder(p), typing)) = self.renaming.as_mut().filter(|(t, _)| *t == Target::Folder(path.to_string())) {
            if ctx.started {
                ui.add_space(SECTION_GAP);
            }
            ctx.started = true;
            let (slot, _) = ui.allocate_exact_size(vec2(ui.available_width(), HEADER), Sense::hover());
            let row = slot.shrink2(vec2(4.0, 1.0));
            let left = row.left() + depth as f32 * INDENT;
            let field = Rect::from_min_max(pos2(left + 10.0, row.center().y - 9.0), pos2(row.right() - 10.0, row.center().y + 9.0));
            let from = p.clone();
            if let Some(commit) = name_box(ui, field, typing, "") {
                let typed = typing.text.trim().to_string();
                self.renaming = None;
                let parent = parent_of(&from);
                let to = if parent.is_empty() { typed.clone() } else { format!("{parent}/{typed}") };
                if commit && !typed.is_empty() && to != from {
                    ctx.out.push(Chosen::RenameFolder { from, to });
                }
            }
        } else {
            let pressed = self.header(ui, path, name, glyph, open, depth, true, ctx);
            if pressed.add {
                ctx.out.push(Chosen::NewPlasmidIn(path.to_string()));
            } else if pressed.toggle {
                if ctx.model.building && ctx.model.folder_kinds.iter().any(|(f, kind)| f == path && kind == "build") {
                    ctx.out.push(Chosen::ShowFolder(path.to_string()));
                }
                self.set_open(path, !open, ctx.out);
            }
            if pressed.menu {
                *ctx.menu = Some(Nav::folder_menu(path));
            }
        }
        if !open {
            return;
        }
        let mut empty = true;
        for node in children {
            if let Node::Plasmid(listed) = node {
                empty = false;
                self.plasmid_row(ui, listed, depth, ctx);
            }
        }
        for node in children {
            if let Node::Folder { path, name, children } = node {
                empty = false;
                // How deep folders go is bounded by the tree's own walk (`tree`).
                self.section(ui, path, name, children, depth + 1, ctx);
            }
        }
        if self.naming.as_ref().is_some_and(|(parent, _)| parent == path) {
            empty = false;
            self.new_folder_row(ui, depth + 1, ctx.out);
        }
        if empty {
            let (slot, _) = ui.allocate_exact_size(vec2(ui.available_width(), 24.0), Sense::hover());
            let x = slot.left() + 4.0 + depth as f32 * INDENT + 10.0;
            ui.painter().text(pos2(x, slot.center().y), Align2::LEFT_CENTER, "Empty", theme::sans(12.0), theme::MUTED);
        }
    }

    /// A section's header: its name small and muted, a chevron on the right, and on hover the +
    /// for a new plasmid in it when `add`. `key` is where a plasmid dropped on it goes.
    #[allow(clippy::too_many_arguments)]
    fn header(
        &mut self,
        ui: &mut Ui,
        key: &str,
        name: &str,
        glyph: Option<(&[icon::Node], Color32)>,
        open: bool,
        depth: usize,
        add: bool,
        ctx: &mut Rows<'_, '_>,
    ) -> HeaderPress {
        if ctx.started {
            ui.add_space(SECTION_GAP);
        }
        ctx.started = true;
        let (slot, response) = ui.allocate_exact_size(vec2(ui.available_width(), HEADER), Sense::click());
        let row = slot.shrink2(vec2(4.0, 1.0));
        ctx.folder_rows.push((row, key.to_string()));
        if !ui.is_rect_visible(slot) {
            return HeaderPress { toggle: false, add: false, menu: false };
        }
        let left = row.left() + depth as f32 * INDENT;
        // Over the header or its +, which sits on top of it and takes the hover from it.
        let hovered = response.contains_pointer();
        let dropping = self.dragging.is_some() && self.drop_on.as_deref() == Some(key);
        if dropping {
            ui.painter().rect_filled(row, radius(), theme::ROW_HOVER);
            ui.painter().rect_stroke(row, radius(), Stroke::new(1.0, theme::ACCENT), StrokeKind::Inside);
        }
        let p = ui.painter().with_clip_rect(row);
        let ink = if hovered { theme::INK } else { theme::MUTED };
        let chevron_at = pos2(row.right() - 10.0 - 6.0, row.center().y);
        icon::paint(&p, chevron_at, 12.0, if open { lucide::CHEVRON_DOWN } else { lucide::CHEVRON_RIGHT }, ink);
        let mut x = left + 10.0;
        if let Some((glyph, tint)) = glyph {
            icon::paint(&p, pos2(x + 6.0, row.center().y), 12.0, glyph, tint);
            x += 12.0 + 6.0;
        }
        let plus = Rect::from_center_size(pos2(chevron_at.x - 6.0 - 4.0 - 10.0, row.center().y), vec2(20.0, 20.0));
        let font = theme::sans_weight(12.5, Weight::Medium);
        let right = if add && hovered { plus.left() - 4.0 } else { chevron_at.x - 6.0 - 6.0 };
        p.text(pos2(x, row.center().y), Align2::LEFT_CENTER, fit_name(ui, name, &font, (right - x).max(0.0)), font, ink);
        let mut added = false;
        if add && hovered {
            let r = ui.interact(plus, ui.scope_id().with(("section-add", key)), Sense::click());
            if r.hovered() {
                p.rect_filled(plus, radius(), ink_wash(0.06));
            }
            icon::paint(&p, plus.center(), 13.0, lucide::PLUS, if r.hovered() { theme::INK } else { theme::MUTED });
            added = r.on_hover_text(format!("New plasmid in {name}")).clicked();
        }
        if hovered {
            ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
        }
        let menu = response.secondary_clicked();
        let tip = match key {
            LOOSE => "Plasmids in no folder",
            DELETED => "Click a plasmid to bring it back",
            folder => folder,
        };
        let toggle = response.on_hover_text(tip).clicked();
        HeaderPress { toggle: toggle && !added, add: added, menu }
    }

    fn plasmid_row(&mut self, ui: &mut Ui, listed: &Listed, depth: usize, ctx: &mut Rows<'_, '_>) {
        let (slot, response) = ui.allocate_exact_size(vec2(ui.available_width(), ROW), Sense::click_and_drag());
        let row = slot.shrink2(vec2(4.0, 1.0));
        let left = row.left() + depth as f32 * INDENT;
        if let Some((_, typing)) = self.renaming.as_mut().filter(|(t, _)| *t == Target::Plasmid(listed.id.clone())) {
            if typing.text.is_empty() && !typing.focused {
                typing.text = listed.name.clone();
            }
            let field = Rect::from_min_max(pos2(left + 4.0 + 10.0, row.center().y - 9.0), pos2(row.right() - 10.0, row.center().y + 9.0));
            if let Some(commit) = name_box(ui, field, typing, "") {
                let typed = typing.text.trim().to_string();
                self.renaming = None;
                if commit && !typed.is_empty() && typed != listed.name {
                    ctx.out.push(Chosen::Rename { id: listed.id.clone(), name: typed });
                }
            }
            return;
        }
        // Scrolled out of sight: the row keeps its place in the list and draws nothing. A long
        // project is hundreds of rows, and painting them all was most of what a frame cost.
        if !ui.is_rect_visible(slot) && self.confirming.as_ref() != Some(&listed.id) {
            return;
        }
        let open = ctx.model.open == Some(&listed.id);
        let hovered = response.hovered();
        let dragged = self.dragging.as_ref() == Some(&listed.id);
        if open {
            ui.painter().rect_filled(row, radius(), theme::ROW_ACTIVE);
        } else if hovered {
            ui.painter().rect_filled(row, radius(), theme::ROW_HOVER);
        }
        // The row being dragged goes quiet rather than disappearing (`.is-dragging`, opacity .4).
        let fade = |c: Color32| if dragged { theme::alpha(c, 0.4) } else { c };
        let confirming = self.confirming.as_ref() == Some(&listed.id);
        let name_right = if confirming { row.center().x } else { row.right() - 10.0 };
        let p = ui.painter().with_clip_rect(row);
        let mark_at = pos2(left + 10.0 + 8.0, row.center().y);
        let mark_ink = fade(if open || hovered { theme::INK } else { theme::SIDE_GLYPH });
        match (listed.circular, open) {
            (true, true) => kind_icons::paint_plasmid_filled(&p, mark_at, theme::SIDE_ICON, mark_ink),
            (true, false) => kind_icons::paint(&p, mark_at, theme::SIDE_ICON, kind_icons::PLASMID, mark_ink),
            (false, _) => kind_icons::paint(&p, mark_at, theme::SIDE_ICON, kind_icons::LINEAR, mark_ink),
        }
        let font = theme::sans_weight(theme::SIDE_TEXT, if open { Weight::Book } else { Weight::Regular });
        let x = left + 10.0 + 16.0 + 10.0;
        let shown = fit_name(ui, &listed.name, &font, (name_right - x).max(0.0));
        p.text(pos2(x, row.center().y), Align2::LEFT_CENTER, shown, font, fade(theme::INK));
        if confirming {
            // The confirm takes the row: the destructive word is the one that has to be read.
            let half = Rect::from_min_max(pos2(row.center().x + 6.0, row.top()), pos2(row.right() - 6.0, row.bottom()));
            let w = (half.width() - 4.0) / 2.0;
            let yes = Rect::from_center_size(pos2(half.left() + w / 2.0, row.center().y), vec2(w, 21.0));
            let no = Rect::from_center_size(pos2(half.right() - w / 2.0, row.center().y), vec2(w, 21.0));
            if confirm_button(ui, yes, "Delete", true) {
                self.confirming = None;
                ctx.out.push(Chosen::Delete(listed.id.clone()));
            }
            if confirm_button(ui, no, "Keep", false) {
                self.confirming = None;
            }
            return;
        }
        if hovered {
            ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
        }
        if response.drag_started() && self.dragging.is_none() {
            self.dragging = Some(listed.id.clone());
        }
        if response.secondary_clicked() {
            *ctx.menu = Some(Nav::plasmid_menu(listed, ctx.folders));
        }
        if response.on_hover_text(&listed.name).clicked() {
            ctx.out.push(Chosen::Open { id: listed.id.clone(), span: None });
        }
    }

    fn new_folder_row(&mut self, ui: &mut Ui, depth: usize, out: &mut Vec<Chosen>) {
        let (slot, _) = ui.allocate_exact_size(vec2(ui.available_width(), ROW), Sense::hover());
        let row = slot.shrink2(vec2(4.0, 1.0));
        let left = row.left() + depth as f32 * INDENT;
        let Some((parent, typing)) = self.naming.as_mut() else {
            return;
        };
        let field = Rect::from_min_max(pos2(left + 4.0 + 10.0, row.center().y - 9.0), pos2(row.right() - 10.0, row.center().y + 9.0));
        if let Some(commit) = name_box(ui, field, typing, "Folder name") {
            let typed = typing.text.trim().to_string();
            let path = if parent.is_empty() { typed.clone() } else { format!("{parent}/{typed}") };
            self.naming = None;
            if commit && !typed.is_empty() {
                self.set_open(&path, true, out);
                out.push(Chosen::NewFolder(path));
            }
        }
    }

    /// Every plasmid whose name holds the query, flat, then what the project-wide search found in
    /// features and bases.
    fn hits(&mut self, ui: &mut Ui, model: &Model<'_>, query: &str, out: &mut Vec<Chosen>) {
        let wanted = query.trim().to_lowercase();
        let mut named: Vec<&Listed> = model.listing.iter().filter(|l| l.name.to_lowercase().contains(&wanted)).collect();
        named.sort_by(|a, b| natural(&a.name, &b.name));
        let deeper: Vec<&Found> = model.found.into_iter().flatten().filter(|f| f.span.is_some()).collect();
        if named.is_empty() && deeper.is_empty() {
            hint(ui, "Nothing matches that.");
            return;
        }
        let no_folders: Vec<String> = Vec::new();
        let mut rows: Vec<(Rect, String)> = Vec::new();
        let mut menu = None;
        let mut ctx = Rows { model, folders: &no_folders, out, folder_rows: &mut rows, menu: &mut menu, started: false };
        // Dragging out of a filtered list would move a row out of a list that is not the project.
        let dragging = self.dragging.take();
        for listed in named {
            self.plasmid_row(ui, listed, 0, &mut ctx);
        }
        self.dragging = dragging.filter(|_| false);
        if let Some(groups) = menu {
            self.open_menu(ui.ctx().pointer_interact_pos(), groups);
        }
        for hit in deeper {
            let (slot, response) = ui.allocate_exact_size(vec2(ui.available_width(), ROW + 12.0), Sense::click());
            let row = slot.shrink2(vec2(4.0, 1.0));
            if response.hovered() {
                ui.painter().rect_filled(row, radius(), theme::ROW_HOVER);
                ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
            }
            let p = ui.painter().with_clip_rect(row);
            let x = row.left() + 10.0 + 15.0 + 9.0;
            icon::paint(&p, pos2(row.left() + 10.0 + 7.5, row.top() + 15.0), 13.0, lucide::SEARCH, theme::MUTED);
            let width = row.right() - 10.0 - x;
            p.text(pos2(x, row.top() + 15.0), Align2::LEFT_CENTER, clip(ui, &hit.title, &theme::sans(13.0), width), theme::sans(13.0), theme::INK);
            p.text(pos2(x, row.top() + 30.0), Align2::LEFT_CENTER, clip(ui, &hit.detail, &theme::sans(11.0), width), theme::sans(11.0), theme::MUTED);
            if response.clicked() {
                out.push(Chosen::Open { id: hit.id.clone(), span: hit.span });
            }
        }
    }

    /// The native section with no web counterpart: what was deleted, within reach, so deleting is
    /// never the end of something. A section of its own at the end, shut until opened.
    fn deleted_rows(&mut self, ui: &mut Ui, model: &Model<'_>, ctx: &mut Rows<'_, '_>) {
        let open = self.deleted_open;
        let pressed = self.header(ui, DELETED, "Recently deleted", Some((lucide::TRASH_2, theme::MUTED)), open, 0, false, ctx);
        // Nothing can be dropped into what was deleted.
        ctx.folder_rows.pop();
        if pressed.toggle {
            self.deleted_open = !open;
        }
        if !self.deleted_open {
            return;
        }
        let out = &mut *ctx.out;
        for gone in model.deleted.iter().rev() {
            let (slot, response) = ui.allocate_exact_size(vec2(ui.available_width(), ROW), Sense::click());
            let row = slot.shrink2(vec2(4.0, 1.0));
            if response.hovered() {
                ui.painter().rect_filled(row, radius(), theme::ROW_HOVER);
                ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
            }
            let p = ui.painter().with_clip_rect(row);
            let left = row.left();
            kind_icons::paint(&p, pos2(left + 10.0 + 7.5, row.center().y), 15.0, kind_icons::PLASMID, theme::MUTED);
            let x = left + 10.0 + 15.0 + 9.0;
            let font = theme::sans(13.0);
            p.text(
                pos2(x, row.center().y),
                Align2::LEFT_CENTER,
                fit_name(ui, &gone.name, &font, (row.right() - 10.0 - x).max(0.0)),
                font,
                theme::MUTED,
            );
            let folder = gone.path.parent().filter(|p| !p.as_os_str().is_empty()).map(slash);
            let tip = folder.map_or_else(|| "Click to bring it back".to_string(), |folder| format!("Click to bring it back to {folder}"));
            if response.on_hover_text(tip).clicked() {
                out.push(Chosen::Restore(gone.id.clone()));
            }
        }
    }

    /// A plasmid dragged onto a folder moves into it; let go over the list's own ground and it
    /// goes to the top; let go anywhere else and nothing happens.
    fn drag(&mut self, ui: &Ui, body: Rect, folders: &[(Rect, String)], out: &mut Vec<Chosen>) {
        let Some(id) = self.dragging.clone() else {
            return;
        };
        let pointer = ui.ctx().pointer_interact_pos();
        self.drop_on = pointer.and_then(|at| {
            folders.iter().find(|(r, _)| r.contains(at)).map(|(_, path)| path.clone()).or_else(|| body.contains(at).then(String::new))
        });
        ui.ctx().set_cursor_icon(CursorIcon::Grabbing);
        if ui.input(|i| i.pointer.any_released()) {
            if let Some(folder) = self.drop_on.take() {
                out.push(Chosen::MoveTo { id, folder });
            }
            self.dragging = None;
        }
    }
}

/// What the rows need from the frame that draws them.
struct Rows<'m, 'a> {
    model: &'m Model<'a>,
    folders: &'m [String],
    out: &'m mut Vec<Chosen>,
    folder_rows: &'m mut Vec<(Rect, String)>,
    menu: &'m mut Option<Vec<Vec<Item>>>,
    /// Something is drawn already, so the next section keeps its distance.
    started: bool,
}

/// What was pressed on a section's header.
struct HeaderPress {
    toggle: bool,
    add: bool,
    menu: bool,
}

/// `.rail-hint`: 12px, muted, padded 10 16 14.
fn hint(ui: &mut Ui, text: &str) {
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 10.0 + 18.0 + 14.0), Sense::hover());
    ui.painter().text(pos2(rect.left() + 16.0, rect.top() + 10.0 + 9.0), Align2::LEFT_CENTER, text, theme::sans(12.0), theme::MUTED);
}

/// `.rail-file-confirm-yes` / `-no`.
fn confirm_button(ui: &mut Ui, rect: Rect, text: &str, danger: bool) -> bool {
    let response = ui.interact(rect, ui.scope_id().with(("rail-confirm", text)), Sense::click());
    let (fill, edge, ink) = if danger {
        (theme::DANGER_BG, theme::DANGER_BORDER, theme::DANGER_TEXT)
    } else {
        (Color32::TRANSPARENT, theme::HAIRLINE_STRONG, theme::INK)
    };
    ui.painter().rect_filled(rect, radius(), fill);
    ui.painter().rect_stroke(rect, radius(), Stroke::new(1.0, edge), StrokeKind::Inside);
    ui.painter().text(rect.center(), Align2::CENTER_CENTER, text, theme::sans(11.0), ink);
    if response.hovered() {
        ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
    }
    response.clicked()
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use bios_project::{ConstructId, Listed};

    use super::{Node, by_name, natural, split_name, tree};

    fn listed(name: &str, path: &str) -> Listed {
        Listed {
            id: ConstructId::parse(name).unwrap(),
            path: PathBuf::from(path),
            name: name.into(),
            length: 10,
            circular: true,
            features: 0,
            version: 1,
            sha: String::new(),
            derived_from: None,
        }
    }

    #[test]
    fn a_long_name_keeps_its_end_from_the_last_separator_within_reach() {
        assert_eq!(split_name("pDemo_backbone_v1"), ("pDemo_backbone_v1", ""));
        assert_eq!(split_name("pDemo01_LacI_GFP_reporter_v2"), ("pDemo01_LacI_GFP_reporter", "_v2"));
        // No separator near the end: a fixed tail of eight.
        assert_eq!(split_name("abcdefghijklmnopqrstuvwxyz"), ("abcdefghijklmnopqr", "stuvwxyz"));
    }

    #[test]
    fn names_sort_with_their_numbers_in_order() {
        let mut names = vec!["pDemo_10", "pDemo_9", "Alpha", "beta", "pDemo_8"];
        names.sort_by(|a, b| natural(a, b));
        assert_eq!(names, ["Alpha", "beta", "pDemo_8", "pDemo_9", "pDemo_10"]);
    }

    #[test]
    fn folders_come_first_and_a_folder_only_a_path_names_is_drawn() {
        let listing = vec![listed("zeta", "zeta.gb"), listed("alpha", "deep/inner/alpha.gb"), listed("beta", "beta.gb")];
        let nodes = tree(&listing, &[PathBuf::from("empty")], &by_name(&mut Default::default(), &listing));
        let names: Vec<String> = nodes
            .iter()
            .map(|n| match n {
                Node::Folder { name, .. } => format!("{name}/"),
                Node::Plasmid(l) => l.name.clone(),
            })
            .collect();
        assert_eq!(names, ["deep/", "empty/", "beta", "zeta"]);
        let Some(Node::Folder { children, .. }) = nodes.first() else { panic!("no folder") };
        assert!(matches!(children.first(), Some(Node::Folder { path, .. }) if path == "deep/inner"));
    }
}
