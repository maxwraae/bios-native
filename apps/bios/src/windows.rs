//! More than one Bios window on the same project, in the same process (native only, PARITY.md).
//!
//! Every window is a whole Bios window: its own sidebar, place, tabs and editors (split too). They
//! share the project, the working copies (editor.rs) and everything else that is App's. The window
//! being worked on, `App::active`, has its state in App's own fields (`ed`, `split`, `place`,
//! `nav`, …); every other window's waits in a [`Shell`] in `App::windows`. To draw another window,
//! or to act in it, its shell is swapped in and back out, the way a parked editor is: so the code
//! that draws one window draws them all. What a window's own keys, presses and in-window menus do
//! acts on it, since it is swapped in while it is drawn; the system menu bar acts on the window
//! the system has in front.

use bios_project::ConstructId;
use bios_ui::session_nav::{self, Nav};
use bios_ui::sidebar::Place;
use egui::ViewportId;

use crate::App;
use crate::editor::{Editor, Split};

/// One window's own state, while another window is the one being worked on.
pub(crate) struct Shell {
    pub(crate) ed: Editor,
    pub(crate) split: Option<Split>,
    pub(crate) place: Place,
    pub(crate) nav: Nav,
    pub(crate) query: String,
    pub(crate) searched: String,
    pub(crate) found: Option<Vec<session_nav::Found>>,
    pub(crate) focus_search: bool,
    pub(crate) panel_hidden: bool,
    pub(crate) panel_width: f32,
    pub(crate) changes_shown: bool,
    pub(crate) work_hidden: bool,
}

/// A window that is not the one in App's own fields.
pub(crate) struct Window {
    pub(crate) id: ViewportId,
    pub(crate) shell: Shell,
}

impl App {
    /// Trades the state of the window in App's fields for `shell`'s, parking and taking up the
    /// editors' working copies on the way.
    fn swap_shell(&mut self, shell: &mut Shell) {
        self.park();
        std::mem::swap(&mut self.ed, &mut shell.ed);
        std::mem::swap(&mut self.split, &mut shell.split);
        std::mem::swap(&mut self.place, &mut shell.place);
        std::mem::swap(&mut self.nav, &mut shell.nav);
        std::mem::swap(&mut self.query, &mut shell.query);
        std::mem::swap(&mut self.searched, &mut shell.searched);
        std::mem::swap(&mut self.found, &mut shell.found);
        std::mem::swap(&mut self.focus_search, &mut shell.focus_search);
        std::mem::swap(&mut self.panel_hidden, &mut shell.panel_hidden);
        std::mem::swap(&mut self.panel_width, &mut shell.panel_width);
        std::mem::swap(&mut self.changes_shown, &mut shell.changes_shown);
        std::mem::swap(&mut self.work_hidden, &mut shell.work_hidden);
        self.unpark();
    }

    /// Puts window `id`'s state in App's fields, saving nothing: to draw it, or to act in it. A
    /// window that is gone is left alone.
    pub(crate) fn enter(&mut self, id: ViewportId) {
        if self.here == id {
            return;
        }
        let Some(at) = self.windows.iter().position(|w| w.id == id) else {
            return;
        };
        let mut window = self.windows.remove(at);
        self.swap_shell(&mut window.shell);
        window.id = std::mem::replace(&mut self.here, id);
        self.windows.insert(at, window);
    }

    /// Every window open, the main one first.
    pub(crate) fn window_ids(&self) -> Vec<ViewportId> {
        let mut ids: Vec<ViewportId> = std::iter::once(self.here).chain(self.windows.iter().map(|w| w.id)).collect();
        ids.sort_by_key(|id| (*id != ViewportId::ROOT, self.window_order(*id)));
        ids
    }

    /// When a window was made, for keeping them in one order however their state moves about.
    fn window_order(&self, id: ViewportId) -> u64 {
        self.window_made.iter().find(|(made, _)| *made == id).map_or(0, |(_, n)| *n)
    }

    /// The plasmid in front of window `id`'s editor being worked on.
    fn front_of(&self, id: ViewportId) -> Option<ConstructId> {
        if id == self.here {
            return self.opened.as_ref().map(|o| o.id.clone());
        }
        self.windows.iter().find(|w| w.id == id).and_then(|w| w.shell.ed.front.clone())
    }

    /// Makes window `id` the one worked on: the system menus act in it. What was typed into a
    /// plasmid it does not show is saved first, as leaving a tab saves. False when there is no
    /// such window.
    pub(crate) fn activate(&mut self, id: ViewportId) -> bool {
        if self.active == id {
            return true;
        }
        if id != self.here && !self.windows.iter().any(|w| w.id == id) {
            return false;
        }
        if self.front_of(id) != self.opened.as_ref().map(|o| o.id.clone()) {
            self.save();
        }
        self.enter(id);
        self.active = id;
        true
    }

    /// Opens a second (or third) Bios window on this project, with `plasmid` in front of it, and
    /// works in it. The same plasmid in both windows is one working copy.
    pub(crate) fn open_in_new_window(&mut self, plasmid: Option<ConstructId>) {
        if self.session.is_none() {
            return;
        }
        let same = plasmid.is_some() && plasmid == self.opened.as_ref().map(|o| o.id.clone());
        let mut ed = self.new_editor();
        if let Some(id) = plasmid {
            ed.tabs.push(id.clone());
            ed.front = Some(id);
            ed.spot = self.opened.as_ref().filter(|_| same).map(|o| o.editing.spot());
        }
        let mut nav = Nav::default();
        let root = self.session.as_ref().map(|s| s.project().root().to_path_buf());
        if let (Some(store), Some(root)) = (&self.prefs, root) {
            nav.arrive(store.get().projects.shut_folders_of(&root.canonicalize().unwrap_or(root)));
        }
        let id = ViewportId::from_hash_of(("bios-window", ed.uid));
        let shell = Shell {
            ed,
            split: None,
            place: Place::Edit,
            nav,
            query: String::new(),
            searched: String::new(),
            found: None,
            focus_search: false,
            panel_hidden: self.panel_hidden,
            panel_width: self.panel_width,
            changes_shown: false,
            work_hidden: false,
        };
        self.window_made.push((id, self.editors_made));
        self.windows.push(Window { id, shell });
        self.activate(id);
    }

    /// Closes window `id`, which is not the main one. What was typed in it has been saved; the
    /// main window becomes the one worked on when this one was.
    pub(crate) fn close_window(&mut self, id: ViewportId) {
        if id == ViewportId::ROOT {
            return;
        }
        if self.here == id {
            self.enter(ViewportId::ROOT);
        }
        if self.active == id {
            self.active = self.here;
        }
        self.windows.retain(|w| w.id != id);
        self.window_made.retain(|(made, _)| *made != id);
        self.prune_copies();
    }

    /// Back to one window, the main one, for another project or none. Nothing is saved: whoever
    /// asks has saved already.
    pub(crate) fn one_window(&mut self) {
        self.enter(ViewportId::ROOT);
        self.active = self.here;
        self.windows.clear();
        self.window_made.retain(|(id, _)| *id == ViewportId::ROOT);
    }

    /// The window the system has in front, of Bios's.
    pub(crate) fn focused_window(ctx: &egui::Context) -> Option<ViewportId> {
        ctx.input(|i| i.raw.viewports.iter().find(|(_, info)| info.focused == Some(true)).map(|(id, _)| *id))
    }
}
