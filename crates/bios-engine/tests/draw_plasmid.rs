//! `draw_plasmid` through the commands, with a pretend painter: what it asks for, where it puts
//! the file, and what it says. The drawing itself is `bios-ui`'s and is tested there.

use std::path::Path;
use std::sync::{Arc, Mutex};

use bios_core::genbank;
use bios_engine::picture::{Ask, Painter, Picture, View};
use bios_engine::{Effect, Session};
use bios_project::{Actor, Project, Who};
use serde_json::{Value, json};

/// A 400-base circular construct with one feature.
const FILE: &str = concat!(
    "LOCUS       demo                     400 bp    DNA     circular SYN 01-JAN-2026\n",
    "FEATURES             Location/Qualifiers\n",
    "     CDS             30..120\n",
    "                     /label=\"pol\"\n",
    "ORIGIN\n",
);

fn construct() -> bios_core::Construct {
    let mut text = String::from(FILE);
    let bases = "acgt".repeat(100);
    for (line, chunk) in bases.as_bytes().chunks(60).enumerate() {
        text.push_str(&format!("{:>9} {}\n", line * 60 + 1, String::from_utf8_lossy(chunk)));
    }
    text.push_str("//\n");
    genbank::read(&text).expect("the fixture is GenBank")
}

/// One picture the pretend painter was asked for.
#[derive(Clone, Debug, PartialEq)]
struct Asked {
    view: String,
    start: i64,
    end: i64,
    width: f64,
    height: f64,
    square: bool,
}

/// Remembers what it was asked for and hands back a picture of a fixed size.
#[derive(Default)]
struct Pretend {
    asked: Mutex<Vec<Asked>>,
    /// The size the drawing comes out at, when it is not the one asked for.
    size: Option<(f64, f64)>,
    /// The bases it says it drew, when they are not the ones asked for.
    drawn: Option<(i64, i64)>,
}

impl Pretend {
    fn new() -> Arc<Self> {
        Arc::new(Pretend::default())
    }

    fn last(&self) -> Asked {
        self.asked.lock().expect("no other thread").last().cloned().expect("it was asked for a picture")
    }

    fn calls(&self) -> usize {
        self.asked.lock().expect("no other thread").len()
    }
}

impl Painter for Pretend {
    fn draw(&self, ask: &Ask<'_>) -> Result<Picture, String> {
        self.asked.lock().expect("no other thread").push(Asked {
            view: ask.view.name().to_string(),
            start: ask.start,
            end: ask.end,
            width: ask.width,
            height: ask.height,
            square: ask.square,
        });
        Ok(Picture {
            svg: "<?xml version=\"1.0\"?>\n<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"10\" height=\"10\"></svg>\n".into(),
            drawn: self.drawn.unwrap_or((ask.start, ask.end)),
            size: self.size.unwrap_or((ask.width, ask.height)),
        })
    }
}

/// The project with `demo` in it, and a session on it.
fn open(dir: &Path, painter: Arc<Pretend>) -> Session {
    let root = dir.join("p");
    std::fs::create_dir_all(&root).expect("a folder");
    std::fs::write(root.join("demo.gb"), genbank_text()).expect("the file");
    let project = Project::open(&root).expect("a project");
    Session::new(project, Who { actor: Actor::Agent, source: "test".into(), session: None }).with_painter(painter)
}

fn genbank_text() -> String {
    bios_core::genbank_write::write(&construct()).expect("GenBank")
}

fn draw(session: &Session, params: Value) -> Value {
    session.execute("draw_plasmid", &params).expect("a picture")
}

#[test]
fn a_picture_with_no_path_goes_to_a_scratch_file_outside_every_project() {
    let dir = tempfile::tempdir().expect("a folder");
    let painter = Pretend::new();
    let session = open(dir.path(), painter.clone());
    let out = draw(&session, json!({ "plasmid": "demo", "view": "circular", "format": "svg" }));

    let path = out["path"].as_str().expect("a path");
    let file = Path::new(path);
    assert!(file.is_file(), "{path}");
    assert!(!file.starts_with(session.project().root()), "a picture nobody asked to keep never lands in the project: {path}");
    assert!(path.ends_with("demo-circular.svg"), "{path}");
    assert_eq!(out["format"], "svg");
    assert_eq!(out["view"], "circular");
    assert_eq!(out["plasmid"], "demo");
    assert_eq!((out["start"].as_i64(), out["end"].as_i64()), (Some(1), Some(400)));
    assert_eq!(out["whole_plasmid"], true);
    assert!(std::fs::read_to_string(file).expect("the picture").contains("<svg"));
    let said = out["said"].as_str().expect("a sentence");
    assert!(said.contains(path) && said.contains("all 400 bases"), "{said}");
}

#[test]
fn a_path_is_where_the_picture_goes_and_keeps_the_name_the_person_asked_for() {
    let dir = tempfile::tempdir().expect("a folder");
    let painter = Pretend::new();
    let session = open(dir.path(), painter.clone());
    let wanted = dir.path().join("pictures").join("map.svg");
    let out = draw(&session, json!({ "plasmid": "demo", "view": "linear", "format": "svg", "path": wanted.display().to_string() }));
    assert_eq!(out["path"], wanted.display().to_string());
    assert!(wanted.is_file(), "the folder it needed was made");

    // A folder takes a picture with a name of its own.
    let folder = dir.path().join("pictures");
    let into = draw(&session, json!({ "plasmid": "demo", "view": "linear", "format": "svg", "path": folder.display().to_string() }));
    assert_eq!(into["path"], folder.join("demo-linear.svg").display().to_string());

    // A path that is not a picture's gains the right ending rather than being overwritten.
    let notes = dir.path().join("notes.txt");
    std::fs::write(&notes, "mine").expect("the notes");
    let beside = draw(&session, json!({ "plasmid": "demo", "view": "linear", "format": "svg", "path": notes.display().to_string() }));
    assert_eq!(beside["path"], dir.path().join("notes.txt.svg").display().to_string());
    assert_eq!(std::fs::read_to_string(&notes).expect("the notes"), "mine");
}

#[test]
fn the_stretch_asked_for_reaches_the_painter_as_bios_counts_it() {
    let dir = tempfile::tempdir().expect("a folder");
    let painter = Pretend::new();
    let session = open(dir.path(), painter.clone());
    draw(&session, json!({ "plasmid": "demo", "view": "sequence", "start": 30, "end": 120, "format": "svg", "width": 800, "height": 600 }));
    // 1-based and inclusive in, 0-based and inclusive to the painter.
    assert_eq!(painter.last(), Asked { view: "sequence".into(), start: 29, end: 119, width: 800.0, height: 600.0, square: false });

    // A whole construct when the stretch is left out.
    draw(&session, json!({ "plasmid": "demo", "view": "linear", "format": "svg" }));
    assert_eq!(painter.last(), Asked { view: "linear".into(), start: 0, end: 399, width: 1200.0, height: 900.0, square: false });
}

#[test]
fn a_picture_says_what_it_actually_holds_rather_than_what_was_asked_for() {
    let dir = tempfile::tempdir().expect("a folder");
    let painter = Arc::new(Pretend { drawn: Some((9, 69)), size: Some((800.0, 300.0)), ..Pretend::default() });
    let session = open(dir.path(), painter);
    let out = draw(&session, json!({ "plasmid": "demo", "view": "sequence", "start": 20, "end": 200, "format": "svg", "width": 800 }));
    assert_eq!((out["start"].as_i64(), out["end"].as_i64()), (Some(10), Some(70)));
    assert_eq!((out["width"].as_i64(), out["height"].as_i64()), (Some(800), Some(300)));
    let said = out["said"].as_str().expect("a sentence");
    assert!(said.contains("bases 10 to 70 of 400"), "{said}");
    assert!(said.contains("Only bases 10 to 70 fit"), "it says the rest did not fit: {said}");
}

#[test]
fn a_range_bios_cannot_draw_is_refused_with_the_reason() {
    let dir = tempfile::tempdir().expect("a folder");
    let painter = Pretend::new();
    let session = open(dir.path(), painter.clone());
    let refused = |params: Value| session.execute("draw_plasmid", &params).expect_err("refused").0;

    assert!(refused(json!({ "plasmid": "demo", "view": "ring" })).contains("circular, linear or sequence"));
    assert!(refused(json!({ "plasmid": "demo", "view": "linear", "start": 0 })).contains("1..400"));
    assert!(refused(json!({ "plasmid": "demo", "view": "linear", "end": 401 })).contains("1..400"));
    assert!(refused(json!({ "plasmid": "demo", "view": "linear", "format": "jpeg" })).contains("png or svg"));
    assert!(refused(json!({ "plasmid": "demo", "view": "linear", "width": 1 })).contains("between 200 and 4000"));
    assert!(refused(json!({ "plasmid": "demo", "view": "linear", "height": 9000 })).contains("between 200 and 4000"));
    // Through the origin: the ring can show it, the two unrolled views cannot.
    let through = refused(json!({ "plasmid": "demo", "view": "linear", "start": 380, "end": 20 }));
    assert!(through.contains("through the origin") && through.contains("two pieces"), "{through}");
    assert!(refused(json!({ "plasmid": "demo", "view": "sequence", "start": 380, "end": 20 })).contains("through the origin"));
    assert_eq!(painter.calls(), 0, "nothing was drawn and no file was written");

    let wrapped = draw(&session, json!({ "plasmid": "demo", "view": "circular", "start": 380, "end": 20, "format": "svg" }));
    assert_eq!((wrapped["start"].as_i64(), wrapped["end"].as_i64()), (Some(380), Some(20)));
}

#[test]
fn a_png_is_asked_for_padded_out_to_a_square_and_an_svg_is_not() {
    let dir = tempfile::tempdir().expect("a folder");
    let painter = Pretend::new();
    let session = open(dir.path(), painter.clone());
    draw(&session, json!({ "plasmid": "demo", "view": "linear", "format": "svg" }));
    assert!(!painter.last().square, "an SVG is written at its own size");

    // The pretend painter's SVG is not a picture QuickLook can turn into a PNG, so this also
    // shows what happens when the conversion fails: the SVG, and a plain word about it.
    let out = draw(&session, json!({ "plasmid": "demo", "view": "linear" }));
    let path = out["path"].as_str().expect("a path");
    if out["format"] == "png" {
        assert!(painter.last().square, "a PNG is drawn into a square for QuickLook");
        assert!(Path::new(path).is_file());
    } else {
        assert_eq!(out["format"], "svg");
        assert!(path.ends_with(".svg"), "{path}");
        assert!(out["png_unavailable"].is_string());
        assert!(out["said"].as_str().unwrap_or_default().contains("PNG could not be made"), "{out}");
    }
}

#[test]
fn without_a_painter_the_command_says_so_and_it_needs_a_project() {
    let dir = tempfile::tempdir().expect("a folder");
    let root = dir.path().join("p");
    std::fs::create_dir_all(&root).expect("a folder");
    std::fs::write(root.join("demo.gb"), genbank_text()).expect("the file");
    let project = Project::open(&root).expect("a project");
    let plain = Session::new(project, Who { actor: Actor::Human, source: "test".into(), session: None });
    let failure = plain.execute("draw_plasmid", &json!({ "plasmid": "demo", "view": "circular" })).expect_err("refused");
    assert!(failure.0.contains("without its drawing"), "{failure}");

    let spec = bios_engine::command("draw_plasmid").expect("the command");
    assert!(spec.needs_project());
    assert_eq!(spec.effect, Effect::Read, "a picture changes nothing in the project");
}

#[test]
fn the_tool_tells_an_agent_it_can_look_at_the_picture() {
    let spec = bios_engine::command("draw_plasmid").expect("the command");
    let said = spec.description;
    assert!(said.contains("LOOK AT IT") && said.contains("full path"), "{said}");
    let names: Vec<&str> = spec.params.iter().map(|p| p.name).collect();
    assert_eq!(names, ["plasmid", "view", "start", "end", "format", "path", "width", "height"]);
    assert_eq!(View::parse("circular"), Some(View::Circular));
    assert_eq!(View::parse("ring"), None);
}
