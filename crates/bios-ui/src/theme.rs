//! Bios's colours and type, from the web app's `design/tokens.css` and `editor/features.ts`.

use std::sync::Arc;

use egui::epaint::text::{FontTweak, VariationCoords};
use egui::{Color32, FontData, FontDefinitions, FontFamily, FontId};

pub const fn hex(v: u32) -> Color32 {
    Color32::from_rgb((v >> 16) as u8, (v >> 8) as u8, v as u8)
}

// The palette (`--sana-*`).
pub const SANA_AMBER: Color32 = hex(0xfdce0b);
pub const SANA_ORANGE: Color32 = hex(0xf87628);
pub const SANA_GREEN: Color32 = hex(0x23ac00);
/// The one blue (`--sana-blue`). On the map it is the selection, thinned over the ground.
pub const SANA_BLUE: Color32 = hex(0x045afa);
pub const SANA_INDIGO: Color32 = hex(0x7001e1);
pub const SANA_SCARLET: Color32 = hex(0xda1f22);
pub const SANA_MAGENTA: Color32 = hex(0xc8268c);

// Surfaces.
pub const CANVAS: Color32 = hex(0xf9f7f3);
pub const CELL: Color32 = hex(0xf4f3f0);
pub const CELL_HOVER: Color32 = hex(0xe6e6e9);
pub const CELL_ACTIVE: Color32 = hex(0xdcdce0);
/// A list row under the pointer, and the open one (`--row-hover`, `--row-active`): lighter than
/// the cell steps, which make a selected row read as a solid grey slab (tokens.css §rows).
pub const ROW_HOVER: Color32 = hex(0xf4f4f6);
pub const ROW_ACTIVE: Color32 = hex(0xebebee);
pub const CELL_SUNKEN: Color32 = hex(0xecebe7);
pub const RAIL_GROUND: Color32 = hex(0xf4f1ea);
pub const FLOAT: Color32 = hex(0xffffff);

// Ink and lines.
pub const INK: Color32 = hex(0x1d1d1f);
pub const MUTED: Color32 = hex(0x6e6e73);
pub const HAIRLINE: Color32 = hex(0xe3e3e6);
pub const HAIRLINE_STRONG: Color32 = hex(0xd2d2d7);
/// Emphasis is weight, not colour: the accent is ink (`--accent`).
pub const ACCENT: Color32 = INK;
pub const ACCENT_HOVER: Color32 = hex(0x37373b);
pub const ON_ACCENT: Color32 = hex(0xffffff);
pub const DIRTY: Color32 = SANA_ORANGE;
pub const FOCUS: Color32 = INK;
pub const DANGER_BG: Color32 = hex(0xfdecec);
pub const DANGER_BORDER: Color32 = hex(0xf3b7b9);
pub const DANGER_TEXT: Color32 = hex(0xa02329);
pub const INFO_BG: Color32 = hex(0xeaf1fe);
pub const INFO_BORDER: Color32 = hex(0xbcd3fa);
pub const INFO_TEXT: Color32 = hex(0x1c4bb0);

/// Each place's colour, on its icon, its tab and its folders (`--place-*`).
pub const PLACE_EDIT: Color32 = SANA_MAGENTA;
pub const PLACE_BUILD: Color32 = SANA_BLUE;
pub const PLACE_VERIFY: Color32 = SANA_GREEN;

// Radii (`--radius*`).
pub const RADIUS: f32 = 14.0;
pub const RADIUS_SM: f32 = 8.0;
pub const RADIUS_PILL: f32 = 999.0;
pub const RADIUS_TAB: f32 = 10.0;

// Sizes (`--gap`, `--head-h`, …).
pub const GAP: f32 = 6.0;
pub const HEAD_H: f32 = 38.0;
pub const NAVIGATOR_H: f32 = 68.0;
pub const NAV_W: f32 = 88.0;
pub const NAV_FULL_W: f32 = 232.0;

// The sidebar's rows, read at a glance as the Claude and Finder sidebars are (Max, 2026-11-23):
// the words in full ink at 14, the glyphs at 16 in near-ink beside them, a 34-point slot.
pub const SIDE_TEXT: f32 = 14.0;
pub const SIDE_ICON: f32 = 16.0;
pub const SIDE_ROW: f32 = 34.0;
pub const SIDE_GLYPH: Color32 = hex(0x3a3a3c);

/// A CSS `box-shadow` layer: offset, blur, spread, colour.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Shadow {
    pub y: f32,
    pub blur: f32,
    pub spread: f32,
    pub color: Color32,
}

const SHADOW_INK: u32 = 0x1e2328; // rgb(30 35 40)

const fn shadow_ink(a: u8) -> Color32 {
    Color32::from_rgba_unmultiplied_const((SHADOW_INK >> 16) as u8, (SHADOW_INK >> 8) as u8, SHADOW_INK as u8, a)
}

/// `--shadow-1`: `0 1px 2px rgba(30,35,40,.06)`.
pub const SHADOW_1: &[Shadow] = &[Shadow { y: 1.0, blur: 2.0, spread: 0.0, color: shadow_ink(15) }];
/// `--shadow-2`: `0 2px 10px rgba(30,35,40,.08)`.
pub const SHADOW_2: &[Shadow] = &[Shadow { y: 2.0, blur: 10.0, spread: 0.0, color: shadow_ink(20) }];
/// `--shadow-cell`: `0 1px 2px …/.05, 0 10px 26px -14px …/.22`.
pub const SHADOW_CELL: &[Shadow] =
    &[Shadow { y: 1.0, blur: 2.0, spread: 0.0, color: shadow_ink(13) }, Shadow { y: 10.0, blur: 26.0, spread: -14.0, color: shadow_ink(56) }];
/// The glass's lift (button.css): `0 1px 2px …/8%, 0 6px 18px -10px …/25%`.
pub const SHADOW_GLASS: &[Shadow] =
    &[Shadow { y: 1.0, blur: 2.0, spread: 0.0, color: shadow_ink(20) }, Shadow { y: 6.0, blur: 18.0, spread: -10.0, color: shadow_ink(64) }];

const CODING: Color32 = hex(0x00a35b);
const PROMOTER: Color32 = hex(0x6c5cd4);
const REGULATORY: Color32 = hex(0xe2624a);
const STRUCTURAL: Color32 = hex(0x8b8479);

/// The row-only colours, from `tokens.css`: a restriction cut, and the primer ribbon's fill, edge
/// and letters, with the deeper shade (and its letters) of what does not anneal.
pub const CUT_SITE: Color32 = hex(0xb8791a);
pub const CUT_SITE_WASH: Color32 = Color32::from_rgba_unmultiplied_const(184, 121, 26, 51);
pub const PRIMER_WASH: Color32 = Color32::from_rgba_unmultiplied_const(181, 55, 154, 26);
pub const PRIMER_FILL: Color32 = hex(0xf4cdea);
pub const PRIMER_EDGE: Color32 = hex(0xb5379a);
pub const PRIMER_INK: Color32 = hex(0x8a2374);
pub const PRIMER_LIFT: Color32 = hex(0xc874b4);
pub const PRIMER_LIFT_INK: Color32 = hex(0x3d0a34);

/// The colours one ribbon in the primer lane is drawn in: its fill, edge and letters, and the
/// deeper shade (and its letters) of what does not anneal.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RibbonInk {
    pub fill: Color32,
    pub edge: Color32,
    pub ink: Color32,
    pub lift: Color32,
    pub lift_ink: Color32,
}

/// A primer, a held binder and an alignment from Find.
pub const PRIMER_RIBBON: RibbonInk =
    RibbonInk { fill: PRIMER_FILL, edge: PRIMER_EDGE, ink: PRIMER_INK, lift: PRIMER_LIFT, lift_ink: PRIMER_LIFT_INK };

/// What an Exact Find marked: drawn like a primer, so its strand and direction show, in the
/// selection's blue so it reads as found rather than made (native only, PARITY.md).
pub const FOUND_RIBBON: RibbonInk =
    RibbonInk { fill: hex(0xd9e6fd), edge: hex(0x2f6fe4), ink: hex(0x1a4fb0), lift: hex(0x86a9ef), lift_ink: hex(0x0c2a66) };

/// Build's two colours (`tokens.css`): new DNA going in, and the homology arms, which are
/// backbone you already have and are drawn apart from the body for exactly that reason.
pub const INSERT: Color32 = hex(0x23ac00);
pub const ARM: Color32 = hex(0xf87628);

const CODING_SAGE: Color32 = hex(0x6fae7f);
const SCAFFOLD: Color32 = hex(0x5f7e9b);
const SIGNAL: Color32 = hex(0x565b66);

/// How much of a feature's colour shows over the cell ground, by where it is drawn.
pub const TINT_RING: f32 = 0.30;
pub const TINT_ROW_BAR: f32 = 0.45;

/// `amount` of `a` over `b`, from 0 to 1.
pub fn mix(a: Color32, b: Color32, amount: f32) -> Color32 {
    let ch = |x: u8, y: u8| (f32::from(x) * amount + f32::from(y) * (1.0 - amount)).round() as u8;
    Color32::from_rgb(ch(a.r(), b.r()), ch(a.g(), b.g()), ch(a.b(), b.b()))
}

pub fn alpha(c: Color32, a: f32) -> Color32 {
    Color32::from_rgba_unmultiplied(c.r(), c.g(), c.b(), (a.clamp(0.0, 1.0) * 255.0).round() as u8)
}

/// The colour of a feature, by what role its GenBank key plays in the construct.
pub fn feature_color(kind: &str) -> Color32 {
    match kind.to_ascii_lowercase().as_str() {
        "cds" | "gene" | "ncrna" | "mrna" => CODING,
        "promoter" | "rbs" => PROMOTER,
        "enhancer" | "terminator" | "polya_signal" | "regulatory" => REGULATORY,
        _ => STRUCTURAL,
    }
}

/// Whether `text` holds one of `words` as a whole word, the way `\b(a|b)\b` reads it. `text` is
/// lowercase already; a word is letters, digits and underscores.
pub fn has_word(text: &str, words: &[&str]) -> bool {
    let bytes = text.as_bytes();
    let is_word = |b: u8| b.is_ascii_alphanumeric() || b == b'_';
    let starts_word = |i: usize| i.checked_sub(1).and_then(|before| bytes.get(before)).is_none_or(|&b| !is_word(b));
    (0..bytes.len()).filter(|&i| starts_word(i)).any(|i| {
        let rest = bytes.get(i..).unwrap_or_default();
        words.iter().any(|w| rest.starts_with(w.as_bytes()) && rest.get(w.len()).is_none_or(|&b| !is_word(b)))
    })
}

/// The colour of a feature's bar in the rows: its key's role first, and only a feature that
/// would otherwise be neutral is classified by its name, so a stray name never repaints genuine
/// biology. Scaffolding is matched first: "eblock for new intron" holds "intron" yet is scaffolding.
pub fn row_feature_color(kind: &str, name: &str) -> Color32 {
    let by_kind = feature_color(kind);
    if by_kind != STRUCTURAL {
        return by_kind;
    }
    let name = name.to_lowercase();
    if has_word(&name, &["eblock", "e-block", "gibson", "homolog", "homology", "homologous", "scaffold", "overlap", "anneal", "primer"]) {
        SCAFFOLD
    } else if has_word(&name, &["exon"]) {
        CODING
    } else if has_word(&name, &["intron"]) {
        CODING_SAGE
    } else if has_word(&name, &["stop"]) {
        SIGNAL
    } else {
        by_kind
    }
}

/// The darker twin of a feature colour, used for its label and leader line.
pub fn label_color(feature: Color32) -> Color32 {
    if feature == CODING {
        hex(0x00874c)
    } else if feature == PROMOTER {
        hex(0x5a4ab8)
    } else if feature == REGULATORY {
        hex(0xb5442e)
    } else {
        hex(0x6f695f)
    }
}

/// How much of a feature's colour shows in a mark: a feature too small to draw at its true width,
/// widened to a tick. Stronger than `TINT_RING` because a tick carries no name to be read off it,
/// and at that size a 30% mix is indistinguishable from the backbone hairline.
pub const TINT_MARK: f32 = 0.65;

/// The colour of a feature on the map, by the family the map's layout gave it. The three families
/// past the palette are for features told apart only by name: gene structure that is not coding,
/// the scaffolding a construct is built with, and a bare in-frame signal.
pub fn role_color(role: bios_layout::overview::display::FeatureRole) -> Color32 {
    use bios_layout::overview::display::FeatureRole;
    match role {
        FeatureRole::Coding => CODING,
        FeatureRole::Promoter => PROMOTER,
        FeatureRole::Regulatory => REGULATORY,
        FeatureRole::Structural => STRUCTURAL,
        FeatureRole::Sage => CODING_SAGE,
        FeatureRole::Scaffold => SCAFFOLD,
        FeatureRole::Signal => SIGNAL,
    }
}

/// The darker twin of [`role_color`], for a name and its leader line.
pub fn role_label_color(role: bios_layout::overview::display::FeatureRole) -> Color32 {
    use bios_layout::overview::display::FeatureRole;
    match role {
        FeatureRole::Sage => hex(0x4e8c63),
        FeatureRole::Scaffold => hex(0x4a6580),
        FeatureRole::Signal => SIGNAL,
        _ => label_color(role_color(role)),
    }
}

/// Where the middle of the interface font's lowercase letters sits above the baseline, as a share
/// of the type size: half of Instrument Sans's x-height. Text the web app centres on a line
/// (`dominant-baseline: middle`) has its baseline this far below that line.
pub const SANS_MIDDLE: f32 = 0.255;

const SERIF: &str = "serif";
const SANS_FACE: &str = "sans";
const MONO_FACE: &str = "mono";

/// A weight of one of the two variable faces. The fork of egui that Bios draws with reads a
/// font's variation axes, so a weight is the same file registered again with its `wght` axis set,
/// under a family of its own.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Weight {
    Regular,
    Medium,
    /// 550, the web's button weight: the variable cut exists so the interface can use it.
    Book,
    Semibold,
    /// 650, the web's emphasised small labels.
    Heavy,
    Bold,
}

impl Weight {
    const ALL: [Weight; 6] = [Weight::Regular, Weight::Medium, Weight::Book, Weight::Semibold, Weight::Heavy, Weight::Bold];

    /// The weight as a style sheet states it.
    pub fn css(self) -> u16 {
        match self {
            Weight::Regular => 400,
            Weight::Medium => 500,
            Weight::Book => 550,
            Weight::Semibold => 600,
            Weight::Heavy => 650,
            Weight::Bold => 700,
        }
    }

    /// Where on a face's weight axis this weight is drawn.
    fn wght(self, face: &str) -> f32 {
        if self == Weight::Semibold && face == MONO_FACE { MONO_SEMIBOLD_WGHT } else { f32::from(self.css()) }
    }

    fn font_name(self, face: &str) -> String {
        format!("{face}-{}", self.css())
    }
}

/// The web app asks for the system mono at `font-weight: 600`, and a browser on macOS draws that
/// heavier than the font's own 600: it takes the Semibold instance and, because Semibold carries no
/// bold trait, emboldens it again. Measured on the reference pictures at 2x, a letter's stem is
/// 3.7px where the font at 600 gives 2.7px; this is where on the axis the font gives the same stem.
const MONO_SEMIBOLD_WGHT: f32 = 750.0;
/// SF Mono's second axis, the thickness of horizontal strokes. Its named instances from Regular to
/// Semibold all sit here, and the axis's own default is the Light's.
const MONO_YAXS: f32 = 325.0;

/// Instrument Sans at a stated weight.
pub fn sans_weight(size: f32, weight: Weight) -> FontId {
    FontId::new(size, FontFamily::Name(weight.font_name(SANS_FACE).into()))
}

/// The sequence face at a stated weight, as a browser draws it (see [`MONO_SEMIBOLD_WGHT`]). Where
/// SF Mono is not installed this is the toolkit's own monospace, which has one weight.
pub fn mono_weight(size: f32, weight: Weight) -> FontId {
    FontId::new(size, FontFamily::Name(weight.font_name(MONO_FACE).into()))
}

/// Registers `data` once per weight, each as a family that falls back to `fallback`'s fonts.
fn add_weights(fonts: &mut FontDefinitions, face: &str, data: Option<&FontData>, fallback: &FontFamily) {
    let rest = fonts.families.get(fallback).cloned().unwrap_or_default();
    for weight in Weight::ALL {
        let name = weight.font_name(face);
        let mut family = Vec::with_capacity(rest.len() + 1);
        if let Some(data) = data {
            // An axis a face does not have is ignored, so the sans takes only its weight.
            let coords = VariationCoords::new([(u32::from_be_bytes(*b"wght"), weight.wght(face)), (u32::from_be_bytes(*b"YAXS"), MONO_YAXS)]);
            fonts.font_data.insert(name.clone(), Arc::new(data.clone().tweak(FontTweak { coords, ..data.tweak.clone() })));
            family.push(name.clone());
        }
        family.extend(rest.iter().cloned());
        fonts.families.insert(FontFamily::Name(name.into()), family);
    }
}

pub fn sans(size: f32) -> FontId {
    FontId::new(size, FontFamily::Proportional)
}

pub fn serif(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name(SERIF.into()))
}

pub fn mono(size: f32) -> FontId {
    FontId::new(size, FontFamily::Monospace)
}

/// Instrument Sans for the interface, Instrument Serif for display, and the system's SF Mono for
/// sequence when it is installed.
pub fn install_fonts(ctx: &egui::Context) {
    let mut fonts = FontDefinitions::default();
    let sans = FontData::from_static(include_bytes!("../assets/fonts/InstrumentSans.ttf"));
    fonts.font_data.insert("InstrumentSans".into(), Arc::new(sans.clone()));
    fonts.font_data.insert("InstrumentSerif".into(), Arc::new(FontData::from_static(include_bytes!("../assets/fonts/InstrumentSerif-Regular.ttf"))));
    fonts.families.entry(FontFamily::Proportional).or_default().insert(0, "InstrumentSans".into());
    let mut serif = vec!["InstrumentSerif".to_string()];
    serif.extend(fonts.families.get(&FontFamily::Proportional).cloned().unwrap_or_default());
    fonts.families.insert(FontFamily::Name(SERIF.into()), serif);
    let sf_mono = std::fs::read("/System/Library/Fonts/SFNSMono.ttf").ok().map(FontData::from_owned);
    if let Some(data) = &sf_mono {
        fonts.font_data.insert("SFMono".into(), Arc::new(data.clone()));
        fonts.families.entry(FontFamily::Monospace).or_default().insert(0, "SFMono".into());
        // The interface face has no ⌘ ⇧ ⌥ ⌃, which every shortcut in a tooltip is written with; a
        // browser takes such a glyph from a system face, and so does this.
        fonts.families.entry(FontFamily::Proportional).or_default().push("SFMono".into());
    }
    add_weights(&mut fonts, SANS_FACE, Some(&sans), &FontFamily::Proportional);
    add_weights(&mut fonts, MONO_FACE, sf_mono.as_ref(), &FontFamily::Monospace);
    ctx.set_fonts(fonts);
}
