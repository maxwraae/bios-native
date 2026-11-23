use std::path::Path;
use std::time::Duration;

use craft_dialogs::Answer;
use egui::{Event, Key, Modifiers, RawInput};

use crate::{App, Then};

/// A window's state with wrap_demo open and the caret before its first base.
fn app() -> (tempfile::TempDir, App) {
    let dir = tempfile::tempdir().unwrap();
    let mut app = App::new(None, Duration::ZERO);
    app.answered(Then::NewProject, Answer::Paths(vec![dir.path().join("Lab")]));
    let file = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../crates/bios-core/tests/fixtures/wrap_demo.gb");
    app.answered(Then::Import, Answer::Paths(vec![file]));
    app.point(0, 0, 0.0);
    (dir, app)
}

/// One frame in which these things happened at the keyboard.
fn frame(app: &mut App, ctx: &egui::Context, events: Vec<Event>) {
    ctx.run_ui(RawInput { events, ..RawInput::default() }, |ui| app.keys(ui.ctx())).drop_without_applying_deltas();
}

fn key(key: Key, modifiers: Modifiers) -> Event {
    Event::Key { key, physical_key: None, pressed: true, repeat: false, modifiers }
}

fn start(app: &App) -> String {
    app.opened.as_ref().unwrap().construct.sequence.chars().take(12).collect()
}

#[test]
fn letters_that_are_bases_are_typed_and_the_rest_are_not() {
    let (_dir, mut app) = app();
    let ctx = egui::Context::default();
    let before = start(&app);
    frame(&mut app, &ctx, vec![Event::Text("g".into()), Event::Text("x".into()), Event::Text("N".into()), Event::Text("7".into())]);
    assert_eq!(start(&app), format!("GN{}", &before[..10]));
    assert_eq!(app.cursor.caret, Some(2));
    frame(&mut app, &ctx, vec![key(Key::Backspace, Modifiers::NONE)]);
    frame(&mut app, &ctx, vec![key(Key::ArrowLeft, Modifiers::NONE), key(Key::Delete, Modifiers::NONE)]);
    assert_eq!(start(&app), before);
    assert_eq!(app.cursor.caret, Some(0));
}

#[test]
fn arrows_move_the_caret_shift_selects_and_select_all_takes_everything() {
    let (_dir, mut app) = app();
    let ctx = egui::Context::default();
    frame(&mut app, &ctx, vec![key(Key::ArrowRight, Modifiers::NONE), key(Key::ArrowRight, Modifiers::NONE)]);
    assert_eq!(app.cursor.caret, Some(2));
    frame(&mut app, &ctx, vec![key(Key::ArrowRight, Modifiers::SHIFT), key(Key::ArrowRight, Modifiers::SHIFT)]);
    assert_eq!((app.cursor.caret, app.cursor.selection), (None, Some((2, 3))));
    // The row it moved to is asked into view.
    assert_eq!(app.cursor.reveal, Some(4));
    frame(&mut app, &ctx, vec![key(Key::A, Modifiers::COMMAND)]);
    assert_eq!(app.cursor.selection, Some((0, 799)));
    // Typing over everything replaces it; deleting everything is refused in a sentence.
    frame(&mut app, &ctx, vec![key(Key::Backspace, Modifiers::NONE)]);
    assert_eq!(app.problem.as_deref(), Some("A plasmid cannot be left with no bases. To get rid of it, delete the plasmid."));
    assert_eq!(app.opened.as_ref().unwrap().construct.len(), 800);
}

#[test]
fn paste_puts_dna_in_and_refuses_prose_and_cut_takes_the_selection_out() {
    let (_dir, mut app) = app();
    let ctx = egui::Context::default();
    let before = start(&app);
    frame(&mut app, &ctx, vec![Event::Paste("1 atgg tgag\n".into())]);
    assert_eq!(start(&app), format!("ATGGTGAG{}", &before[..4]));
    frame(&mut app, &ctx, vec![Event::Paste("Remember to order more tips on Tuesday.".into())]);
    assert_eq!(app.problem.as_deref(), Some("That does not look like DNA, so nothing was pasted."));
    assert_eq!(app.opened.as_ref().unwrap().construct.len(), 808);

    app.point(0, 8, 0.0);
    frame(&mut app, &ctx, vec![Event::Cut]);
    assert_eq!(start(&app), before);
    assert_eq!((app.cursor.caret, app.cursor.selection), (Some(0), None));
}

#[test]
fn copy_reverse_complement_reads_the_other_strand_of_the_selection() {
    let (_dir, mut app) = app();
    app.command(crate::menus::COPY_REVERSE);
    assert_eq!(app.to_clipboard, None, "nothing is selected yet");
    app.point(0, 6, 0.0);
    assert_eq!(app.opened.as_ref().unwrap().editing.copy().as_deref(), Some("ACGTAC"));
    app.command(crate::menus::COPY_REVERSE);
    assert_eq!(app.to_clipboard.as_deref(), Some("GTACGT"));
    // The stretch cut the feature that runs through the origin, and the window says so.
    assert!(app.notice.is_some());
}

#[test]
fn a_feature_copied_whole_is_pasted_with_its_bases() {
    let (_dir, mut app) = app();
    let ctx = egui::Context::default();
    // The MCS, bases 600 to 620, selected whole and copied.
    app.point(599, 620, 0.0);
    frame(&mut app, &ctx, vec![Event::Copy]);
    let copied = app.to_clipboard.clone().unwrap();
    assert_eq!(copied.len(), 21);
    app.point(300, 300, 0.0);
    frame(&mut app, &ctx, vec![Event::Paste(copied)]);
    let features = &app.opened.as_ref().unwrap().construct.features;
    let sites: Vec<(usize, usize)> = features.iter().filter(|f| f.name == "MCS").map(|f| (f.start, f.end)).collect();
    assert_eq!(sites, [(620, 640), (300, 320)], "the original moved 21 bases on, and the copy sits where it was pasted");
    assert!(app.save());
    assert_eq!(app.opened.as_ref().unwrap().history.last().map(|e| e.summary.as_str()), Some("Pasted 21 bases with 1 annotation after base 300"));
}

#[test]
fn keys_meant_for_the_naming_sheet_do_not_reach_the_bases() {
    let (_dir, mut app) = app();
    let ctx = egui::Context::default();
    app.point(0, 20, 0.0);
    app.command(crate::menus::NEW_FEATURE);
    assert!(app.ask.is_some());
    let before = start(&app);
    frame(&mut app, &ctx, vec![Event::Text("g".into()), key(Key::Backspace, Modifiers::NONE)]);
    assert_eq!(start(&app), before);
    assert_eq!(app.cursor.selection, Some((0, 19)));
}
