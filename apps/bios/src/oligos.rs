//! Oligos held against the open plasmid without landing in it: the binders (`editor/binders.ts`,
//! `workspace/useWorkspace.tsx` `addBinderFromSelection`, `addBinder`, `removeBinder`), and Find's
//! alignment of a query (`editor/find/useProbeHits.ts`, `find/worker.ts`). And one thing that is
//! in the file: a made primer's end, dragged with its Tm live (`editor/primer/PrimerDrag.tsx`).
//!
//! A binder is window state, like the selection: it is held per plasmid for as long as the window
//! is open and never reaches the file, so nothing here goes through a command that writes. Where it
//! lands is asked of the sequence on screen every time it changes (the same question the
//! `binder_sites` command answers for an agent), so an off-target site cannot go stale.

use std::sync::mpsc::{Receiver, Sender, channel};
use std::time::{Duration, Instant};

use bios_core::Construct;
use bios_core::align::ALIGN_DEFAULTS;
use bios_core::binders::{Binder, BinderSite, binder_from_bases, binder_from_selection, binders_to_sites};
use bios_core::find_align::{AlignHit, CELL_BUDGET, FIND_LIVE_NT, FIND_MAX_NT, FIND_MIN_NT, alignment_hits, cells_for, query_limit};
use bios_project::ConstructId;

use crate::App;

/// The binders held in this window, and where those of the open plasmid land.
#[derive(Default)]
pub(crate) struct Oligos {
    /// Every binder held, with the plasmid it is held against.
    held: Vec<(ConstructId, Binder)>,
    /// Numbers the next binder: unique for the window's life, so a name is never reused.
    made: u64,
    /// Bumped whenever `held` changes.
    changed: u64,
    /// The open plasmid's binder sites, and what they were worked out for.
    sites: Vec<BinderSite>,
    sites_for: Option<(ConstructId, u64, u64)>,
    /// Find's alignment of its query, worked out off the window's thread.
    pub(crate) aligner: Aligner,
    /// `--hover`: where the pointer is held, for a picture.
    pub(crate) hover_at: Option<egui::Pos2>,
    /// A made primer's end being dragged.
    drag: Option<PrimerDrag>,
}

/// A made primer being lengthened or shortened by one of its ends.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct PrimerDrag {
    /// Which of the construct's features.
    feature: usize,
    /// Its start (left) end, or its end (right) one.
    start: bool,
    /// Its footprint when the drag began, and now; 0-based, inclusive.
    was: (usize, usize),
    now: (usize, usize),
}

/// Where a drag to `gap` puts the footprint `was` (`PrimerDrag.tsx` `onMove`): the end being
/// dragged follows the pointer base by base, never closer than one base to the other end and never
/// off the construct.
fn dragged(was: (usize, usize), start: bool, gap: usize, length: usize) -> (usize, usize) {
    let (s, e) = was;
    if start {
        (gap.min(e.saturating_sub(1)), e)
    } else {
        let last = length.saturating_sub(1);
        (s, gap.saturating_sub(1).max(s + 1).min(last))
    }
}

/// `x,y` as a point in the window.
pub(crate) fn point(xy: &str) -> Option<egui::Pos2> {
    let (x, y) = xy.split_once(',')?;
    Some(egui::pos2(x.trim().parse().ok()?, y.trim().parse().ok()?))
}

/// What an alignment is for: the revision of what is drawn, and the query's bases.
type Asked = (u64, String);

/// One alignment's answer, and what it answers.
struct Answer {
    asked: Asked,
    hits: Vec<AlignHit>,
}

/// Find's aligner. Every query is aligned on a thread of its own, never the window's, so a long
/// paste never freezes it. Once a thread is inside the aligner it cannot be stopped, so, as on the
/// web, a stale answer is only ever dropped, and the real defence against typing is the wait: a
/// short query runs on every keystroke, a long one once the typing stops.
pub(crate) struct Aligner {
    send: Sender<Answer>,
    receive: Receiver<Answer>,
    /// What the bar wants aligned now, and when it may be sent.
    wanted: Option<Asked>,
    due: Option<Instant>,
    /// What was last sent to a thread.
    sent: Option<Asked>,
    answer: Option<Answer>,
    /// Bumped whenever the hits change, for the rows.
    pub(crate) changed: u64,
}

impl Default for Aligner {
    fn default() -> Self {
        let (send, receive) = channel();
        Aligner { send, receive, wanted: None, due: None, sent: None, answer: None, changed: 0 }
    }
}

/// How long a long query waits for the typing to stop.
const DEBOUNCE: Duration = Duration::from_millis(120);

impl Aligner {
    /// Asks for `bases` (cleaned) to be aligned against `c` as it is at `revision`, or for nothing.
    /// Returns true when a new answer arrived this frame.
    pub(crate) fn want(&mut self, ctx: &egui::Context, c: &Construct, revision: u64, bases: Option<String>) -> bool {
        let mut arrived = false;
        while let Ok(answer) = self.receive.try_recv() {
            if Some(&answer.asked) == self.sent.as_ref() {
                self.answer = Some(answer);
                self.changed += 1;
                arrived = true;
            }
        }
        let wanted = bases.filter(|b| alignable(c, b)).map(|b| (revision, b));
        if wanted != self.wanted {
            // Switching to Exact (or back) shows or hides the held answer without a new one
            // arriving; the rows key on `changed`, so without this they keep drawing the old fit.
            if wanted.is_some() != self.wanted.is_some() {
                self.changed += 1;
            }
            let long = wanted.as_ref().is_some_and(|(_, b)| b.len() > FIND_LIVE_NT);
            self.due = wanted.as_ref().map(|_| Instant::now() + if long { DEBOUNCE } else { Duration::ZERO });
            self.wanted = wanted;
        }
        if let (Some(wanted), Some(due)) = (&self.wanted, self.due) {
            if self.answered(wanted) || self.sent.as_ref() == Some(wanted) {
                self.due = None;
            } else if Instant::now() >= due {
                self.due = None;
                self.sent = Some(wanted.clone());
                let (send, asked, sequence, circular, ctx) = (self.send.clone(), wanted.clone(), c.sequence.clone(), c.circular, ctx.clone());
                std::thread::spawn(move || {
                    let hits = alignment_hits(&sequence, circular, &asked.1);
                    // The window may have closed; then there is no one to tell.
                    let _ = send.send(Answer { asked, hits });
                    ctx.request_repaint();
                });
            } else {
                ctx.request_repaint_after(due.saturating_duration_since(Instant::now()));
            }
        }
        arrived
    }

    fn answered(&self, asked: &Asked) -> bool {
        self.answer.as_ref().is_some_and(|a| &a.asked == asked)
    }

    /// The hits to draw and walk. A changed QUERY keeps the last fit drawn while the next is worked
    /// out, so the rows do not flash empty between keystrokes; a changed CONSTRUCT clears at once,
    /// because hits against other bases are wrong, not stale.
    pub(crate) fn hits(&self, revision: u64) -> &[AlignHit] {
        match (&self.answer, &self.wanted) {
            (Some(a), Some(w)) if a.asked.0 == revision && w.0 == revision => &a.hits,
            _ => &[],
        }
    }

    /// An alignment is outstanding for what is wanted.
    pub(crate) fn pending(&self) -> bool {
        self.wanted.as_ref().is_some_and(|w| !self.answered(w))
    }

    /// Lets go of everything: the bar closed, and a held oligo that outlives its query is a
    /// drawing nobody asked for.
    pub(crate) fn clear(&mut self) {
        if self.wanted.is_some() || self.answer.is_some() {
            self.changed += 1;
        }
        self.wanted = None;
        self.due = None;
        self.answer = None;
    }

    /// The answer stopped at the most the aligner reports.
    pub(crate) fn capped(&self, revision: u64) -> bool {
        self.hits(revision).len() >= ALIGN_DEFAULTS.max_results
    }
}

/// Whether `bases` can be aligned against `c`: at least six bases, and within the cell budget.
pub(crate) fn alignable(c: &Construct, bases: &str) -> bool {
    bases.len() >= FIND_MIN_NT && bases.len() <= FIND_MAX_NT && cells_for(c.len(), bases.len(), c.circular) <= CELL_BUDGET
}

/// The longest query `c` admits, when `bases` is longer.
pub(crate) fn too_long(c: &Construct, bases: &str) -> Option<usize> {
    (bases.len() >= FIND_MIN_NT && !alignable(c, bases)).then(|| query_limit(c.len(), c.circular))
}

impl Oligos {
    /// The binders held against `id`, in the order they were made.
    pub(crate) fn binders_of<'a>(&'a self, id: &'a ConstructId) -> impl Iterator<Item = &'a Binder> + 'a {
        self.held.iter().filter(move |(of, _)| of == id).map(|(_, b)| b)
    }

    /// The open plasmid's binder sites as last worked out by [`Oligos::refresh`].
    pub(crate) fn sites(&self) -> &[BinderSite] {
        &self.sites
    }

    /// Works out where the open plasmid's binders land, again only when the binders or what is
    /// drawn changed.
    pub(crate) fn refresh(&mut self, id: Option<&ConstructId>, c: &Construct, revision: u64) {
        let Some(id) = id else {
            self.sites.clear();
            self.sites_for = None;
            return;
        };
        let key = (id.clone(), revision, self.changed);
        if self.sites_for.as_ref() != Some(&key) {
            let binders: Vec<Binder> = self.binders_of(id).cloned().collect();
            self.sites = binders_to_sites(&binders, &c.sequence, c.circular);
            self.sites_for = Some(key);
        }
    }

    /// What the rows need to know to lay out again: the binders held or Find's hits changed.
    pub(crate) fn key(&self) -> u64 {
        self.changed.wrapping_mul(1_000_003).wrapping_add(self.aligner.changed)
    }

    /// The site under the pointer, by its place in the last [`Oligos::sites`], and all the sites of
    /// its binder with the binder's place among the open plasmid's.
    pub(crate) fn binder_at(&self, id: &ConstructId, site: usize) -> Option<(usize, Vec<BinderSite>)> {
        let binder = self.sites.get(site)?.binder_id.clone();
        let index = self.binders_of(id).position(|b| b.id == binder)?;
        Some((index, self.sites.iter().filter(|s| s.binder_id == binder).cloned().collect()))
    }

    fn hold(&mut self, id: &ConstructId, binder: Binder) {
        self.held.push((id.clone(), binder));
        self.changed += 1;
    }

    fn next(&mut self) -> u64 {
        self.made += 1;
        self.made
    }
}

impl App {
    /// Holds the oligo that anneals to `start..=end` against the open plasmid: the reverse
    /// complement of what was selected. A selection through the origin makes none, as on the web.
    pub(crate) fn add_binder_from_selection(&mut self, start: usize, end: usize) {
        let Some(opened) = &self.opened else {
            return;
        };
        let number = self.ed.oligos.next();
        if let Some(binder) = binder_from_selection(&opened.construct.sequence, start, end, number) {
            self.ed.oligos.hold(&opened.id, binder);
        }
    }

    /// Holds an oligo someone handed over (a kept Find query) against the open plasmid.
    pub(crate) fn add_binder(&mut self, bases: &str) {
        let Some(id) = self.opened.as_ref().map(|o| o.id.clone()) else {
            return;
        };
        let number = self.ed.oligos.next();
        if let Some(binder) = binder_from_bases(bases, None, number) {
            self.ed.oligos.hold(&id, binder);
        }
    }

    /// Takes back the open plasmid's binder at this place in its list, every site of it.
    pub(crate) fn remove_binder(&mut self, index: usize) {
        let Some(id) = self.opened.as_ref().map(|o| o.id.clone()) else {
            return;
        };
        let Some(binder) = self.ed.oligos.binders_of(&id).nth(index).map(|b| b.id.clone()) else {
            return;
        };
        self.ed.oligos.held.retain(|(of, b)| !(of == &id && b.id == binder));
        self.ed.oligos.changed += 1;
    }
}

impl App {
    /// `--binder <from>..<to>`: holds the binder for those bases (1-based), for a picture.
    pub(crate) fn flag_binder(&mut self, range: &str) {
        let length = self.opened.as_ref().map_or(0, |o| o.construct.len());
        let ends = range.split_once("..").and_then(|(from, to)| Some((from.trim().parse::<usize>().ok()?, to.trim().parse::<usize>().ok()?)));
        match ends {
            Some((from, to)) if from >= 1 && from <= to && to <= length => self.add_binder_from_selection(from - 1, to - 1),
            _ => self.problem = Some(format!("--binder takes two bases of the open plasmid, as 120..140; it has {length}")),
        }
    }
}

impl App {
    /// A press on a made primer's end: the drag starts from its footprint as it is.
    pub(crate) fn begin_primer_drag(&mut self, feature: usize, start: bool) {
        let Some(f) = self.opened.as_ref().and_then(|o| o.construct.features.get(feature)) else {
            return;
        };
        if f.kind != "primer_bind" || f.end < f.start {
            return;
        }
        self.ed.anchor = None;
        self.ed.oligos.drag = Some(PrimerDrag { feature, start, was: (f.start, f.end), now: (f.start, f.end) });
    }

    /// The pointer moved to `gap` with a primer's end held: the primer is drawn at its new length
    /// at once, so its card's Tm, GC and uniqueness are worked out again under the pointer. Nothing
    /// is written until the button comes up. False when no end is held.
    pub(crate) fn drag_primer(&mut self, gap: usize) -> bool {
        let (Some(drag), Some(opened)) = (self.ed.oligos.drag.as_mut(), self.opened.as_mut()) else {
            return false;
        };
        let now = dragged(drag.was, drag.start, gap, opened.construct.len());
        if now != drag.now
            && let Some(f) = opened.construct.features.get_mut(drag.feature)
        {
            (f.start, f.end) = now;
            drag.now = now;
            self.revision += 1;
        }
        true
    }

    /// While an end is held: the primer's card, its Tm live. When the button comes up the new
    /// footprint is written through `update_primer`, once, as one step in the history.
    pub(crate) fn primer_drag_frame(&mut self, ctx: &egui::Context) {
        let Some(drag) = self.ed.oligos.drag else {
            return;
        };
        if ctx.input(|i| i.pointer.primary_down()) {
            ctx.set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
            // Over its own box the card already sits by the box, as the web's does all through a
            // drag (`PrimerCard.tsx` hit-tests the box each frame); off it, it stays by the pointer.
            let on_box = bios_ui::rows::seen(ctx).bar.is_some_and(|(i, _)| i == drag.feature);
            if !on_box
                && let (Some(opened), Some(at)) = (&self.opened, ctx.input(|i| i.pointer.hover_pos()))
                && let Some(facts) = bios_ui::primer_card::PrimerFacts::of(&opened.construct, drag.feature)
            {
                bios_ui::primer_card::show(ctx, &facts, egui::Rect::from_center_size(at, egui::vec2(8.0, 18.0)));
            }
            return;
        }
        self.ed.oligos.drag = None;
        if drag.now == drag.was {
            return;
        }
        // Moving names the version it is made on, so what was typed is saved first.
        if !self.save() {
            return;
        }
        let Some(opened) = &self.opened else {
            return;
        };
        let params = serde_json::json!({
            "plasmid": opened.id.as_str(),
            "version": opened.token,
            "primer": format!("#{}", drag.feature + 1),
            "start": drag.now.0 + 1,
            "end": drag.now.1 + 1,
        });
        if self.run("update_primer", params).is_none() {
            // Refused: the primer goes back to where the project has it.
            self.refresh();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::dragged;

    #[test]
    fn a_dragged_end_follows_the_pointer_and_never_crosses_the_other_or_leaves_the_construct() {
        // Footprint 100..119 on a 1,000 bp construct.
        assert_eq!(dragged((100, 119), true, 95, 1000), (95, 119));
        assert_eq!(dragged((100, 119), true, 130, 1000), (118, 119));
        assert_eq!(dragged((100, 119), false, 125, 1000), (100, 124));
        assert_eq!(dragged((100, 119), false, 50, 1000), (100, 101));
        assert_eq!(dragged((100, 119), false, 5000, 1000), (100, 999));
        assert_eq!(dragged((100, 119), true, 0, 1000), (0, 119));
    }
}
