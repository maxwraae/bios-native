use std::path::Path;
use std::time::Duration;

use craft_dialogs::Answer;

use bios_core::Construct;

use crate::{App, Then};

#[test]
fn choosing_a_search_hit_opens_the_plasmid_with_the_hit_selected_and_in_view() {
    let dir = tempfile::tempdir().unwrap();
    let mut app = App::new(None, Duration::ZERO);
    app.answered(Then::NewProject, Answer::Paths(vec![dir.path().join("Lab")]));
    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../crates/bios-core/tests/fixtures");
    app.answered(Then::Import, Answer::Paths(vec![fixtures.join("pUC19.gb"), fixtures.join("gfp.fasta")]));
    assert_eq!(app.opened.as_ref().unwrap().construct.name, "pGFP_demo");

    // Twenty bases from the middle of the other plasmid, searched for by typing them.
    let (id, wanted) = {
        let listed = app.listing.iter().find(|l| l.name == "pUC19").unwrap();
        let construct = app.session.as_ref().unwrap().project().read(&listed.id).unwrap().construct;
        (listed.id.clone(), construct.sequence[1300..1320].to_string())
    };
    app.query = wanted.to_lowercase();
    app.search();
    let hit = app.found.as_ref().unwrap().iter().find(|f| f.span == Some((1300, 1319))).cloned().expect("the bases were not found");
    assert_eq!(hit.id, id);
    app.go_to(&hit.id, hit.span);
    assert_eq!(app.opened.as_ref().unwrap().construct.name, "pUC19");
    assert_eq!((app.ed.cursor.selection, app.ed.cursor.reveal), (Some((1300, 1319)), Some(1320)));

    // A feature found by name is selected whole.
    app.query = "AmpR".into();
    app.search();
    let feature = app.found.as_ref().unwrap().iter().find(|f| f.title == "AmpR").cloned().unwrap();
    app.go_to(&feature.id, feature.span);
    let (start, end) = app.ed.cursor.selection.unwrap();
    let amp = app.opened.as_ref().unwrap().construct.features.iter().find(|f| f.name == "AmpR").unwrap().clone();
    assert_eq!((start, end), (amp.start, amp.end));
    assert_eq!((start, end), (1625, 2485), "AmpR is complement(1626..2486)");
}

#[test]
fn command_f_finds_in_the_open_plasmid_and_goes_to_the_first_hit() {
    let dir = tempfile::tempdir().unwrap();
    let mut app = App::new(None, Duration::ZERO);
    app.answered(Then::NewProject, Answer::Paths(vec![dir.path().join("Lab")]));
    let file = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../crates/bios-core/tests/fixtures/wrap_demo.gb");
    app.answered(Then::Import, Answer::Paths(vec![file]));
    app.command(crate::menus::FIND);
    assert!(app.ed.find.is_some());
    // The web's bar on the same plasmid and query in Exact: "1 / 80", the hit "11 bp".
    if let Some((bar, ..)) = app.ed.find.as_mut() {
        bar.mode = bios_ui::find_bar::Mode::Exact;
    }
    app.open_find(Some("GACGTACGTAG".into()));
    let (bar, matches, _) = app.ed.find.clone().unwrap();
    let none = bios_ui::find_bar::Aligning::default();
    assert_eq!((bar.count(&matches, &none), matches[0].label.as_str()), ("1 / 80".to_string(), "11 bp"));
    assert_eq!(app.ed.cursor.selection, Some((matches[0].start, matches[0].end)));
    // The project's own search is still a menu item away, without the chord.
    app.command(crate::menus::FIND_PROJECT);
    assert!(app.focus_search);
}

#[test]
fn alignment_is_worked_out_off_the_window_and_its_first_fit_selected_and_kept_as_a_binder() {
    let dir = tempfile::tempdir().unwrap();
    let mut app = App::new(None, Duration::ZERO);
    app.answered(Then::NewProject, Answer::Paths(vec![dir.path().join("Lab")]));
    let file = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../crates/bios-core/tests/fixtures/pUC19.gb");
    app.answered(Then::Import, Answer::Paths(vec![file]));
    // 1300..1334 behind a 5' tail, with three extra bases and one changed base (C to A at 1322).
    let query = "GAATTCAAACTAGAAGAACAGTATCCCTTGGTATATGCGCTCTGCTG";
    app.open_find(Some(query.into()));
    assert_eq!(app.ed.find.as_ref().unwrap().0.mode, bios_ui::find_bar::Mode::Alignment, "Alignment is the default");
    let ctx = egui::Context::default();
    let started = std::time::Instant::now();
    loop {
        let c = app.opened.as_ref().unwrap().construct.clone();
        if app.ed.oligos.aligner.want(&ctx, &c, app.revision, Some(query.into())) {
            break;
        }
        assert!(started.elapsed() < Duration::from_secs(20), "the alignment never came back");
        std::thread::sleep(Duration::from_millis(5));
    }
    app.find_again();
    let (_, matches, _) = app.ed.find.clone().unwrap();
    assert_eq!(matches.len(), 1);
    assert!(matches[0].label.starts_with("36/47 nt aligned"), "{}", matches[0].label);
    // The tail's last A happens to match base 1299, so the fit starts there.
    assert_eq!((matches[0].start, matches[0].end), (1299, 1334));
    assert_eq!(app.ed.cursor.selection, Some((matches[0].start, matches[0].end)));
    assert_eq!(app.ed.oligos.aligner.hits(app.revision).len(), 1);

    app.add_binder(query);
    let c = app.opened.as_ref().unwrap().construct.clone();
    let id = app.opened.as_ref().unwrap().id.clone();
    app.ed.oligos.refresh(Some(&id), &c, app.revision);
    assert_eq!(app.ed.oligos.binders_of(&id).count(), 1);
    app.ed.oligos.aligner.clear();
    assert!(app.ed.oligos.aligner.hits(app.revision).is_empty());
}

#[test]
fn switching_to_exact_takes_the_alignment_off_the_rows_and_back() {
    let c: Construct = bios_core::genbank::read(
        &std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../crates/bios-core/tests/fixtures/pUC19.gb")).unwrap(),
    )
    .unwrap();
    let query = "GAATTCAAACTAGAAGAACAGTATCCCTTGGTATATGCGCTCTGCTG";
    let mut aligner = crate::oligos::Aligner::default();
    let ctx = egui::Context::default();
    let started = std::time::Instant::now();
    while !aligner.want(&ctx, &c, 0, Some(query.into())) {
        assert!(started.elapsed() < Duration::from_secs(20), "the alignment never came back");
        std::thread::sleep(Duration::from_millis(5));
    }
    let drawn = aligner.changed;
    aligner.want(&ctx, &c, 0, None);
    assert!(aligner.hits(0).is_empty());
    assert_ne!(aligner.changed, drawn, "the rows redraw without the fit");
    let hidden = aligner.changed;
    aligner.want(&ctx, &c, 0, Some(query.into()));
    assert_eq!(aligner.hits(0).len(), 1);
    assert_ne!(aligner.changed, hidden, "and redraw with it again");
}
