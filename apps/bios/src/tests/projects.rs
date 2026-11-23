//! The projects page: the folders opened lately, and what its cards, its tiles and a drop do.

use std::path::{Path, PathBuf};
use std::time::Duration;

use bios_project::recents::Recents;
use bios_ui::projects_home::Chosen;

use crate::App;

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../crates/bios-core/tests/fixtures").join(name)
}

/// A window whose list holds "Lab" (a project with one plasmid) and "Downloads" (a plain folder
/// of GenBank files), with the list kept in the test's own file.
fn app() -> (tempfile::TempDir, App) {
    let dir = tempfile::tempdir().unwrap();
    let base = dir.path().canonicalize().unwrap();
    let lab = bios_project::home::create_in(&base, "Lab").unwrap();
    lab.import(&fixture("wrap_demo.gb"), "", None, &bios_project::Who { actor: bios_project::Actor::Human, source: "test".into(), session: None })
        .unwrap();
    let plain = base.join("Downloads");
    std::fs::create_dir(&plain).unwrap();
    std::fs::copy(fixture("pUC19.gb"), plain.join("pUC19.gb")).unwrap();
    std::fs::copy(fixture("gfp.fasta"), plain.join("gfp.fasta")).unwrap();
    let mut app = App::new(None, Duration::ZERO);
    app.recents = Recents::at(&base.join("settings/recents.json"));
    app.recents.opened(lab.root()).unwrap();
    app.recents.opened(&plain).unwrap();
    app.new_place = Some(base.clone());
    app.read_cards(true);
    (dir, app)
}

fn id_of(app: &App, name: &str) -> String {
    app.page_cards().into_iter().find(|c| c.name == name).map(|c| c.id).unwrap()
}

#[test]
fn the_page_lists_the_folders_opened_latest_first_with_their_counts_and_places() {
    let (dir, app) = app();
    let cards = app.page_cards();
    let names: Vec<&str> = cards.iter().map(|c| c.name.as_str()).collect();
    assert_eq!(names, ["Downloads", "Lab"]);
    assert!(cards[0].meta.starts_with("2 constructs"), "{}", cards[0].meta);
    assert!(cards[1].meta.starts_with("1 construct · "), "{}", cards[1].meta);
    assert!(cards[1].thumb.is_some(), "a project with a plasmid draws its map");
    assert_eq!(cards[1].id, dir.path().canonicalize().unwrap().join("Lab").display().to_string());
    assert!(!cards[0].place.is_empty());
    assert_eq!(app.page_counts(), (2, 0));
    assert!(!dir.path().join("Downloads/bios.json").exists(), "listing a folder wrote into it");
}

#[test]
fn a_card_opens_its_folder_and_leaving_comes_back_to_the_page() {
    let (dir, mut app) = app();
    let plain = id_of(&app, "Downloads");
    app.page_chose(Chosen::Open(plain));
    let root = dir.path().canonicalize().unwrap().join("Downloads");
    assert_eq!(app.session.as_ref().map(|s| s.project().root().to_path_buf()), Some(root.clone()));
    assert_eq!(app.listing.len(), 2);
    assert!(!root.join("bios.json").exists());
    app.leave_project();
    assert!(app.session.is_none());
    assert_eq!(app.page_cards().len(), 2);
}

#[test]
fn renaming_a_card_renames_its_folder_where_it_is_and_keeps_its_pin() {
    let (dir, mut app) = app();
    let base = dir.path().canonicalize().unwrap();
    let plain = id_of(&app, "Downloads");
    app.page_chose(Chosen::Pins(vec![plain.clone()]));
    app.page_chose(Chosen::Rename { id: plain, name: "Maps".into() });
    assert_eq!(app.problem, None);
    assert!(base.join("Maps/pUC19.gb").is_file());
    assert!(!base.join("Maps/bios.json").exists());
    assert!(app.page_cards().iter().any(|c| c.name == "Maps"));
    assert_eq!(app.recents.list().pins, [base.join("Maps")]);
    assert_eq!(app.page_counts(), (2, 1));
}

#[test]
fn remove_from_list_forgets_a_folder_and_leaves_it_where_it_is() {
    let (dir, mut app) = app();
    let lab = id_of(&app, "Lab");
    app.page_chose(Chosen::Remove(lab));
    assert_eq!(app.page_cards().len(), 1);
    assert!(dir.path().join("Lab/bios.json").is_file());
    assert!(app.notice.as_deref().is_some_and(|n| n.contains("still at")));
}

#[test]
fn the_order_made_by_hand_is_kept_until_sort_by_recent() {
    let (_dir, mut app) = app();
    let (plain, lab) = (id_of(&app, "Downloads"), id_of(&app, "Lab"));
    app.page_chose(Chosen::Order(vec![lab.clone(), plain.clone()]));
    let names: Vec<String> = app.page_cards().into_iter().map(|c| c.name).collect();
    assert_eq!(names, ["Lab", "Downloads"]);
    app.page_chose(Chosen::SortByRecent);
    let names: Vec<String> = app.page_cards().into_iter().map(|c| c.name).collect();
    assert_eq!(names, ["Downloads", "Lab"]);
}

#[test]
fn files_dropped_on_the_page_start_a_project_in_the_place_new_projects_go() {
    let (dir, mut app) = app();
    app.start_from_files(&[fixture("pUC19.gb"), fixture("wrap_demo.gb")]);
    assert!(dir.path().join("pUC19/bios.json").is_file());
    assert_eq!(app.listing.len(), 2);
    assert!(app.opened.is_some());
    assert_eq!(app.recent().first().map(String::as_str), Some("pUC19"));
}
