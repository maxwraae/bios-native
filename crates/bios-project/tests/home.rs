//! Project folders: cards, making and renaming them where they are, and a folder of projects.

use std::fs;
use std::path::{Path, PathBuf};

use bios_project::home::Home;
use bios_project::{Actor, Error, Project, Who};

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../bios-core/tests/fixtures").join(name)
}

fn me() -> Who {
    Who { actor: Actor::Human, source: "test".into(), session: None }
}

fn home() -> (tempfile::TempDir, Home) {
    let dir = tempfile::tempdir().unwrap();
    let home = Home::new(dir.path());
    (dir, home)
}

#[test]
fn the_projects_are_the_project_folders_inside_and_nothing_else() {
    let (dir, home) = home();
    home.create("Lab").unwrap();
    home.create("alpha").unwrap();
    fs::create_dir(dir.path().join("not a project")).unwrap();
    fs::create_dir(dir.path().join(".hidden")).unwrap();
    let folders: Vec<String> = home.projects().unwrap().into_iter().map(|(f, _)| f).collect();
    assert_eq!(folders, ["alpha", "Lab"]);
}

#[test]
fn a_card_counts_the_plasmids_and_draws_the_first_one_put_in() {
    let (_dir, home) = home();
    let project = home.create("Lab").unwrap();
    let first = project.import(&fixture("pUC19.gb"), "", None, &me()).unwrap();
    project.import(&fixture("wrap_demo.gb"), "", None, &me()).unwrap();
    let cards = home.cards().unwrap();
    assert_eq!(cards.len(), 1);
    assert_eq!(cards[0].plasmids, 2);
    assert_eq!(cards[0].name, "Lab");
    assert_eq!(cards[0].id, project.id());
    assert!(cards[0].last_changed.is_some());
    assert_eq!(cards[0].lead.as_ref().map(|c| c.name.clone()), Some(first.construct.construct.name));
    let empty = home.create("Empty").unwrap();
    let card = bios_project::home::card(&empty);
    assert_eq!((card.plasmids, card.lead), (0, None));
}

#[test]
fn creating_refuses_a_name_that_cannot_be_a_folder_or_is_taken() {
    let (_dir, home) = home();
    home.create("Lab").unwrap();
    assert!(matches!(home.create("Lab"), Err(Error::NameTaken(_))));
    for bad in ["", "  ", ".dot", "a/b", "a:b"] {
        assert!(home.create(bad).is_err(), "{bad:?} was taken");
    }
    assert_eq!(home.free_name("Lab"), "Lab 2");
    assert_eq!(home.free_name("New"), "New");
}

#[test]
fn renaming_moves_the_folder_and_the_name_together() {
    let (dir, home) = home();
    let project = home.create("Lab").unwrap();
    let id = project.id().to_string();
    project.import(&fixture("pUC19.gb"), "", None, &me()).unwrap();
    let renamed = home.rename("Lab", "Cloning").unwrap();
    assert_eq!(renamed.name(), "Cloning");
    assert_eq!(renamed.id(), id);
    assert!(dir.path().join("Cloning/bios.json").is_file());
    assert!(!dir.path().join("Lab").exists());
    assert_eq!(renamed.list().unwrap().len(), 1);
    home.create("Other").unwrap();
    assert!(matches!(home.rename("Cloning", "Other"), Err(Error::NameTaken(_))));
    // The name inside can differ from the folder; renaming to the folder's own name only renames.
    let again = home.rename("cloning", "Cloning").unwrap();
    assert_eq!(again.root(), dir.path().join("Cloning"));
}

#[test]
fn a_plain_folder_is_renamed_where_it_is_without_being_made_a_project() {
    let dir = tempfile::tempdir().unwrap();
    let plain = dir.path().join("Downloads");
    fs::create_dir(&plain).unwrap();
    fs::copy(fixture("wrap_demo.gb"), plain.join("wrap_demo.gb")).unwrap();
    let now = bios_project::home::rename_folder(&plain, "Maps").unwrap();
    assert_eq!(now, dir.path().join("Maps"));
    assert!(!plain.exists());
    assert!(!now.join("bios.json").exists());
    assert_eq!(Project::open(&now).unwrap().name(), "Maps");
    fs::create_dir(dir.path().join("Taken")).unwrap();
    assert!(matches!(bios_project::home::rename_folder(&now, "Taken"), Err(Error::NameTaken(_))));
    assert!(bios_project::home::rename_folder(&now, "a/b").is_err());
}

#[test]
fn a_project_is_made_in_any_folder_and_only_trashed_if_it_is_there() {
    let dir = tempfile::tempdir().unwrap();
    let made = bios_project::home::create_in(dir.path(), "AAV").unwrap();
    assert!(dir.path().join("AAV/bios.json").is_file());
    assert_eq!(made.name(), "AAV");
    assert!(matches!(bios_project::home::create_in(dir.path(), "AAV"), Err(Error::NameTaken(_))));
    assert!(bios_project::home::trash_folder(&dir.path().join("missing")).is_err());
}

#[test]
fn a_project_renamed_in_place_keeps_its_id() {
    let dir = tempfile::tempdir().unwrap();
    let mut project = Project::create(&dir.path().join("p"), "One").unwrap();
    project.rename("Two").unwrap();
    assert_eq!(Project::open(project.root()).unwrap().name(), "Two");
    assert!(project.rename("  ").is_err());
}

#[test]
fn a_card_says_today_yesterday_then_the_date() {
    use bios_project::home::{count_of, when_between};
    let now = chrono::DateTime::parse_from_rfc3339("2026-11-21T12:00:00Z").unwrap().with_timezone(&chrono::Utc);
    assert_eq!(when_between("2026-11-21T01:00:00Z", now), "today");
    assert_eq!(when_between("2026-11-20T11:00:00Z", now), "yesterday");
    assert!(when_between("2026-11-03T12:00:00Z", now).starts_with("Nov "));
    assert_eq!(when_between("not a time", now), "");
    assert_eq!((count_of(1), count_of(4), count_of(0)), ("1 construct".into(), "4 constructs".into(), "0 constructs".into()));
}
