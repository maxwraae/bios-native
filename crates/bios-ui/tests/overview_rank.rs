//! Parity with Bios `editor/rank.test.ts`: one test here per test there, in the same order, with
//! the same assertions and the same numbers.
//!
//! The original runs against a crowded lab construct through the parser the app uses,
//! deliberately, rather than a hand-written fixture: the file encodes what a crowded construct
//! actually is. That file is private, so every test that reads it is in `private` and needs
//! `BIOS_PRIVATE_FIXTURES` (the file is `crates/bios-core/tests/fixtures/lab_construct.gb` there);
//! the one test that needs no file stays out here. This lives beside the map's painter, which is
//! the first place that has both the parser and the rank, and ends with what the map itself makes
//! of the same file.

use bios_layout::overview::rank::span_of;

/// span: a feature that wraps the origin is two arms, not a negative
#[test]
fn span_a_feature_that_wraps_the_origin_is_two_arms_not_a_negative() {
    assert_eq!(span_of(10, 19, 100, true), 10);
    assert_eq!(span_of(95, 4, 100, true), 10);
    // end < start on a LINEAR molecule is malformed, not a wrap. Guessing a 6kb span here is how a
    // 1bp typo becomes the loudest thing on the map.
    assert_eq!(span_of(95, 4, 100, false), 1);
}

/// The tests that read the lab construct, in the original's order.
mod private {
    use bios_core::Construct;
    use bios_layout::overview::rank::{RankContext, Ranked, labeled_indices, make_rank_context, rank_feature};
    use bios_ui::map::scene_of;

    /// A file in the private fixtures folder, which keeps this repository's layout.
    fn private_fixture(path: &str) -> std::path::PathBuf {
        let root = std::env::var_os("BIOS_PRIVATE_FIXTURES").expect("set BIOS_PRIVATE_FIXTURES to the private fixtures folder");
        std::path::PathBuf::from(root).join(path)
    }

    fn load() -> Construct {
        let text = std::fs::read_to_string(private_fixture("crates/bios-core/tests/fixtures/lab_construct.gb")).expect("the sample plasmid");
        bios_core::genbank::read(&text).expect("a GenBank record")
    }

    fn ranked(c: &Construct) -> Vec<Ranked<'_>> {
        c.features.iter().map(|f| Ranked { kind: &f.kind, name: &f.name, start: f.start as i64, end: f.end as i64 }).collect()
    }

    fn context(c: &Construct) -> RankContext {
        make_rank_context(c.len() as i64, c.circular, &ranked(c))
    }

    /// the sample is the construct we think it is
    #[test]
    #[ignore = "needs BIOS_PRIVATE_FIXTURES: answers recorded from private data"]
    fn the_sample_is_the_construct_we_think_it_is() {
        let c = load();
        assert_eq!(c.len(), 6093);
        // The file says linear on its LOCUS line.
        assert!(!c.circular);
        assert!(c.features.len() > 40, "expected a crowded construct, got {} features", c.features.len());
    }

    /// class outranks size: a 12bp T7 promoter beats a big misc_feature
    #[test]
    #[ignore = "needs BIOS_PRIVATE_FIXTURES: answers recorded from private data"]
    fn class_outranks_size_a_12bp_t7_promoter_beats_a_big_misc_feature() {
        let ctx = context(&load());
        let t7 = rank_feature(Ranked { name: "T7 promoter", kind: "promoter", start: 863, end: 874 }, &ctx);
        let misc = rank_feature(Ranked { name: "some blob", kind: "misc_feature", start: 0, end: 400 }, &ctx);
        assert!(t7 > misc, "T7 ({t7:.1}) should outrank a 400bp misc ({misc:.1})");
    }

    /// informativeness: the file's three `U` features do not earn labels
    #[test]
    #[ignore = "needs BIOS_PRIVATE_FIXTURES: answers recorded from private data"]
    fn informativeness_the_files_three_u_features_do_not_earn_labels() {
        let c = load();
        let ctx = context(&c);
        let us: Vec<Ranked<'_>> = ranked(&c).into_iter().filter(|f| f.name.trim() == "U").collect();
        assert!(us.len() >= 2, "expected repeated U features in the sample, found {}", us.len());
        for u in us {
            assert!(rank_feature(u, &ctx) < 40.0, "\"U\" scored {:.1}, should be culled", rank_feature(u, &ctx));
        }
    }

    /// an unnamed feature has no label to spend and cannot win one
    #[test]
    #[ignore = "needs BIOS_PRIVATE_FIXTURES: answers recorded from private data"]
    fn an_unnamed_feature_has_no_label_to_spend_and_cannot_win_one() {
        let ctx = context(&load());
        assert_eq!(rank_feature(Ranked { name: "", kind: "cds", start: 0, end: 2000 }, &ctx), 0.0);
    }

    /// the landmarks survive: a real construct's headline features keep their labels
    #[test]
    #[ignore = "needs BIOS_PRIVATE_FIXTURES: answers recorded from private data"]
    fn the_landmarks_survive_a_real_constructs_headline_features_keep_their_labels() {
        let c = load();
        let features = ranked(&c);
        let names: Vec<&str> = labeled_indices(&features, &context(&c)).into_iter().map(|i| features[i].name).collect();
        // If a rank change ever drops one of these, it is wrong: these are the things you'd name
        // the plasmid after.
        for must in ["AmpR", "ori", "NeoR/KanR"] {
            assert!(names.contains(&must), "\"{must}\" lost its label; kept: {}", names.join(", "));
        }
    }

    /// the wall is thinned: most of a 53-feature construct does not earn a label
    #[test]
    #[ignore = "needs BIOS_PRIVATE_FIXTURES: answers recorded from private data"]
    fn the_wall_is_thinned_most_of_a_53_feature_construct_does_not_earn_a_label() {
        let c = load();
        let features = ranked(&c);
        let labeled = labeled_indices(&features, &context(&c));
        // A band, not a number: what it guards is the two ways rank can be wrong, leaving the wall
        // standing or starting to delete biology.
        assert!(labeled.len() >= 8, "only {} labels: rank is deleting biology, not thinning it", labeled.len());
        assert!(labeled.len() < features.len() / 2, "{} of {} features labelled: that's still a wall", labeled.len(), features.len());
    }

    /// rank is pure: the same file always yields the same map
    #[test]
    #[ignore = "needs BIOS_PRIVATE_FIXTURES: answers recorded from private data"]
    fn rank_is_pure_the_same_file_always_yields_the_same_map() {
        let c = load();
        let features = ranked(&c);
        let ctx = context(&c);
        assert_eq!(labeled_indices(&features, &ctx), labeled_indices(&features, &ctx));
    }

    // ---- Not in the original: what the map makes of the same file ----

    /// The map draws thirty of the file's fifty-three features: not the source, and not the
    /// twenty-two splice-scan hits. The names are the file's, and the order is the file's.
    #[test]
    #[ignore = "needs BIOS_PRIVATE_FIXTURES: answers recorded from private data"]
    fn the_map_draws_the_files_own_features_without_the_scan_hits() {
        let c = load();
        let scene = scene_of(&c);
        assert_eq!(c.features.len(), 53);
        assert_eq!(scene.features.len(), 30);
        assert!(scene.features.iter().all(|f| f.kind != "source" && !f.name.ends_with("SM")));
        assert_eq!(scene.features[0].name, "CMV enhancer");
        assert_eq!(c.features[scene.features[29].source].name, "HHR");
        assert_eq!((scene.priorities.len(), scene.lanes.len()), (30, 30));
        // The cassette nests four deep: the stop codon sits on the fourth lane.
        assert_eq!(scene.lanes.iter().max(), Some(&3));
    }
}
