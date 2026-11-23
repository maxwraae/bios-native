//! The documentation, under Help: the web's docs pages (`apps/web/public/docs/*.html`, listed by
//! `docs/manifest.ts`) drawn natively in the theme, with the sidebar, the page's own contents on
//! the right and links between pages, as `docs/DocsPage.tsx` lays them out.
//!
//! The pages are the web's text as it stands. The fragments use a handful of plain tags (`p`,
//! `h2`, `h3`, `ul`, `ol`, `li`, `strong`, `em`, `code`, `a`, `blockquote`), and that is all the
//! reader here understands; anything else is read as its text.

use egui::text::{LayoutJob, TextFormat};
use egui::{Align2, CornerRadius, CursorIcon, Galley, Pos2, Rect, Sense, Stroke, UiBuilder, pos2, vec2};
use std::sync::Arc;

use crate::theme::{self, Weight};
use crate::{mark, primitives};

/// One page (`DocPage`): its address, its title and the line under the title.
pub struct Page {
    pub slug: &'static str,
    pub title: &'static str,
    pub blurb: &'static str,
    html: &'static str,
}

/// The sidebar's groups, in order (`SECTIONS`).
pub const SECTIONS: &[(&str, &[Page])] = &[
    (
        "Start here",
        &[Page {
            slug: "what-bios-is",
            title: "What Bios is",
            blurb: "The loop the whole app is built around, in one page.",
            html: include_str!("../docs/what-bios-is.html"),
        }],
    ),
    (
        "Concepts",
        &[
            Page {
                slug: "feature",
                title: "Features",
                blurb: "The only kind of object in Bios, and why there is only one.",
                html: include_str!("../docs/feature.html"),
            },
            Page {
                slug: "variant",
                title: "Variants",
                blurb: "Keeping several answers at one position instead of picking one.",
                html: include_str!("../docs/variant.html"),
            },
            Page {
                slug: "project",
                title: "Projects and constructs",
                blurb: "Where your work lives, and which construct the assistant is looking at.",
                html: include_str!("../docs/project.html"),
            },
        ],
    ),
];

/// The page at a slug (`findPage`).
pub fn find(slug: &str) -> Option<&'static Page> {
    SECTIONS.iter().flat_map(|(_, pages)| pages.iter()).find(|p| p.slug == slug)
}

/// A run of text in a block.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Run {
    Text(String),
    Bold(String),
    Italic(String),
    Code(String),
    /// A link's text and the page it leads to (`/docs/<slug>`), or `None` for an address outside.
    Link(String, Option<String>),
}

/// A block of a page.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Block {
    Para(Vec<Run>),
    Heading(u8, String),
    List { ordered: bool, items: Vec<Vec<Run>> },
    Quote(Vec<Run>),
}

fn unescape(text: &str) -> String {
    text.replace("&lt;", "<").replace("&gt;", ">").replace("&quot;", "\"").replace("&#39;", "'").replace("&nbsp;", "\u{a0}").replace("&amp;", "&")
}

/// Collapses the source's line breaks and runs of spaces to one space, as HTML does.
fn collapse(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut space = false;
    for c in text.chars() {
        if c.is_whitespace() {
            space = true;
        } else {
            if space {
                out.push(' ');
            }
            space = false;
            out.push(c);
        }
    }
    if space {
        out.push(' ');
    }
    out
}

/// Reads a docs fragment into blocks. Unknown tags are skipped and their text kept.
pub fn parse(html: &str) -> Vec<Block> {
    let mut blocks = Vec::new();
    let mut runs: Vec<Run> = Vec::new();
    let mut items: Vec<Vec<Run>> = Vec::new();
    let mut list: Option<bool> = None;
    let mut in_item = false;
    let mut block: Option<&str> = None;
    let (mut bold, mut italic, mut code) = (false, false, false);
    let mut link: Option<Option<String>> = None;
    let mut rest = html;
    let flush_text = |runs: &mut Vec<Run>, text: &str, bold: bool, italic: bool, code: bool, link: &Option<Option<String>>| {
        let text = unescape(&collapse(text));
        if text.is_empty() || (text == " " && runs.is_empty()) {
            return;
        }
        runs.push(match (link, code, bold, italic) {
            (Some(to), ..) => Run::Link(text, to.clone()),
            (None, true, ..) => Run::Code(text),
            (None, false, true, _) => Run::Bold(text),
            (None, false, false, true) => Run::Italic(text),
            _ => Run::Text(text),
        });
    };
    let trim = |mut runs: Vec<Run>| {
        if let Some(Run::Text(t)) = runs.first_mut() {
            *t = t.trim_start().to_string();
        }
        if let Some(Run::Text(t)) = runs.last_mut() {
            *t = t.trim_end().to_string();
        }
        runs.retain(|r| !matches!(r, Run::Text(t) if t.is_empty()));
        runs
    };
    while !rest.is_empty() {
        let (text, tag) = match rest.find('<') {
            Some(at) => (rest.get(..at).unwrap_or_default(), rest.get(at..).unwrap_or_default()),
            None => (rest, ""),
        };
        if block.is_some() || in_item {
            flush_text(&mut runs, text, bold, italic, code, &link);
        }
        if tag.is_empty() {
            break;
        }
        let Some(end) = tag.find('>') else {
            break;
        };
        let inner = tag.get(1..end).unwrap_or_default().trim();
        rest = tag.get(end + 1..).unwrap_or_default();
        let closing = inner.starts_with('/');
        let name: String = inner.trim_start_matches('/').chars().take_while(|c| c.is_ascii_alphanumeric()).collect::<String>().to_ascii_lowercase();
        match (name.as_str(), closing) {
            ("p" | "h2" | "h3" | "h4" | "blockquote", false) => {
                block = Some(match name.as_str() {
                    "p" => "p",
                    "h2" => "h2",
                    "h3" | "h4" => "h3",
                    _ => "blockquote",
                });
                runs.clear();
            }
            ("p" | "h2" | "h3" | "h4" | "blockquote", true) if !in_item => {
                let done = trim(std::mem::take(&mut runs));
                let text = || {
                    done.iter()
                        .map(|r| match r {
                            Run::Text(t) | Run::Bold(t) | Run::Italic(t) | Run::Code(t) | Run::Link(t, _) => t.as_str(),
                        })
                        .collect::<String>()
                };
                match block {
                    Some("h2") => blocks.push(Block::Heading(2, text())),
                    Some("h3") => blocks.push(Block::Heading(3, text())),
                    Some("blockquote") => blocks.push(Block::Quote(done)),
                    _ if !done.is_empty() => blocks.push(Block::Para(done)),
                    _ => {}
                }
                block = None;
            }
            ("ul" | "ol", false) => {
                list = Some(name == "ol");
                items.clear();
            }
            ("ul" | "ol", true) => {
                blocks.push(Block::List { ordered: list.unwrap_or(false), items: std::mem::take(&mut items) });
                list = None;
            }
            ("li", false) => {
                in_item = true;
                runs.clear();
            }
            ("li", true) => {
                items.push(trim(std::mem::take(&mut runs)));
                in_item = false;
            }
            ("strong" | "b", c) => bold = !c,
            ("em" | "i", c) => italic = !c,
            ("code", c) => code = !c,
            ("a", false) => {
                let href = inner.split("href=\"").nth(1).and_then(|h| h.split('"').next()).unwrap_or_default();
                link = Some(href.strip_prefix("/docs/").map(str::to_string));
            }
            ("a", true) => link = None,
            _ => {}
        }
    }
    blocks
}

/// What was done on the docs page.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Outcome {
    /// "Back to Bios".
    Close,
}

/// The docs page's state: which page is open (`None` is the contents).
#[derive(Default)]
pub struct Docs {
    pub slug: Option<String>,
    /// A heading to bring into view, from the page's contents on the right.
    scroll_to: Option<usize>,
}

const NAV_W: f32 = 208.0;
const ARTICLE_W: f32 = 680.0;
const TOC_W: f32 = 168.0;

fn r(v: f32) -> CornerRadius {
    CornerRadius::same(v.round().clamp(0.0, 255.0) as u8)
}

/// A link in laid-out text: its first and last-but-one character, and the page it leads to.
type LinkSpan = (usize, usize, Option<String>);

/// A paragraph's text laid out in `width`, with the links' ranges as character spans.
fn lay_out(ui: &egui::Ui, runs: &[Run], width: f32, size: f32, color: egui::Color32) -> (Arc<Galley>, Vec<LinkSpan>) {
    let mut job = LayoutJob { wrap: egui::text::TextWrapping { max_width: width, ..Default::default() }, ..Default::default() };
    let plain = TextFormat { font_id: theme::sans(size), color, line_height: Some(size * 1.6), ..Default::default() };
    let mut links = Vec::new();
    let mut chars = 0;
    for run in runs {
        let (text, format) = match run {
            Run::Text(t) => (t, plain.clone()),
            Run::Bold(t) => (t, TextFormat { font_id: theme::sans_weight(size, Weight::Semibold), ..plain.clone() }),
            Run::Italic(t) => (t, TextFormat { italics: true, ..plain.clone() }),
            Run::Code(t) => (t, TextFormat { font_id: theme::mono(13.0), background: theme::CELL_SUNKEN, ..plain.clone() }),
            Run::Link(t, to) => {
                links.push((chars, chars + t.chars().count(), to.clone()));
                (t, TextFormat { underline: Stroke::new(1.0, theme::alpha(theme::INK, 0.35)), ..plain.clone() })
            }
        };
        chars += text.chars().count();
        job.append(text, 0.0, format);
    }
    (ui.fonts_mut(|f| f.layout_job(job)), links)
}

impl Docs {
    pub fn open(slug: Option<&str>) -> Self {
        Docs { slug: slug.filter(|s| find(s).is_some()).map(str::to_string), scroll_to: None }
    }

    /// Draws the docs over `screen`.
    pub fn show(&mut self, ui: &mut egui::Ui, screen: Rect) -> Option<Outcome> {
        let mut outcome = None;
        let mut go: Option<Option<String>> = None;
        egui::Area::new(egui::Id::unique("docs-page")).order(egui::Order::Foreground).fixed_pos(screen.min).fade_in(false).show(ui.ctx(), |ui| {
            ui.interact(screen, ui.scope_id().with("docs-ground"), Sense::click());
            let painter = ui.painter().clone();
            painter.rect_filled(screen, 0.0, theme::CANVAS);
            // The head: the mark, "Bios docs", and the way back.
            let head = Rect::from_min_size(screen.min, vec2(screen.width(), 57.0));
            painter.rect_filled(head, 0.0, theme::RAIL_GROUND);
            painter.line_segment([head.left_bottom(), head.right_bottom()], Stroke::new(1.0, theme::HAIRLINE));
            mark::paint(&painter, pos2(head.left() + 35.0, head.center().y), 22.0);
            painter.text(pos2(head.left() + 54.0, head.center().y - 1.0), Align2::LEFT_CENTER, "Bios", theme::serif(19.0), theme::INK);
            let docs_link = Rect::from_min_size(pos2(head.left() + 90.0, head.center().y - 10.0), vec2(34.0, 20.0));
            let response = ui.interact(docs_link, ui.scope_id().with("docs-index"), Sense::click());
            painter.text(docs_link.left_center(), Align2::LEFT_CENTER, "docs", theme::sans(13.0), theme::MUTED);
            if response.clicked() {
                go = Some(None);
            }
            let back_text = "Back to Bios";
            let back_w = primitives::text_width(ui, back_text, theme::sans(13.0)) + 26.0;
            let back = Rect::from_min_size(pos2(head.right() - 24.0 - back_w, head.center().y - 14.0), vec2(back_w, 28.0));
            let response = ui.interact(back, ui.scope_id().with("docs-back"), Sense::click());
            painter.rect_filled(back, r(theme::RADIUS_PILL), if response.hovered() { theme::CELL } else { theme::FLOAT });
            painter.rect_stroke(back, r(theme::RADIUS_PILL), Stroke::new(1.0, theme::HAIRLINE_STRONG), egui::StrokeKind::Inside);
            painter.text(back.center(), Align2::CENTER_CENTER, back_text, theme::sans(13.0), theme::INK);
            if response.hovered() {
                ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
            }
            if response.clicked() {
                outcome = Some(Outcome::Close);
            }

            // The three columns, centred.
            let total = NAV_W + 32.0 + ARTICLE_W + 48.0 + TOC_W;
            let show_toc = screen.width() >= total + 48.0;
            let used = if show_toc { total } else { NAV_W + 32.0 + ARTICLE_W.min(screen.width() - NAV_W - 80.0) };
            let left = screen.left() + ((screen.width() - used) / 2.0).max(24.0);
            let top = head.bottom() + 36.0;
            // The sidebar.
            let mut y = top;
            for (title, pages) in SECTIONS {
                painter.text(pos2(left, y + 7.0), Align2::LEFT_CENTER, title.to_uppercase(), theme::sans_weight(11.0, Weight::Medium), theme::MUTED);
                y += 22.0;
                for page in *pages {
                    let row = Rect::from_min_size(pos2(left - 8.0, y), vec2(NAV_W, 26.0));
                    let response = ui.interact(row, ui.scope_id().with(("docs-nav", page.slug)), Sense::click());
                    let current = self.slug.as_deref() == Some(page.slug);
                    if current {
                        painter.rect_filled(row, r(theme::RADIUS_SM), theme::ROW_ACTIVE);
                    } else if response.hovered() {
                        painter.rect_filled(row, r(theme::RADIUS_SM), theme::ROW_HOVER);
                    }
                    if response.hovered() {
                        ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
                    }
                    painter.text(
                        pos2(left, row.center().y),
                        Align2::LEFT_CENTER,
                        page.title,
                        theme::sans(13.0),
                        if current { theme::INK } else { theme::MUTED },
                    );
                    if response.clicked() {
                        go = Some(Some(page.slug.to_string()));
                    }
                    y += 27.0;
                }
                y += 18.0;
            }

            // The article, scrolled under the head.
            let article_w = used - NAV_W - 32.0 - if show_toc { 48.0 + TOC_W } else { 0.0 };
            let article_left = left + NAV_W + 32.0;
            let area = Rect::from_min_max(pos2(article_left - 8.0, head.bottom()), pos2(article_left + article_w + 8.0, screen.bottom()));
            let mut headings: Vec<(u8, String, f32)> = Vec::new();
            let page = self.slug.as_deref().and_then(find);
            let scroll_to = self.scroll_to.take();
            ui.scope_builder(UiBuilder::new().max_rect(area), |ui| {
                ui.set_clip_rect(area);
                let id = ui.make_persistent_id(("docs-article", self.slug.clone()));
                egui::ScrollArea::vertical().id_salt(id).auto_shrink([false, false]).show(ui, |ui| {
                    let origin = ui.min_rect().top();
                    let x = article_left;
                    let mut y = origin + 36.0;
                    let title = page.map_or("Bios documentation", |p| p.title);
                    let lede = page.map_or("What the words mean, and how to work with them. Start at the top if you are new.", |p| p.blurb);
                    ui.painter().text(pos2(x, y), Align2::LEFT_TOP, title, theme::serif(40.0), theme::INK);
                    y += 44.0 + 10.0;
                    let g = ui.fonts_mut(|f| f.layout(lede.to_string(), theme::sans(16.0), theme::MUTED, article_w));
                    let h = g.size().y;
                    ui.painter().galley(pos2(x, y), g, theme::MUTED);
                    y += h + 32.0;
                    match page {
                        Some(page) => {
                            for block in parse(page.html) {
                                y = self.block(ui, &block, x, y, article_w, &mut headings, &mut go);
                            }
                        }
                        None => {
                            for (title, pages) in SECTIONS {
                                y += 12.0;
                                ui.painter().text(pos2(x, y), Align2::LEFT_TOP, *title, theme::serif(26.0), theme::INK);
                                y += 32.0 + 12.0;
                                for page in *pages {
                                    let w = primitives::text_width(ui, page.title, theme::sans_weight(15.0, Weight::Medium));
                                    let link = Rect::from_min_size(pos2(x, y), vec2(w, 22.0));
                                    let response = ui.interact(link, ui.scope_id().with(("docs-index-link", page.slug)), Sense::click());
                                    if response.hovered() {
                                        ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
                                    }
                                    ui.painter().text(
                                        link.left_center(),
                                        Align2::LEFT_CENTER,
                                        page.title,
                                        theme::sans_weight(15.0, Weight::Medium),
                                        theme::INK,
                                    );
                                    ui.painter().text(pos2(x, y + 22.0 + 10.0), Align2::LEFT_CENTER, page.blurb, theme::sans(14.0), theme::MUTED);
                                    if response.clicked() {
                                        go = Some(Some(page.slug.to_string()));
                                    }
                                    y += 56.0;
                                }
                            }
                        }
                    }
                    if let Some((_, _, at)) = scroll_to.and_then(|i| headings.get(i)) {
                        ui.scroll_to_rect(Rect::from_min_size(pos2(x, *at - 24.0), vec2(1.0, 40.0)), Some(egui::Align::TOP));
                    }
                    ui.allocate_rect(Rect::from_min_max(pos2(x, origin), pos2(x + article_w, y + 80.0)), Sense::hover());
                });
            });

            // The page's own contents (`Toc`), from two headings up.
            if show_toc && headings.len() >= 2 {
                let tx = article_left + article_w + 48.0;
                let mut ty = top;
                painter.text(pos2(tx, ty + 7.0), Align2::LEFT_CENTER, "ON THIS PAGE", theme::sans_weight(11.0, Weight::Medium), theme::MUTED);
                ty += 24.0;
                for (i, (level, text, _)) in headings.iter().enumerate() {
                    let indent = if *level == 3 { 12.0 } else { 0.0 };
                    let g = ui.fonts_mut(|f| f.layout(text.clone(), theme::sans(12.0), theme::MUTED, TOC_W - indent));
                    let rect = Rect::from_min_size(pos2(tx + indent, ty), g.size());
                    let response = ui.interact(rect, ui.scope_id().with(("docs-toc", i)), Sense::click());
                    let color = if response.hovered() { theme::INK } else { theme::MUTED };
                    if response.hovered() {
                        ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
                    }
                    let h = g.size().y;
                    painter.galley(rect.min, g, color);
                    if response.clicked() {
                        self.scroll_to = Some(i);
                    }
                    ty += h + 8.0;
                }
            }
        });
        if let Some(slug) = go {
            self.slug = slug;
        }
        if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
            outcome = Some(Outcome::Close);
        }
        outcome
    }

    #[allow(clippy::too_many_arguments)]
    fn block(
        &self,
        ui: &mut egui::Ui,
        block: &Block,
        x: f32,
        y: f32,
        width: f32,
        headings: &mut Vec<(u8, String, f32)>,
        go: &mut Option<Option<String>>,
    ) -> f32 {
        match block {
            Block::Heading(level, text) => {
                let (size, above, below) = if *level == 2 { (26.0, 44.0, 12.0) } else { (19.0, 28.0, 8.0) };
                let top = y + above - 16.0;
                headings.push((*level, text.clone(), top));
                ui.painter().text(pos2(x, top), Align2::LEFT_TOP, text, theme::serif(size), theme::INK);
                top + size * 1.2 + below
            }
            Block::Para(runs) => self.paragraph(ui, runs, pos2(x, y), width, theme::INK, go) + 16.0 + y,
            Block::Quote(runs) => {
                let h = self.paragraph(ui, runs, pos2(x + 16.0, y), width - 16.0, theme::MUTED, go);
                ui.painter().line_segment([pos2(x + 1.0, y), pos2(x + 1.0, y + h)], Stroke::new(2.0, theme::HAIRLINE_STRONG));
                y + h + 20.0
            }
            Block::List { ordered, items } => {
                let mut y = y;
                for (i, item) in items.iter().enumerate() {
                    let marker = if *ordered { format!("{}.", i + 1) } else { "\u{2022}".to_string() };
                    ui.painter().text(pos2(x + 16.0, y + 15.0 * 1.6 / 2.0), Align2::RIGHT_CENTER, marker, theme::sans(15.0), theme::INK);
                    let h = self.paragraph(ui, item, pos2(x + 22.0, y), width - 22.0, theme::INK, go);
                    y += h + 7.0;
                }
                y + 9.0
            }
        }
    }

    /// Paints a paragraph and makes its links work; gives back its height.
    fn paragraph(&self, ui: &mut egui::Ui, runs: &[Run], at: Pos2, width: f32, color: egui::Color32, go: &mut Option<Option<String>>) -> f32 {
        let (galley, links) = lay_out(ui, runs, width, 15.0, color);
        let height = galley.size().y;
        ui.painter().galley(at, galley.clone(), color);
        for (n, (from, to, slug)) in links.iter().enumerate() {
            let a = galley.pos_from_cursor(egui::text::CCursor::new(*from));
            let b = galley.pos_from_cursor(egui::text::CCursor::new(*to));
            // A link that wraps is pressed on either line; its first line is enough to aim at.
            let rect = if (a.top() - b.top()).abs() < 1.0 {
                Rect::from_min_max(a.min, pos2(b.min.x, a.max.y))
            } else {
                Rect::from_min_max(a.min, pos2(width, a.max.y))
            };
            let rect = rect.translate(at.to_vec2());
            let response = ui.interact(rect, ui.scope_id().with(("docs-link", at.y as i64, n)), Sense::click());
            if response.hovered() && slug.is_some() {
                ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
            }
            if response.clicked()
                && let Some(slug) = slug
            {
                *go = Some(Some(slug.clone()));
            }
        }
        height
    }
}

#[cfg(test)]
mod tests {
    use super::{Block, Run, SECTIONS, find, parse};

    #[test]
    fn every_page_in_the_sidebar_reads_and_its_links_lead_to_pages_that_exist() {
        for (_, pages) in SECTIONS {
            for page in *pages {
                let blocks = parse(page.html);
                assert!(blocks.len() > 3, "{} read as almost nothing", page.slug);
                for block in &blocks {
                    let runs: Vec<&Run> = match block {
                        Block::Para(r) | Block::Quote(r) => r.iter().collect(),
                        Block::List { items, .. } => items.iter().flatten().collect(),
                        Block::Heading(..) => Vec::new(),
                    };
                    for run in runs {
                        if let Run::Link(text, Some(to)) = run {
                            assert!(find(to).is_some(), "{} links \"{text}\" to {to}, which is not a page", page.slug);
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn a_fragment_reads_as_paragraphs_headings_lists_and_marked_runs() {
        let blocks = parse(
            "<p>Bios is <strong>free</strong>, reads <code>.dna</code>\nand  more.</p>\n<h2>The loop</h2>\n<ol>\n  <li><strong>Open.</strong> Drop a file\n  in.</li>\n  <li>Change it.</li>\n</ol><p>See <a href=\"/docs/feature\">Features</a> &amp; more.</p>",
        );
        assert_eq!(
            blocks,
            vec![
                Block::Para(vec![
                    Run::Text("Bios is ".into()),
                    Run::Bold("free".into()),
                    Run::Text(", reads ".into()),
                    Run::Code(".dna".into()),
                    Run::Text(" and more.".into())
                ]),
                Block::Heading(2, "The loop".into()),
                Block::List {
                    ordered: true,
                    items: vec![vec![Run::Bold("Open.".into()), Run::Text(" Drop a file in.".into())], vec![Run::Text("Change it.".into())]]
                },
                Block::Para(vec![Run::Text("See ".into()), Run::Link("Features".into(), Some("feature".into())), Run::Text(" & more.".into())]),
            ]
        );
    }
}
