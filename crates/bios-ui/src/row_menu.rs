//! The menu on a selection, and on the things drawn over the bases: a right-click away.
//!
//! Ported from the web app's `editor/rows/RowMenu.tsx` (`menuGroups` 78-157 and the menu itself
//! 159-295) and the `.row-menu` rules in `styles.css` 10202-10289. The groups follow what a person
//! does with a stretch of bases: take it away, make something of it (a feature, a primer, a
//! binder), go and find out about it, and work on it. Rows whose capability Bios does not have are not offered (the web leaves a row out
//! the same way when its action is absent).

use egui::epaint::Shadow;
use egui::{Align2, Color32, CornerRadius, CursorIcon, Id, Pos2, Rect, Sense, Stroke, Ui, pos2, vec2};

use crate::primitives::text_width;
use crate::theme;

/// What a row of the menu does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    /// Open the panel of this feature.
    Card(usize),
    RemovePrimer(usize),
    /// Take this held binder back off; any of its sites stands for the whole oligo.
    RemoveBinder(usize),
    /// Not in the web's menu: the web removed a feature through its assistant, which Bios leaves
    /// out, so without this a person could not remove a feature from the window at all.
    RemoveFeature(usize),
    Copy,
    CopyReverse,
    CopyFasta,
    CopyProtein,
    Delete,
    Feature,
    Primer,
    /// Hold the oligo that anneals to the selection against the construct, without landing it.
    Binder,
    /// Open Find with the selected bases typed.
    Find,
    NewConstruct,
    /// Select this recognition site, first and last base.
    Site(usize, usize),
    Optimise,
    /// Float the amino acids of the selection over it, read on this strand in this frame (0-2).
    Translate {
        forward: bool,
        frame: u8,
    },
    /// Take the open plasmid's `index`th floating translation off.
    RemoveTranslation(usize),
}

/// One row: its words, what it does, and what it says at the right.
#[derive(Clone, Debug, PartialEq)]
pub struct Item {
    pub id: String,
    pub text: String,
    /// `None` for a row that only says something, or that opens a submenu.
    pub action: Option<Action>,
    pub danger: bool,
    pub muted: bool,
    /// A note at the right: an overhang, a coordinate.
    pub hint: Option<String>,
    /// The chord that does the same, as the web's `Kbd` prints it.
    pub kbd: Option<&'static str>,
    pub children: Vec<Vec<Item>>,
}

impl Item {
    fn new(id: &str, text: &str, action: Action) -> Item {
        Item { id: id.into(), text: text.into(), action: Some(action), danger: false, muted: false, hint: None, kbd: None, children: Vec::new() }
    }

    fn danger(self) -> Item {
        Item { danger: true, ..self }
    }

    fn kbd(self, kbd: &'static str) -> Item {
        Item { kbd: Some(kbd), ..self }
    }

    fn sub(id: &str, text: &str, children: Vec<Vec<Item>>) -> Item {
        Item { action: None, children, ..Item::new(id, text, Action::Copy) }
    }
}

/// What was under the pointer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Thing {
    Feature(usize),
    Primer(usize),
    /// One of the binders held against the construct, by its place in that list.
    Binder(usize),
}

/// A cut inside the selection, as the Cut here submenu lists it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CutHere {
    pub enzyme: String,
    /// The top-strand cut, a gap counted from 0, as the web prints it.
    pub at: usize,
    /// `blunt`, `5′ AATT` or `3′ ACGT`.
    pub overhang: String,
    /// The recognition site, first and last base.
    pub site: (usize, usize),
}

/// The rows for this target, grouped; a rule is drawn between groups (`menuGroups`).
pub fn menu_groups(thing: Option<Thing>, selection: bool, cuts: &[CutHere]) -> Vec<Vec<Item>> {
    let mut groups = Vec::new();
    match thing {
        Some(Thing::Feature(i)) => groups.push(vec![
            Item::new("card", "Feature card", Action::Card(i)),
            Item::new("remove-feature", "Remove feature", Action::RemoveFeature(i)).danger(),
        ]),
        Some(Thing::Primer(i)) => groups.push(vec![Item::new("remove-primer", "Remove primer", Action::RemovePrimer(i)).danger()]),
        // An oligo you can make but not un-make is ink, which a binder is defined by not being.
        Some(Thing::Binder(i)) => groups.push(vec![Item::new("remove-binder", "Remove binder", Action::RemoveBinder(i)).danger()]),
        None => {}
    }
    if !selection {
        return groups;
    }
    groups.push(vec![
        Item::new("copy", "Copy", Action::Copy),
        Item::new("copy-revcomp", "Copy reverse complement", Action::CopyReverse).kbd("⌘⇧C"),
        Item::sub(
            "copy-as",
            "Copy as",
            vec![vec![Item::new("copy-fasta", "FASTA", Action::CopyFasta), Item::new("copy-protein", "Protein, frame +1", Action::CopyProtein)]],
        ),
        Item::new("delete", "Delete", Action::Delete).danger(),
    ]);
    groups.push(vec![
        Item::new("feature", "Feature", Action::Feature).kbd("⌘⇧F"),
        Item::new("primer", "Primer", Action::Primer).kbd("⌘⇧P"),
        Item::new("binder", "Binder", Action::Binder),
    ]);
    groups.push(vec![
        Item::new("find", "Find elsewhere in the map", Action::Find),
        Item::new("new-construct", "New construct from this", Action::NewConstruct),
    ]);
    let rows = if cuts.is_empty() {
        vec![Item { muted: true, action: None, ..Item::new("cut-none", "No enzyme cuts here", Action::Copy) }]
    } else {
        cuts.iter()
            .map(|c| Item {
                hint: Some(format!("{} · {}", crate::selection_readout::thousands(c.at), c.overhang)),
                ..Item::new(&format!("cut-{}-{}", c.enzyme, c.at), &c.enzyme, Action::Site(c.site.0, c.site.1))
            })
            .collect()
    };
    groups.push(vec![Item::sub("cut-here", "Cut here", vec![rows]), Item::new("optimise", "Codon-optimise", Action::Optimise)]);
    groups
}

/// Adds "Translate this" to the group that makes something of the selection, last
/// (`RowMenu.tsx` 124-137): frames +1 to +3, then −1 to −3 on the bottom strand, then a Remove for
/// each floating translation the selection overlaps, given as its index and its label.
pub fn with_translations(mut groups: Vec<Vec<Item>>, here: &[(usize, String)]) -> Vec<Vec<Item>> {
    let Some(make) = groups.iter_mut().find(|g| g.iter().any(|it| it.id == "feature")) else {
        return groups;
    };
    let frames = |forward: bool| -> Vec<Item> {
        (0..3u8)
            .map(|f| {
                let (id, text) = if forward {
                    (format!("translate-f{}", f + 1), format!("Frame +{}", f + 1))
                } else {
                    (format!("translate-r{}", f + 1), format!("Frame −{}", f + 1))
                };
                let item = Item::new(&id, &text, Action::Translate { forward, frame: f });
                if forward { item } else { Item { hint: Some("bottom strand".into()), ..item } }
            })
            .collect()
    };
    let mut children = vec![frames(true), frames(false)];
    if !here.is_empty() {
        children.push(
            here.iter()
                .map(|(i, label)| Item::new(&format!("untranslate-{i}"), &format!("Remove {label}"), Action::RemoveTranslation(*i)).danger())
                .collect(),
        );
    }
    make.push(Item::sub("translate", "Translate this", children));
    groups
}

/// The menu while it is open.
#[derive(Clone, Debug, PartialEq)]
pub struct RowMenu {
    /// Where it was asked for, on screen.
    pub at: Pos2,
    pub groups: Vec<Vec<Item>>,
    /// Which submenu stands open.
    open_sub: Option<String>,
    /// A submenu the pointer rests on, and since when: it opens after a short grace.
    armed: Option<(String, f64)>,
}

/// What the person did with the menu.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    Run(Action),
    Close,
}

/// `.row-menu-item`: 6px 10px round 13px type.
const ITEM_H: f32 = 13.0 * 1.25 + 12.0;
const RULE_H: f32 = 9.0;
/// How long a pointer rests on a row before its submenu opens.
const ARM: f64 = 0.12;

impl RowMenu {
    pub fn new(at: Pos2, groups: Vec<Vec<Item>>) -> Option<RowMenu> {
        if groups.iter().all(Vec::is_empty) {
            return None;
        }
        Some(RowMenu { at, groups, open_sub: None, armed: None })
    }
}

fn width_of(ui: &Ui, groups: &[Vec<Item>], min: f32) -> f32 {
    let widest = groups
        .iter()
        .flatten()
        .map(|it| {
            let mut w = text_width(ui, &it.text, theme::sans(13.0));
            for note in [it.hint.as_deref(), it.kbd].into_iter().flatten() {
                w += 12.0 + text_width(ui, note, theme::mono(11.0));
            }
            if !it.children.is_empty() {
                w += 12.0 + 8.0;
            }
            w + 20.0
        })
        .fold(0.0, f32::max);
    (widest + 8.0).max(min)
}

/// Draws the menu and says what was chosen. Any press elsewhere, or Escape, closes it.
pub fn show(ctx: &egui::Context, menu: &mut RowMenu) -> Option<Outcome> {
    let screen = ctx.content_rect();
    let count: usize = menu.groups.iter().map(Vec::len).sum();
    // Clamped so a right-click near the right or bottom edge does not open it off screen.
    let corner =
        pos2(menu.at.x.min(screen.right() - 240.0), menu.at.y.min(screen.bottom() - (count as f32 * 30.0 + menu.groups.len() as f32 * 9.0 + 16.0)));
    let sub_left = menu.at.x > screen.right() - 480.0;
    let now = ctx.input(|i| i.time);
    let mut chosen = None;
    let mut rects = Vec::new();
    let groups = menu.groups.clone();
    let mut hovered_sub: Option<(String, Rect, Vec<Vec<Item>>)> = None;
    let mut open_now: Option<(Rect, Vec<Vec<Item>>)> = None;
    let mut toggled = None;
    rects.push(panel(
        ctx,
        Id::unique("row-menu"),
        corner,
        &groups,
        220.0,
        |it, r| {
            if !it.children.is_empty() {
                if menu.open_sub.as_deref() == Some(it.id.as_str()) {
                    open_now = Some((r, it.children.clone()));
                }
                return RowFace { open: menu.open_sub.as_deref() == Some(it.id.as_str()) };
            }
            RowFace { open: false }
        },
        |it, r, response| {
            if !it.children.is_empty() {
                if response.hovered() {
                    hovered_sub = Some((it.id.clone(), r, it.children.clone()));
                }
                if response.clicked() {
                    toggled = Some(it.id.clone());
                }
            } else if response.clicked()
                && let Some(action) = it.action
            {
                chosen = Some(Outcome::Run(action));
            }
        },
    ));
    if let Some(id) = toggled {
        menu.open_sub = if menu.open_sub.as_deref() == Some(id.as_str()) { None } else { Some(id) };
        menu.armed = None;
    }
    match hovered_sub {
        Some((id, _, _)) if menu.open_sub.as_deref() != Some(id.as_str()) => match &menu.armed {
            Some((armed, since)) if *armed == id => {
                if now - since >= ARM {
                    menu.open_sub = Some(id);
                    menu.armed = None;
                } else {
                    ctx.request_repaint_after(std::time::Duration::from_secs_f64(ARM));
                }
            }
            _ => {
                menu.armed = Some((id, now));
                ctx.request_repaint_after(std::time::Duration::from_secs_f64(ARM));
            }
        },
        Some(_) => {}
        None => menu.armed = None,
    }
    if let Some((row, children)) = open_now {
        // `.row-menu.is-sub`: beside its row, 5px up, 2px clear; to the left near the right edge.
        let width = ctx.data(|d| d.get_temp::<f32>(Id::unique("row-menu-sub-w"))).unwrap_or(200.0);
        let x = if sub_left { row.left() - 4.0 - 2.0 - width } else { row.right() + 4.0 + 2.0 };
        let sub = panel(
            ctx,
            Id::unique("row-menu-sub"),
            pos2(x, row.top() - 4.0 - 5.0),
            &children,
            200.0,
            |_, _| RowFace { open: false },
            |it, _, response| {
                if response.clicked()
                    && let Some(action) = it.action
                {
                    chosen = Some(Outcome::Run(action));
                }
            },
        );
        ctx.data_mut(|d| d.insert_temp(Id::unique("row-menu-sub-w"), sub.width()));
        rects.push(sub);
    }
    if chosen.is_none() {
        let away = ctx.input(|i| (i.pointer.primary_pressed() || i.pointer.secondary_pressed()).then(|| i.pointer.interact_pos()).flatten());
        if away.is_some_and(|at| !rects.iter().any(|r| r.contains(at))) || ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            chosen = Some(Outcome::Close);
        }
    }
    chosen
}

/// How a row is drawn beyond its item: whether its submenu is open.
struct RowFace {
    open: bool,
}

/// One `.row-menu` panel at `corner`. `face` says how each row looks, `then` hears what happened
/// on it. Gives the panel's rect.
fn panel(
    ctx: &egui::Context,
    id: Id,
    corner: Pos2,
    groups: &[Vec<Item>],
    min_width: f32,
    mut face: impl FnMut(&Item, Rect) -> RowFace,
    mut then: impl FnMut(&Item, Rect, &egui::Response),
) -> Rect {
    let shown = egui::Area::new(id).order(egui::Order::Tooltip).fixed_pos(corner).fade_in(false).show(ctx, |ui| {
        egui::Frame::new()
            .fill(theme::CELL)
            .stroke(Stroke::new(1.0, theme::HAIRLINE))
            .corner_radius(theme::RADIUS)
            .inner_margin(4.0)
            .shadow(Shadow { offset: [0, 2], blur: 10, spread: 0, color: Color32::from_rgba_unmultiplied(30, 35, 40, 20) })
            .show(ui, |ui| {
                ui.spacing_mut().item_spacing = vec2(0.0, 0.0);
                let width = width_of(ui, groups, min_width) - 8.0 - 2.0;
                for (gi, group) in groups.iter().enumerate() {
                    if gi > 0 {
                        let (r, _) = ui.allocate_exact_size(vec2(width, RULE_H), Sense::hover());
                        ui.painter().hline(r.left() + 6.0..=r.right() - 6.0, r.center().y, Stroke::new(1.0, theme::HAIRLINE));
                    }
                    for it in group {
                        let sense = if it.muted { Sense::hover() } else { Sense::click() };
                        let (r, response) = ui.allocate_exact_size(vec2(width, ITEM_H), sense);
                        let look = face(it, r);
                        row(ui, it, r, response.hovered() || look.open);
                        if response.hovered() && !it.muted {
                            ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
                        }
                        then(it, r, &response);
                    }
                }
            });
    });
    shown.response.rect
}

/// `.row-menu-item`, with its hint, its chord and its chevron at the right.
fn row(ui: &Ui, it: &Item, r: Rect, lit: bool) {
    let painter = ui.painter();
    if lit && !it.muted {
        painter.rect_filled(r, CornerRadius::same(theme::RADIUS_SM as u8), theme::ROW_HOVER);
    }
    let ink = if it.danger {
        theme::SANA_SCARLET
    } else if it.muted {
        theme::MUTED
    } else {
        theme::INK
    };
    painter.text(pos2(r.left() + 10.0, r.center().y), Align2::LEFT_CENTER, &it.text, theme::sans(13.0), ink);
    let mut right = r.right() - 10.0;
    if !it.children.is_empty() {
        let chev = painter.text(pos2(right, r.center().y), Align2::RIGHT_CENTER, "›", theme::sans(13.0), theme::MUTED);
        right = chev.left() - 8.0;
    }
    for note in [it.kbd, it.hint.as_deref()].into_iter().flatten() {
        let drawn = painter.text(pos2(right, r.center().y), Align2::RIGHT_CENTER, note, theme::mono(11.0), theme::MUTED);
        right = drawn.left() - 8.0;
    }
}

#[cfg(test)]
mod tests {
    use super::{Action, CutHere, Thing, menu_groups, with_translations};

    fn texts(groups: &[Vec<super::Item>]) -> Vec<Vec<String>> {
        groups.iter().map(|g| g.iter().map(|it| it.text.clone()).collect()).collect()
    }

    #[test]
    fn a_selection_is_offered_what_bios_can_do_with_it_in_the_webs_groups() {
        let groups = menu_groups(None, true, &[]);
        assert_eq!(
            texts(&groups),
            [
                vec!["Copy", "Copy reverse complement", "Copy as", "Delete"],
                vec!["Feature", "Primer", "Binder"],
                vec!["Find elsewhere in the map", "New construct from this"],
                vec!["Cut here", "Codon-optimise"],
            ]
        );
        let cut_here = &groups[3][0].children[0];
        assert_eq!((cut_here[0].text.as_str(), cut_here[0].muted, cut_here[0].action), ("No enzyme cuts here", true, None));
    }

    #[test]
    fn a_thing_under_the_pointer_comes_first_and_alone_without_a_selection() {
        assert_eq!(texts(&menu_groups(Some(Thing::Feature(2)), false, &[])), [vec!["Feature card", "Remove feature"]]);
        let primer = menu_groups(Some(Thing::Primer(4)), false, &[]);
        assert_eq!((primer[0][0].action, primer[0][0].danger), (Some(Action::RemovePrimer(4)), true));
        let feature = menu_groups(Some(Thing::Feature(2)), false, &[]);
        assert_eq!((feature[0][1].action, feature[0][1].danger), (Some(Action::RemoveFeature(2)), true));
        let binder = menu_groups(Some(Thing::Binder(1)), false, &[]);
        assert_eq!((binder[0][0].text.as_str(), binder[0][0].action, binder[0][0].danger), ("Remove binder", Some(Action::RemoveBinder(1)), true));
        assert!(menu_groups(None, false, &[]).is_empty());
    }

    #[test]
    fn translate_this_offers_six_frames_and_removes_what_the_selection_overlaps() {
        let groups = with_translations(menu_groups(None, true, &[]), &[(3, "+2 1–30".to_string())]);
        let make = &groups[1];
        assert_eq!(make.last().map(|it| it.text.as_str()), Some("Translate this"));
        assert_eq!(make.first().map(|it| it.text.as_str()), Some("Feature"));
        let translate = &make[make.len() - 1].children;
        assert_eq!(texts(translate), [vec!["Frame +1", "Frame +2", "Frame +3"], vec!["Frame −1", "Frame −2", "Frame −3"], vec!["Remove +2 1–30"]]);
        assert_eq!(translate[1][2].action, Some(Action::Translate { forward: false, frame: 2 }));
        assert_eq!(translate[1][0].hint.as_deref(), Some("bottom strand"));
        assert_eq!((translate[2][0].action, translate[2][0].danger), (Some(Action::RemoveTranslation(3)), true));
        // With no selection there is nothing to make, and nothing is added.
        assert!(with_translations(menu_groups(None, false, &[]), &[]).is_empty());
    }

    #[test]
    fn the_cuts_inside_say_where_and_what_they_leave() {
        let cut = CutHere { enzyme: "EcoRI".into(), at: 1201, overhang: "5′ AATT".into(), site: (1200, 1205) };
        let groups = menu_groups(None, true, &[cut]);
        let row = &groups[3][0].children[0][0];
        assert_eq!((row.text.as_str(), row.hint.as_deref(), row.action), ("EcoRI", Some("1,201 · 5′ AATT"), Some(Action::Site(1200, 1205))));
    }
}
