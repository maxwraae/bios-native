//! What acts on what, drawn across the wrapped rows. Ported from Bios
//! `editor/rows/RowInteractions.tsx` (`interactionsForRow` and the drawing's geometry).
//!
//! Each row is drawn on its own, so two features that wrap to different rows cannot be joined by
//! one line. A relationship draws as whichever of these is true:
//!
//! - both ends in this row: a bracket from one to the other, arrowhead on the thing acted on;
//! - one end in this row: a stub at that end, labelled with the partner, pointing the way it lies
//!   along the molecule.
//!
//! The whole construct is never an endpoint here: a statement about the molecule is real and shows
//! on the card, but a line from almost every feature to nothing in particular is not a drawing.

use crate::rows::{PACK_GAP, Row, pack_lanes, span_in_row, track, unwrap};

/// Roughly how wide 11px of the interface face runs, per character.
pub const LABEL_CHAR_PX: f64 = 5.8;
/// How far a stub reaches toward the partner it cannot draw.
pub const STUB: f64 = 16.0;
/// The bracket's soft corner.
const R: f64 = 4.0;

/// Where a feature sits, for the ends of an edge. 0-based inclusive.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct EdgeSpan<'a> {
    pub start: i64,
    pub end: i64,
    pub name: &'a str,
}

/// One relationship: feature `actor` acting on feature `target`, or on the construct (`None`).
/// Both are indices into the spans handed to [`interactions_for_row`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RowEdge<'a> {
    pub actor: usize,
    pub target: Option<usize>,
    pub verb: &'a str,
}

/// One edge placed in one row.
#[derive(Clone, Debug, PartialEq)]
pub struct EdgePiece {
    /// Which of the edges handed in this is.
    pub edge: usize,
    pub actor: usize,
    pub target: usize,
    pub lane: usize,
    /// x of each end in this row, or `None` when that end wrapped to another row.
    pub actor_x: Option<f64>,
    pub target_x: Option<f64>,
    /// Which way the absent end lies along the molecule: -1 earlier, 1 later.
    pub away: f64,
    /// The verb when both ends are here, else the verb and the partner.
    pub label: String,
    /// The horizontal room the label and the drawing need, for lane packing.
    pub x: f64,
    pub width: f64,
}

fn mid_x(span: &EdgeSpan, row: Row, char_width: f64, seq_len: i64) -> Option<f64> {
    unwrap(span.start, span.end, seq_len).into_iter().find_map(|(from, to)| span_in_row(from, to, row, char_width)).map(|p| p.x_start + p.width / 2.0)
}

fn chars(s: &str) -> f64 {
    s.chars().count() as f64
}

/// The edges that touch this row, placed and packed into lanes, and how many lanes they need.
pub fn interactions_for_row(edges: &[RowEdge], spans: &[Option<EdgeSpan>], row: Row, char_width: f64, seq_len: i64) -> (Vec<EdgePiece>, usize) {
    let mut placed = Vec::new();
    for (i, e) in edges.iter().enumerate() {
        let Some(target) = e.target else { continue };
        let (Some(Some(a)), Some(Some(t))) = (spans.get(e.actor), spans.get(target)) else { continue };
        let actor_x = mid_x(a, row, char_width, seq_len);
        let target_x = mid_x(t, row, char_width, seq_len);
        let away = if t.start < a.start { -1.0 } else { 1.0 };
        let (label, x, width) = match (actor_x, target_x) {
            (None, None) => continue,
            (Some(ax), Some(tx)) => (e.verb.to_string(), ax.min(tx), (tx - ax).abs().max(chars(e.verb) * LABEL_CHAR_PX)),
            (here_a, _) => {
                // The end that IS here says what it is wired to: the other end is off the row.
                let label = if here_a.is_some() { format!("{} {}", e.verb, t.name) } else { format!("{} {}", a.name, e.verb) };
                let here = actor_x.or(target_x).unwrap_or_default();
                let reach = STUB + chars(&label) * LABEL_CHAR_PX;
                // The drawing lies on the partner's side either way.
                let dir = if here_a.is_some() { away } else { -away };
                (label, if dir < 0.0 { here - reach } else { here }, reach)
            }
        };
        placed.push(EdgePiece { edge: i, actor: e.actor, target, lane: 0, actor_x, target_x, away, label, x, width });
    }
    let items: Vec<(f64, f64)> = placed.iter().map(|p| (p.x, p.width)).collect();
    let (lanes, count) = pack_lanes(&items, PACK_GAP);
    for (p, lane) in placed.iter_mut().zip(lanes) {
        p.lane = lane;
    }
    (placed, count)
}

/// Where the text of a piece is anchored.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Anchor {
    Start,
    Middle,
    End,
}

/// A piece as drawn, in the row's pixels: the line, the arrowhead when the thing acted on is in
/// this row, and where the label stands.
#[derive(Clone, Debug, PartialEq)]
pub struct EdgeDrawing {
    pub line: Vec<(f64, f64)>,
    /// A downward arrowhead: the thing acted on is underneath it.
    pub head: Option<[(f64, f64); 3]>,
    pub label_at: (f64, f64),
    pub anchor: Anchor,
}

/// A quadratic from `a` through control `c` to `b`, as points after `a`.
fn quad(out: &mut Vec<(f64, f64)>, a: (f64, f64), c: (f64, f64), b: (f64, f64)) {
    const STEPS: usize = 6;
    for k in 1..=STEPS {
        let t = k as f64 / STEPS as f64;
        let u = 1.0 - t;
        out.push((u * u * a.0 + 2.0 * u * t * c.0 + t * t * b.0, u * u * a.1 + 2.0 * u * t * c.1 + t * t * b.1));
    }
}

/// The piece's drawing in its lane, the lanes' band starting at `offset_y`.
pub fn drawing(p: &EdgePiece, offset_y: f64) -> EdgeDrawing {
    let top = offset_y + p.lane as f64 * track::INTERACTIONS + 5.0;
    let base = offset_y + (p.lane as f64 + 1.0) * track::INTERACTIONS - 2.0;
    let head = |x: f64| [(x - 3.5, base - 5.0), (x + 3.5, base - 5.0), (x, base)];
    let mut line = Vec::new();
    match (p.actor_x, p.target_x) {
        (Some(a), Some(t)) => {
            // A bracket with soft corners: up off the actor, across, down onto the target.
            let dir = if t > a { 1.0 } else { -1.0 };
            let r = R.min((t - a).abs() / 2.0);
            line.push((a, base));
            line.push((a, top + r));
            quad(&mut line, (a, top + r), (a, top), (a + dir * r, top));
            line.push((t - dir * r, top));
            quad(&mut line, (t - dir * r, top), (t, top), (t, top + r));
            line.push((t, base));
            EdgeDrawing { line, head: Some(head(t)), label_at: ((a + t) / 2.0, top - 3.0), anchor: Anchor::Middle }
        }
        (Some(a), None) => {
            // The actor, reaching toward a target that wrapped away.
            let to = a + p.away * STUB;
            line.push((a, base));
            line.push((a, top + R));
            quad(&mut line, (a, top + R), (a, top), (a + p.away * R, top));
            line.push((to, top));
            let anchor = if p.away < 0.0 { Anchor::End } else { Anchor::Start };
            EdgeDrawing { line, head: None, label_at: (to + p.away * 3.0, top - 3.0), anchor }
        }
        (None, Some(t)) => {
            // The target, with the actor's reach coming in from wherever the actor lies.
            let from = t - p.away * STUB;
            line.push((from, top));
            line.push((t - p.away * R, top));
            quad(&mut line, (t - p.away * R, top), (t, top), (t, top + R));
            line.push((t, base));
            let anchor = if -p.away < 0.0 { Anchor::End } else { Anchor::Start };
            EdgeDrawing { line, head: Some(head(t)), label_at: (from - p.away * 3.0, top - 3.0), anchor }
        }
        (None, None) => EdgeDrawing { line, head: None, label_at: (0.0, top), anchor: Anchor::Middle },
    }
}

#[cfg(test)]
mod tests {
    //! `rowInteractions.test.ts`, in its order.
    use super::*;
    use crate::rows::rows_for;

    const CW: f64 = 10.0;
    const BPR: i64 = 60;
    const LEN: i64 = 600;
    const POL: usize = 0;
    const P1: usize = 1;
    const FAR: usize = 2;
    const GONE: usize = 3;

    fn spans() -> Vec<Option<EdgeSpan<'static>>> {
        vec![
            Some(EdgeSpan { start: 10, end: 30, name: "T7 RNAP" }),
            Some(EdgeSpan { start: 40, end: 50, name: "T7 p1" }),
            Some(EdgeSpan { start: 300, end: 320, name: "SP6 p" }),
            None,
        ]
    }

    fn edge(actor: usize, target: usize) -> RowEdge<'static> {
        RowEdge { actor, target: Some(target), verb: "transcribes" }
    }

    #[test]
    fn both_ends_in_one_row_draw_as_one_piece_with_both_xs() {
        let rows = rows_for(LEN, BPR);
        let (pieces, lanes) = interactions_for_row(&[edge(POL, P1)], &spans(), rows[0], CW, LEN);
        assert_eq!((pieces.len(), lanes), (1, 1));
        let p = &pieces[0];
        assert_eq!(p.label, "transcribes", "with both ends here the verb is the whole label");
        assert!(p.target_x.unwrap() > p.actor_x.unwrap(), "the target is later on the molecule, so it draws to the right");
    }

    #[test]
    fn an_end_in_another_row_becomes_a_stub_that_names_the_partner_and_points_the_way() {
        let rows = rows_for(LEN, BPR);
        let (here, _) = interactions_for_row(&[edge(POL, FAR)], &spans(), rows[0], CW, LEN);
        assert_eq!(here.len(), 1);
        assert_eq!(here[0].target_x, None, "the target wrapped away");
        assert_eq!(here[0].label, "transcribes SP6 p", "a bare verb would not say what it acts on");
        assert_eq!(here[0].away, 1.0, "the partner lies later on the molecule");

        let (there, _) = interactions_for_row(&[edge(POL, FAR)], &spans(), rows[5], CW, LEN);
        assert_eq!(there.len(), 1);
        assert_eq!(there[0].actor_x, None, "the actor wrapped away");
        assert_eq!(there[0].label, "T7 RNAP transcribes", "the target end names who acts on it");
    }

    #[test]
    fn a_row_touching_neither_end_draws_nothing_and_buys_no_height() {
        let rows = rows_for(LEN, BPR);
        assert_eq!(interactions_for_row(&[edge(POL, P1)], &spans(), rows[7], CW, LEN), (Vec::new(), 0));
    }

    #[test]
    fn the_whole_construct_is_never_an_endpoint_on_the_map() {
        let rows = rows_for(LEN, BPR);
        let serves = RowEdge { actor: POL, target: None, verb: "serves" };
        assert!(interactions_for_row(&[serves], &spans(), rows[0], CW, LEN).0.is_empty());
    }

    #[test]
    fn an_edge_whose_partner_was_deleted_draws_nothing_rather_than_pointing_at_nowhere() {
        let rows = rows_for(LEN, BPR);
        assert!(interactions_for_row(&[edge(POL, GONE)], &spans(), rows[0], CW, LEN).0.is_empty());
    }

    #[test]
    fn edges_that_would_overlap_are_packed_into_separate_lanes() {
        let rows = rows_for(LEN, BPR);
        let many = [edge(POL, P1), edge(POL, FAR), RowEdge { actor: P1, target: Some(FAR), verb: "binds" }];
        let (pieces, lanes) = interactions_for_row(&many, &spans(), rows[0], CW, LEN);
        assert_eq!(pieces.len(), 3);
        assert!(lanes >= 2, "overlapping edges should stack, got {lanes} lane(s)");
        let used: std::collections::BTreeSet<usize> = pieces.iter().map(|p| p.lane).collect();
        assert_eq!(used.len(), lanes);
    }

    #[test]
    fn a_bracket_lands_its_arrowhead_on_the_target_and_a_stub_reaches_toward_the_partner() {
        let rows = rows_for(LEN, BPR);
        let (pieces, _) = interactions_for_row(&[edge(POL, P1), edge(POL, FAR)], &spans(), rows[0], CW, LEN);
        let bracket = drawing(&pieces[0], 0.0);
        let tx = pieces[0].target_x.unwrap();
        assert_eq!(bracket.head.unwrap()[2], (tx, 18.0), "the head's tip sits on the target at the lane's base");
        assert_eq!(*bracket.line.last().unwrap(), (tx, 18.0));
        let stub = drawing(&pieces[1], 0.0);
        assert!(stub.head.is_none(), "the thing acted on is in another row");
        assert_eq!(stub.anchor, Anchor::Start);
    }
}
