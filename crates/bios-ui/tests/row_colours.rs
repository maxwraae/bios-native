//! The colour of a feature's bar in the rows, as the web app's `colorForFeature` gives it: the
//! key's role first, and the name only for a feature that would otherwise be neutral.

use bios_ui::theme::{feature_color, has_word, hex, row_feature_color};

#[test]
fn a_word_is_matched_whole() {
    assert!(has_word("eblock for new intron", &["intron"]));
    assert!(has_word("sv40 poly a signal", &["poly a"]));
    assert!(has_word("e-block 2", &["e-block"]));
    assert!(!has_word("eblock_for_new_intron", &["intron"]), "an underscore is part of a word");
    assert!(!has_word("introns", &["intron"]));
    assert!(!has_word("", &["intron"]));
}

#[test]
fn a_role_is_kept_and_only_a_neutral_feature_is_named_into_a_colour() {
    let structural = feature_color("misc_feature");
    assert_eq!(row_feature_color("enhancer", "CMV enhancer"), feature_color("enhancer"));
    assert_eq!(row_feature_color("CDS", "stop"), feature_color("CDS"), "a name never repaints genuine biology");
    assert_eq!(row_feature_color("misc_feature", "eblock for new intron"), hex(0x5f7e9b), "scaffolding first, though it holds \"intron\"");
    assert_eq!(row_feature_color("misc_feature", "gibson homology"), hex(0x5f7e9b));
    assert_eq!(row_feature_color("misc_feature", "Exon"), feature_color("CDS"));
    assert_eq!(row_feature_color("misc_feature", "chimeric intron"), hex(0x6fae7f));
    assert_eq!(row_feature_color("misc_feature", "stop"), hex(0x565b66));
    assert_eq!(row_feature_color("rep_origin", "ori"), structural);
    assert_eq!(row_feature_color("misc_feature", "linker"), structural);
}
