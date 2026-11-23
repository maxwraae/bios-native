//! What the map layout tests share: the recorded cases under `tests/fixtures/web-map`.
//!
//! Each case is one construct the Bios web app's map was asked to draw in a 1440 x 378 frame, as a
//! ring and as a line. Three kinds of file per case:
//!
//!   - `<case>-features.tsv`: the construct's length and topology, then every feature the file
//!     states (name, key, 0-based inclusive start and end, strand).
//!   - `<case>-widths.tsv`: the width the web app's own text ruler gave, at 11px, for every prefix
//!     of every shown name and every prefix with the two dots on. The layout is handed these as its
//!     ruler, so it fits names by the same numbers the web app did.
//!   - `<case>-ring.tsv` and `<case>-line.tsv`: what the web app drew, one thing per line, in the
//!     order it drew them (the kinds are listed where they are read).
//!
//! The drawings were read off the web app's map-reader page, which draws with the editor's own
//! `PlasmidOverview`. `wrap_demo` is the file of that name under `bios-core/tests/fixtures`. Two
//! cases were recorded on private data and are kept in the folder `BIOS_PRIVATE_FIXTURES` names,
//! under this repository's layout: `lab_construct`, a linear lab construct with thirty shown
//! features, and `crowded_ring`, a crowded AAV-style vector whose three long backbone notes are
//! what make its ring crowded.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use bios_layout::overview::display::{FileFeature, display_features};
use bios_layout::overview::scene::MapScene;

fn read(path: &Path) -> String {
    std::fs::read_to_string(path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

pub fn number(text: &str) -> f64 {
    text.parse().unwrap_or_else(|e| panic!("{text:?} is not a number: {e}"))
}

/// Every number in an SVG path, in order.
pub fn path_numbers(d: &str) -> Vec<f64> {
    d.split(|c: char| !(c.is_ascii_digit() || c == '.' || c == '-' || c == 'e')).filter(|s| !s.is_empty()).map(number).collect()
}

pub fn close(a: f64, b: f64) -> bool {
    (a - b).abs() <= 1e-6
}

pub fn all_close(a: &[f64], b: &[f64]) -> bool {
    a.len() == b.len() && a.iter().zip(b).all(|(x, y)| close(*x, *y))
}

pub struct Case {
    pub scene: MapScene,
    widths: HashMap<String, f64>,
    dir: PathBuf,
    name: &'static str,
}

impl Case {
    /// A case kept in this repository.
    pub fn load(name: &'static str) -> Self {
        Self::load_from(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/web-map"), name)
    }

    /// A case kept in the private fixtures folder, which keeps this repository's layout.
    pub fn load_private(name: &'static str) -> Self {
        let root = std::env::var_os("BIOS_PRIVATE_FIXTURES").expect("set BIOS_PRIVATE_FIXTURES to the private fixtures folder");
        Self::load_from(PathBuf::from(root).join("crates/bios-layout/tests/fixtures/web-map"), name)
    }

    fn load_from(dir: PathBuf, name: &'static str) -> Self {
        let features = read(&dir.join(format!("{name}-features.tsv")));
        let mut lines = features.lines();
        let head: Vec<&str> = lines.next().expect("a first line").split('\t').collect();
        let rows: Vec<Vec<&str>> = lines.map(|line| line.split('\t').collect()).collect();
        let file: Vec<FileFeature<'_>> = rows
            .iter()
            .map(|r| FileFeature {
                name: r[0],
                kind: r[1],
                start: number(r[2]) as i64,
                end: number(r[3]) as i64,
                forward: r[4] == "+",
                locations: None,
                location_unsupported: false,
            })
            .collect();
        let scene = MapScene::new(&display_features(&file, false), number(head[0]) as i64, head[1] == "circular");
        let widths = read(&dir.join(format!("{name}-widths.tsv")))
            .lines()
            .map(|line| {
                let (text, width) = line.rsplit_once('\t').expect("a width");
                (text.to_owned(), number(width))
            })
            .collect();
        Self { scene, widths, dir, name }
    }

    /// The web app's ruler: the width it measured for `text`. Every name on the map is set at 11px.
    pub fn measure(&self) -> impl FnMut(&str, f64) -> f64 + '_ {
        |text, font_px| {
            assert_eq!(font_px, 11.0, "the recorded widths are for 11px type");
            if text.is_empty() {
                return 0.0;
            }
            *self.widths.get(text).unwrap_or_else(|| panic!("the web app never measured {text:?}"))
        }
    }

    /// What the web app drew for this case as `shape` (`ring` or `line`): the rows of one kind,
    /// without the kind, in drawing order.
    pub fn drawn(&self, shape: &str, kind: &str) -> Vec<Vec<String>> {
        read(&self.dir.join(format!("{}-{shape}.tsv", self.name)))
            .lines()
            .map(|line| line.split('\t').map(str::to_owned).collect::<Vec<String>>())
            .filter(|row| row[0] == kind)
            .map(|row| row[1..].to_vec())
            .collect()
    }
}
