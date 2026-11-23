//! The projects page in numbers: the map on a card, the grid the cards sit in, which cards a
//! filter shows, and how a drag rearranges them. Ported from the web's `projects/ProjectMap.tsx`,
//! `projects/projectFilter.ts` and the drag model in `projects/ProjectsHome.tsx` 470-580.

use crate::overview::viewport::split_span;

/// `TRACK`: the width of a feature's band on a card, in the map's own coordinates.
pub const TRACK: f64 = 6.0;
/// `size`: the map's coordinate space. It is scaled to whatever box it is drawn in.
pub const MAP_SIZE: f64 = 132.0;
/// A card's width (`minmax(…, 208px)`), the gap between cards, and the foot under the square art.
pub const CARD_W: f64 = 208.0;
pub const CARD_GAP: f64 = 16.0;
/// The web's foot is 46; a third line here says where the project's folder is.
pub const FOOT_H: f64 = 62.0;
/// The page's column is at most this wide, centred (`.oge-projects-inner`).
pub const COLUMN_MAX: f64 = 1080.0;

/// One feature's place on a card, by its index in the list the map was given.
#[derive(Clone, Debug, PartialEq)]
pub enum Mark {
    /// An arc of the ring, in fractions of the molecule from the origin.
    Arc { feature: usize, from: f64, to: f64 },
    /// A bar along the line, in the map's coordinates.
    Bar { feature: usize, left: f64, width: f64 },
}

/// The map on a card, in a `MAP_SIZE` square.
#[derive(Clone, Debug, PartialEq)]
pub struct CardMap {
    pub circular: bool,
    /// The ring's centre and radius, or the line's middle height, left end and length.
    pub cx: f64,
    pub cy: f64,
    pub r: f64,
    pub line_x0: f64,
    pub line_w: f64,
    /// Longest first, so the small ones land on top.
    pub marks: Vec<Mark>,
}

fn span_of(start: i64, end: i64, length: i64) -> i64 {
    if end >= start { end - start + 1 } else { length - start + end + 1 }
}

/// `ProjectMap`: a ring for a circular construct, a line for a linear one, each feature drawn as a
/// band, the longest first. `features` are 0-based inclusive spans; an end before its start wraps.
pub fn card_map(length: i64, circular: bool, features: &[(i64, i64)]) -> CardMap {
    let (w, h) = (MAP_SIZE, MAP_SIZE);
    let r = (w / 2.0 - TRACK * 2.0 - 2.0).max(1.0);
    let mut order: Vec<usize> =
        if length > 0 { (0..features.len()).filter(|&i| features[i].0 >= 0 && features[i].1 >= 0).collect() } else { Vec::new() };
    order.sort_by_key(|&i| std::cmp::Reverse(span_of(features[i].0, features[i].1, length)));
    let mut marks = Vec::new();
    let (line_x0, line_w) = (TRACK, w - TRACK * 2.0);
    for i in order {
        let (start, end) = features[i];
        if circular {
            for piece in split_span(start, end, length) {
                marks.push(Mark::Arc { feature: i, from: piece.start_frac, to: piece.start_frac + piece.width_frac });
            }
        } else {
            let (from, to) = (start.min(end), start.max(end));
            let left = line_x0 + from as f64 / length as f64 * line_w;
            let width = ((to - from + 1) as f64 / length as f64 * line_w).max(1.5);
            marks.push(Mark::Bar { feature: i, left, width });
        }
    }
    CardMap { circular, cx: w / 2.0, cy: h / 2.0, r, line_x0, line_w, marks }
}

/// Where the cards go: how many to a row, the row's left edge and its width.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Grid {
    pub columns: usize,
    pub left: f64,
    pub card_w: f64,
    pub card_h: f64,
}

impl Grid {
    /// The top-left corner of the `i`th card, from the grid's own top.
    pub fn at(&self, i: usize) -> (f64, f64) {
        let columns = self.columns.max(1);
        let (row, col) = (i / columns, i % columns);
        (self.left + col as f64 * (self.card_w + CARD_GAP), row as f64 * (self.card_h + CARD_GAP))
    }

    /// How tall `count` cards stand.
    pub fn height(&self, count: usize) -> f64 {
        let rows = count.div_ceil(self.columns.max(1));
        if rows == 0 { 0.0 } else { rows as f64 * (self.card_h + CARD_GAP) - CARD_GAP }
    }
}

/// `.oge-projects-inner` and `.oge-projects-grid`: a centred column at most 1080 wide with at least
/// 20 either side, holding 208-wide cards 16 apart from its left edge; a column narrower than one
/// card gives the card its whole width.
pub fn grid(width: f64) -> Grid {
    let pad = 20f64.max((width - COLUMN_MAX) / 2.0);
    let inner = (width - pad * 2.0).max(0.0);
    let card_w = CARD_W.min(inner).max(1.0);
    let columns = (((inner + CARD_GAP) / (card_w + CARD_GAP)).floor() as usize).max(1);
    Grid { columns, left: pad, card_w, card_h: card_w + FOOT_H }
}

/// The rail's rows (`ProjectFilter`). "Shared" is not here: Bios on your own machine shares
/// nothing from this page.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Filter {
    #[default]
    All,
    Pinned,
}

/// `matchesFilter`.
pub fn matches_filter(id: &str, filter: Filter, pinned: &[String]) -> bool {
    match filter {
        Filter::Pinned => pinned.iter().any(|p| p == id),
        Filter::All => true,
    }
}

/// `countsOf`: all, and pinned counting only pins that still name a project.
pub fn counts(ids: &[&str], pinned: &[String]) -> (usize, usize) {
    (ids.len(), ids.iter().filter(|id| pinned.iter().any(|p| p == *id)).count())
}

/// The two bands a drag moves cards between.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Band {
    Pinned,
    Recent,
}

/// One step of a drag (`onGridPointerMove`): the card `id` is over band `zone`, on the card
/// `over` or on none. Within its own band it takes the place of the card it is over; across
/// bands it goes in before that card, or at the end.
pub fn drag_to(pinned: &mut Vec<String>, recent: &mut Vec<String>, id: &str, zone: Band, over: Option<&str>) {
    let from = if pinned.iter().any(|c| c == id) { Band::Pinned } else { Band::Recent };
    if zone == from {
        let list = if from == Band::Pinned { pinned } else { recent };
        let Some(over) = over.filter(|o| *o != id) else {
            return;
        };
        let (Some(a), Some(b)) = (list.iter().position(|c| c == id), list.iter().position(|c| c == over)) else {
            return;
        };
        let card = list.remove(a);
        list.insert(b, card);
        return;
    }
    let (source, target) = if from == Band::Pinned { (pinned, recent) } else { (recent, pinned) };
    let Some(a) = source.iter().position(|c| c == id) else {
        return;
    };
    let card = source.remove(a);
    let at = over.and_then(|o| target.iter().position(|c| c == o)).unwrap_or(target.len());
    target.insert(at, card);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn arcs(map: &CardMap) -> usize {
        map.marks.iter().filter(|m| matches!(m, Mark::Arc { .. })).count()
    }

    // ProjectMap.test.tsx, in its order.

    #[test]
    fn a_feature_that_crosses_the_origin_is_drawn_as_two_arcs_not_one_long_way_round() {
        assert_eq!(arcs(&card_map(1000, true, &[(950, 49)])), 2, "the wrap is split at the origin");
        assert_eq!(arcs(&card_map(1000, true, &[(100, 199)])), 1);
    }

    #[test]
    fn the_ring_has_a_real_radius_at_card_size() {
        assert!(card_map(1000, true, &[(10, 300)]).r > 20.0);
    }

    #[test]
    fn the_longest_feature_is_drawn_first_so_a_small_nested_one_is_not_painted_over() {
        let map = card_map(5000, true, &[(1000, 1720), (500, 2900)]);
        let order: Vec<usize> = map.marks.iter().map(|m| if let Mark::Arc { feature, .. } = m { *feature } else { 99 }).collect();
        assert_eq!(order, [1, 0]);
    }

    #[test]
    fn a_linear_construct_is_a_line_not_a_ring() {
        let map = card_map(1000, false, &[(10, 300)]);
        assert!(!map.circular);
        assert_eq!(map.marks.len(), 1);
        assert!(matches!(map.marks[0], Mark::Bar { .. }));
    }

    // projectFilter.test.ts, without Shared.

    #[test]
    fn the_predicates() {
        let pinned = vec!["a".to_string()];
        assert!(matches_filter("a", Filter::All, &pinned));
        assert!(matches_filter("b", Filter::All, &pinned));
        assert!(matches_filter("a", Filter::Pinned, &pinned));
        assert!(!matches_filter("b", Filter::Pinned, &pinned));
    }

    #[test]
    fn the_counts_agree_with_the_cards_and_a_pin_naming_a_deleted_project_counts_nothing() {
        let pinned = vec!["a".to_string(), "gone".to_string()];
        assert_eq!(counts(&["a", "b", "c"], &pinned), (3, 1));
        assert_eq!(counts(&[], &["a".to_string()]), (0, 0));
    }

    #[test]
    fn the_grid_is_208_wide_cards_16_apart_in_a_centred_column() {
        let g = grid(1174.0);
        assert_eq!(g.columns, 4);
        assert_eq!(g.left, 47.0);
        // The web's card is 254 tall (208 + a 46 foot); here the foot has a line for where the folder is.
        let card_h = 208.0 + FOOT_H;
        assert_eq!(g.at(5), (47.0 + 224.0, card_h + 16.0));
        assert_eq!(g.height(5), card_h * 2.0 + 16.0);
        // Narrower than one card: the card gives way.
        let narrow = grid(150.0);
        assert_eq!((narrow.columns, narrow.card_w), (1, 110.0));
    }

    #[test]
    fn a_drag_reorders_within_a_band_and_carries_a_card_across() {
        let s = |v: &[&str]| v.iter().map(|x| x.to_string()).collect::<Vec<_>>();
        let (mut pinned, mut recent) = (s(&["p"]), s(&["a", "b", "c"]));
        drag_to(&mut pinned, &mut recent, "c", Band::Recent, Some("a"));
        assert_eq!(recent, s(&["c", "a", "b"]));
        drag_to(&mut pinned, &mut recent, "a", Band::Pinned, Some("p"));
        assert_eq!((pinned.clone(), recent.clone()), (s(&["a", "p"]), s(&["c", "b"])));
        drag_to(&mut pinned, &mut recent, "p", Band::Recent, None);
        assert_eq!((pinned, recent), (s(&["a"]), s(&["c", "b", "p"])));
    }
}
