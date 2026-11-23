//! The folders a person opened: the list every surface of Bios finds projects by.

use std::fs;
use std::path::PathBuf;

use bios_project::recents::{List, Recents};

fn folders(n: usize) -> (tempfile::TempDir, Vec<PathBuf>) {
    let dir = tempfile::tempdir().unwrap();
    let made = (0..n)
        .map(|i| {
            let p = dir.path().join(format!("p{i}"));
            fs::create_dir(&p).unwrap();
            p.canonicalize().unwrap()
        })
        .collect();
    (dir, made)
}

#[test]
fn the_folder_opened_last_is_first_and_is_listed_once() {
    let (dir, f) = folders(3);
    let file = dir.path().join("settings/recents.json");
    let mut recents = Recents::at(&file);
    for i in [0, 1, 0, 2] {
        recents.opened(&f[i]).unwrap();
    }
    assert_eq!(recents.folders(), [f[2].clone(), f[0].clone(), f[1].clone()]);
    // Another program reading the same file sees the same list.
    assert_eq!(Recents::at(&file).folders(), recents.folders());
    let written: List = serde_json::from_str(&fs::read_to_string(&file).unwrap()).unwrap();
    assert_eq!(written.v, 1);
    assert!(written.folders.iter().all(|r| !r.opened_at.is_empty()));
}

#[test]
fn pins_and_order_follow_a_folder_and_go_when_it_leaves_the_list() {
    let (dir, f) = folders(3);
    let mut recents = Recents::at(&dir.path().join("recents.json"));
    for p in &f {
        recents.opened(p).unwrap();
    }
    recents.set_pins(vec![f[1].clone(), dir.path().join("never opened")]).unwrap();
    recents.set_order(vec![f[0].clone(), f[1].clone(), f[2].clone()]).unwrap();
    assert_eq!(recents.list().pins, [f[1].clone()]);

    let renamed = dir.path().canonicalize().unwrap().join("renamed");
    fs::rename(&f[1], &renamed).unwrap();
    recents.moved(&f[1], &renamed).unwrap();
    let list = recents.list();
    assert_eq!(list.pins, std::slice::from_ref(&renamed));
    assert_eq!(list.order, [f[0].clone(), renamed.clone(), f[2].clone()]);

    recents.forget(&renamed).unwrap();
    let list = recents.list();
    assert!(list.pins.is_empty());
    assert_eq!(list.order, [f[0].clone(), f[2].clone()]);
    assert!(renamed.is_dir(), "forgetting a folder never touches it");
}

#[test]
fn a_folder_that_is_gone_is_kept_but_not_listed() {
    let (dir, f) = folders(2);
    let mut recents = Recents::at(&dir.path().join("recents.json"));
    recents.opened(&f[0]).unwrap();
    recents.opened(&f[1]).unwrap();
    fs::remove_dir(&f[0]).unwrap();
    assert_eq!(recents.folders(), [f[1].clone()]);
    assert_eq!(recents.list().folders.len(), 2);
}

#[test]
fn a_damaged_file_reads_as_empty_and_is_replaced_on_the_next_change() {
    let (dir, f) = folders(1);
    let file = dir.path().join("recents.json");
    fs::write(&file, "{ half").unwrap();
    let mut recents = Recents::at(&file);
    assert!(recents.folders().is_empty());
    recents.opened(&f[0]).unwrap();
    assert_eq!(recents.folders(), [f[0].clone()]);
}

#[test]
fn a_list_in_memory_writes_no_file() {
    let (dir, f) = folders(1);
    let mut recents = Recents::in_memory();
    recents.opened(&f[0]).unwrap();
    assert_eq!(recents.folders(), [f[0].clone()]);
    assert_eq!(recents.file(), None);
    assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 1);
}

#[test]
fn a_list_read_before_sees_a_change_another_program_made() {
    let (dir, f) = folders(2);
    let file = dir.path().join("settings/recents.json");
    let mut mine = Recents::at(&file);
    mine.opened(&f[0]).unwrap();
    assert_eq!(mine.folders(), [f[0].clone()]);
    // The window keeps the list it read; another program then adds to the file.
    Recents::at(&file).opened(&f[1]).unwrap();
    assert_eq!(mine.folders(), [f[1].clone(), f[0].clone()]);
    fs::remove_file(&file).unwrap();
    assert!(mine.folders().is_empty());
}
