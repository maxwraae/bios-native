use std::path::{Path, PathBuf};
use std::time::Duration;

use craft_dialogs::{Answer, MessageResult};

use crate::{App, Then, label, menus};

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../crates/bios-core/tests/fixtures").join(name)
}

/// A window's state with a new project open, as File > New Project leaves it.
fn app() -> (tempfile::TempDir, App) {
    let dir = tempfile::tempdir().unwrap();
    let mut app = App::new(None, Duration::ZERO);
    app.answered(Then::NewProject, Answer::Paths(vec![dir.path().join("My Lab")]));
    (dir, app)
}

fn paths(app: &App) -> Vec<String> {
    app.listing.iter().map(|l| label(&l.path)).collect()
}

#[test]
fn a_new_project_is_made_where_the_person_chose_and_opened_under_the_folders_name() {
    let (dir, app) = app();
    assert_eq!(app.problem, None);
    assert_eq!(app.session.as_ref().map(|s| s.project().name().to_string()).as_deref(), Some("My Lab"));
    assert!(dir.path().join("My Lab/bios.json").is_file());
    assert_eq!(app.menu_state(), menus::State { project: true, ..menus::State::default() });
}

#[test]
fn importing_several_files_adds_each_and_leaves_the_last_one_open() {
    let (_dir, mut app) = app();
    app.answered(Then::Import, Answer::Paths(vec![fixture("wrap_demo.gb"), fixture("gfp.fasta")]));
    assert_eq!(app.problem, None);
    assert_eq!(paths(&app), ["pGFP_demo", "wrap_demo"]);
    assert_eq!(app.opened.as_ref().map(|o| o.construct.name.as_str()), Some("pGFP_demo"));
    // A file that is not a sequence is refused in a sentence, and nothing is added.
    let notes = _dir.path().join("notes.txt");
    std::fs::write(&notes, "Order more tips on Tuesday.\n").unwrap();
    app.answered(Then::Import, Answer::Paths(vec![notes]));
    assert!(app.problem.as_deref().is_some_and(|p| p.starts_with("notes.txt doesn't look like any sequence format")), "{:?}", app.problem);
    assert_eq!(paths(&app).len(), 2);
}

#[test]
fn duplicate_makes_a_copy_under_a_free_name_and_opens_it() {
    let (_dir, mut app) = app();
    app.answered(Then::Import, Answer::Paths(vec![fixture("wrap_demo.gb")]));
    app.command(menus::DUPLICATE);
    assert_eq!(app.opened.as_ref().map(|o| o.construct.name.as_str()), Some("wrap_demo copy"));
    assert_eq!(app.opened.as_ref().and_then(|o| o.lineage.made_from.as_deref()), Some("wrap_demo"));
    app.command(menus::DUPLICATE);
    assert_eq!(paths(&app), ["wrap_demo copy copy", "wrap_demo copy", "wrap_demo"]);
}

#[test]
fn rename_move_and_new_folder_ask_for_a_line_and_go_through_the_commands() {
    let (_dir, mut app) = app();
    app.answered(Then::Import, Answer::Paths(vec![fixture("wrap_demo.gb")]));
    let id = app.opened.as_ref().map(|o| o.id.clone()).unwrap();

    app.command(menus::RENAME);
    let (what, prompt) = app.prompt.clone().unwrap();
    assert_eq!((prompt.label.as_str(), prompt.text.as_str()), ("Rename wrap_demo", "wrap_demo"));
    app.asked(&what, "wrap v2");
    assert_eq!(app.problem, None);
    assert_eq!(paths(&app), ["wrap v2"]);
    assert_eq!(app.opened.as_ref().map(|o| (o.id.clone(), o.construct.name.clone())), Some((id.clone(), "wrap v2".to_string())));

    app.command(menus::MOVE);
    let (what, prompt) = app.prompt.clone().unwrap();
    assert_eq!(prompt.text, "", "it is at the top of the project");
    app.asked(&what, "backbones/amp");
    assert_eq!(paths(&app), ["backbones/amp/wrap v2"]);
    app.command(menus::MOVE);
    assert_eq!(app.prompt.as_ref().map(|(_, p)| p.text.as_str()), Some("backbones/amp"));

    app.command(menus::NEW_FOLDER);
    let (what, _) = app.prompt.clone().unwrap();
    app.asked(&what, "parts");
    assert!(app.folders.iter().any(|f| f.as_os_str() == "parts"), "{:?}", app.folders);
    // The history is still the plasmid's own after the rename and the move.
    assert_eq!(app.opened.as_ref().unwrap().history.first().map(|e| e.command.as_str()), Some("import_plasmid"));
}

#[test]
fn export_writes_a_file_outside_the_project_in_the_format_its_name_asks_for() {
    let (dir, mut app) = app();
    app.answered(Then::Import, Answer::Paths(vec![fixture("wrap_demo.gb")]));
    let id = app.opened.as_ref().map(|o| o.id.clone()).unwrap();
    app.point(0, 0, 0.0);
    app.edit(|e| e.type_text("ggg"));
    let (genbank, fasta) = (dir.path().join("out.gb"), dir.path().join("out.FA"));
    app.answered(Then::Export(id.clone()), Answer::Paths(vec![genbank.clone()]));
    app.answered(Then::Export(id.clone()), Answer::Paths(vec![fasta.clone()]));
    assert_eq!(app.problem, None);
    // What was typed a moment ago is in the file: it was saved before the export ran.
    let written = bios_core::formats::read(&std::fs::read(&genbank).unwrap(), "out.gb").unwrap().construct;
    assert_eq!(&written.sequence[..7], "GGGACGT");
    assert_eq!(written.features.iter().map(|f| f.name.as_str()).collect::<Vec<_>>(), ["AmpR", "GFP", "MCS", "synthetic DNA construct"]);
    assert!(std::fs::read_to_string(&fasta).unwrap().starts_with(">wrap_demo\nGGGACGT"));
    // A place inside the project is refused: a plasmid lives in the project once.
    app.answered(Then::Export(id), Answer::Paths(vec![dir.path().join("My Lab/copy.gb")]));
    assert!(app.problem.as_deref().is_some_and(|p| p.contains("inside the project")), "{:?}", app.problem);
}

#[test]
fn folders_that_were_opened_are_remembered_latest_first_and_reopen_from_the_menu() {
    let dir = tempfile::tempdir().unwrap();
    let mut app = App::new(None, Duration::ZERO);
    let file = dir.path().join("settings/recents.json");
    app.recents = bios_project::recents::Recents::at(&file);
    app.answered(Then::NewProject, Answer::Paths(vec![dir.path().join("Lab")]));
    app.answered(Then::NewProject, Answer::Paths(vec![dir.path().join("Thesis")]));
    assert_eq!(app.recent(), ["Thesis", "Lab"]);
    assert_eq!(app.session.as_ref().map(|s| s.project().name().to_string()).as_deref(), Some("Thesis"));
    app.command("project.recent:1");
    assert_eq!(app.session.as_ref().map(|s| s.project().name().to_string()).as_deref(), Some("Lab"));
    assert_eq!(app.recent(), ["Lab", "Thesis"]);
    // The list is the file every surface reads; a folder that is gone is not listed.
    assert_eq!(bios_project::recents::Recents::at(&file).folders().len(), 2);
    std::fs::remove_dir_all(dir.path().join("Lab")).unwrap();
    assert_eq!(app.recent(), ["Thesis"]);
}

#[test]
fn delete_happens_only_when_the_person_says_ok() {
    let (_dir, mut app) = app();
    app.answered(Then::Import, Answer::Paths(vec![fixture("wrap_demo.gb")]));
    let id = app.opened.as_ref().map(|o| o.id.clone()).unwrap();
    app.answered(Then::Delete(id.clone()), Answer::Message(MessageResult::Cancel));
    assert_eq!(paths(&app), ["wrap_demo"]);
    app.answered(Then::Delete(id.clone()), Answer::Cancelled);
    assert_eq!(paths(&app), ["wrap_demo"]);
    app.answered(Then::Delete(id.clone()), Answer::Message(MessageResult::Ok));
    assert_eq!(app.problem, None);
    assert!(paths(&app).is_empty());
    assert!(app.opened.is_none());
    // It is listed as deleted, and one click brings it back with its history.
    assert_eq!(app.deleted.iter().map(|d| d.name.as_str()).collect::<Vec<_>>(), ["wrap_demo"]);
    app.bring_back(&id);
    assert_eq!(app.problem, None);
    assert_eq!(paths(&app), ["wrap_demo"]);
    assert!(app.deleted.is_empty());
    let opened = app.opened.as_ref().unwrap();
    assert_eq!((opened.id.clone(), opened.history.first().map(|e| e.command.as_str())), (id, Some("import_plasmid")));
}

#[test]
fn undo_from_the_menu_takes_back_the_last_change() {
    let (_dir, mut app) = app();
    app.answered(Then::Import, Answer::Paths(vec![fixture("wrap_demo.gb")]));
    assert!(!app.menu_state().undo);
    let id = app.opened.as_ref().map(|o| o.id.clone()).unwrap();
    let named = |app: &App, name: &str| app.opened.as_ref().is_some_and(|o| o.construct.features.iter().any(|f| f.name == name));
    app.run("rename_feature", serde_json::json!({ "plasmid": id.as_str(), "feature": "MCS", "new_name": "polylinker" }));
    assert_eq!(app.problem, None);
    assert!(named(&app, "polylinker") && !named(&app, "MCS"));
    assert!(app.menu_state().undo);
    app.command(menus::UNDO);
    assert!(named(&app, "MCS") && !named(&app, "polylinker"));
}

#[test]
fn any_folder_opens_without_being_written_to_and_a_dialog_that_could_not_show_is_reported() {
    let (dir, mut app) = app();
    let downloads = dir.path().join("Downloads");
    std::fs::create_dir(&downloads).unwrap();
    std::fs::copy(fixture("pUC19.gb"), downloads.join("pUC19.gb")).unwrap();
    app.answered(Then::OpenFolder, Answer::Paths(vec![downloads.clone()]));
    assert_eq!(app.problem, None);
    assert_eq!(paths(&app), ["pUC19"]);
    assert_eq!(std::fs::read_dir(&downloads).unwrap().count(), 1, "opening wrote into the folder");
    app.answered(Then::OpenFolder, Answer::Unavailable("no desktop portal".into()));
    assert_eq!(app.problem.as_deref(), Some("That dialog could not be shown: no desktop portal"));
}
