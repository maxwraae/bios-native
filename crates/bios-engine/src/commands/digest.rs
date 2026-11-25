//! Restriction digests and restriction cloning: what a digest gives on a gel, and what ligating
//! one fragment into an opened vector gives.

use bios_core::digest::{
    CarriedFeature, Cut, DigestResult, Fragment, FragmentEnd, Junction, LigationProduct, Orientation, Piece, can_self_close, digest,
    fragment_features, joins, ligate, site_survives_at,
};
use bios_core::enzymes::{OverhangKind, get_enzyme};
use bios_core::{Construct, Feature, Segment, Strand};
use serde_json::{Value, json};

use super::files::created;
use super::sequence::{bases, in_range, new_bios_id, slice, span_len};
use super::{PLASMID, feature_summary};
use crate::{Args, CommandSpec, Effect, Failure, Kind, Param, Run, Session, fail, refs};

const ENZYMES: Param = Param {
    name: "enzymes",
    kind: Kind::Strings,
    required: true,
    description: "The enzymes in the reaction, as [\"EcoRI\", \"PstI\"]. All of them cut at once, as they do in one tube. A name Bios has no verified cut positions for is refused rather than guessed at.",
};

const DIGEST_START: Param = Param {
    name: "start",
    kind: Kind::Integer,
    required: false,
    description: "Optional: digest only this stretch of the plasmid, from this base, as if it had been cut out or amplified first. The stretch is treated as a linear piece, so its own two ends are not enzyme ends.",
};
const DIGEST_END: Param = Param {
    name: "end",
    kind: Kind::Integer,
    required: false,
    description: "The last base of that stretch. start greater than end runs through the origin of a circular plasmid.",
};

const VECTOR_ENZYMES: Param = Param {
    name: "enzymes",
    kind: Kind::Strings,
    required: true,
    description: "What opens the vector: one enzyme that cuts it once, or two that each cut it once. With two, the shorter of the two pieces leaves and the longer is kept as the backbone.",
};
const FROM_PLASMID: Param = Param {
    name: "from_plasmid",
    kind: Kind::String,
    required: false,
    description: "Take the insert from another plasmid in the project, with either from_enzymes or from_start and from_end.",
};
const FROM_ENZYMES: Param = Param {
    name: "from_enzymes",
    kind: Kind::Strings,
    required: false,
    description: "Cut from_plasmid with these and use the fragment that is not its backbone. If more than one fragment could be meant, Bios lists the candidates and does nothing.",
};
const FROM_START: Param = Param {
    name: "from_start",
    kind: Kind::Integer,
    required: false,
    description: "Or take a stretch of from_plasmid: its first base. The stretch comes with blunt ends, so it needs a blunt-cut vector.",
};
const FROM_END: Param =
    Param { name: "from_end", kind: Kind::Integer, required: false, description: "The last base of that stretch. With from_start." };
const SEQUENCE: Param = Param {
    name: "sequence",
    kind: Kind::String,
    required: false,
    description: "Or give the insert's bases directly, 5' to 3'. With no sequence_ends they are taken as a blunt piece exactly as given.",
};
const SEQUENCE_ENDS: Param = Param {
    name: "sequence_ends",
    kind: Kind::Strings,
    required: false,
    description: "The enzymes whose sites flank the bases you gave, as [\"EcoRI\", \"HindIII\"] (one name means the same enzyme both ends). Bios cuts the sequence with them and ligates the piece between the cuts, so give the sequence with its recognition sites on, the way it would be ordered.",
};

const CLONE: &[Param] = &[PLASMID, VECTOR_ENZYMES, FROM_PLASMID, FROM_ENZYMES, FROM_START, FROM_END, SEQUENCE, SEQUENCE_ENDS];

pub const COMMANDS: &[CommandSpec] = &[
    CommandSpec {
        name: "digest_plasmid",
        description: "Cut a plasmid with one or more restriction enzymes and report the fragments. This is the tool for a diagnostic digest or a virtual gel: do not work fragment sizes out yourself. Fragments come back longest first, as a gel runs them, with each one's length, where it lies on the plasmid (through the origin where it does), what its two ends are (blunt, or a 5' or 3' overhang with its bases) and which enzyme made each end, plus the features that lie whole inside it and the ones it is cut through. A length is the fragment's top strand from one cut to the next, which is the size everyone quotes; the lengths add up to the whole plasmid, and the total is reported so that can be seen. One cut in a circular plasmid gives ONE linear fragment, not two. An enzyme that does not cut is named. An enzyme that cuts outside its recognition site (BsaI and the other Type IIS enzymes) is handled like any other.",
        params: &[PLASMID, ENZYMES, DIGEST_START, DIGEST_END],
        effect: Effect::Read,
        run: Run::Call(digest_plasmid),
    },
    CommandSpec {
        name: "clone_by_restriction",
        description: "Open a vector with restriction enzymes, ligate an insert into it, and report the plasmid that comes out. The insert is a fragment of another plasmid (from_plasmid with from_enzymes), a stretch of one (from_plasmid with from_start and from_end), or bases you give (sequence). Reports the product's sequence and length, each junction and whether the enzyme site is rebuilt there or destroyed, which orientations the ends allow, the features that come along and where they land, and warnings: ends that cannot be ligated, a vector that can close on itself without the insert, a blunt ligation that cannot control orientation, and an insert the vector's own enzymes would cut. Nothing is saved; use save_restriction_clone to keep the product.",
        params: CLONE,
        effect: Effect::Read,
        run: Run::Call(clone_by_restriction),
    },
    CommandSpec {
        name: "save_restriction_clone",
        description: "Run clone_by_restriction and keep the product as a new circular plasmid. It records which vector it was built from, carries the vector's features that survive the opening and the insert's features at their new positions, and marks the insert. When the ends allow both orientations it saves the insert the way round it was given and says so.",
        params: &[
            PLASMID,
            VECTOR_ENZYMES,
            FROM_PLASMID,
            FROM_ENZYMES,
            FROM_START,
            FROM_END,
            SEQUENCE,
            SEQUENCE_ENDS,
            Param { name: "name", kind: Kind::String, required: true, description: "Name of the new plasmid." },
            Param { name: "folder", kind: Kind::String, required: false, description: "Folder for it. Default: the top of the project." },
        ],
        effect: Effect::Write,
        run: Run::Call(save_restriction_clone),
    },
];

/// An overhang as a person reads it: "5' AATT", "3' TGCA" or "blunt".
fn end_text(end: &FragmentEnd) -> String {
    match &end.overhang {
        None => "the end of the linear plasmid, as it came".to_string(),
        Some(o) => match o.kind {
            OverhangKind::Blunt => "blunt".to_string(),
            OverhangKind::FivePrime => format!("5' {}", o.seq),
            OverhangKind::ThreePrime => format!("3' {}", o.seq),
        },
    }
}

fn end_report(end: &FragmentEnd) -> Value {
    json!({ "made_by": end.enzyme, "end": end_text(end) })
}

fn names(construct: &Construct, indexes: &[usize]) -> Vec<String> {
    indexes.iter().filter_map(|i| construct.features.get(*i)).map(|f| f.name.clone()).collect()
}

/// One fragment as an agent reads it. Positions are 1-based and inclusive.
fn fragment_report(fragment: &Fragment, construct: &Construct, total: usize) -> Value {
    let found = fragment_features(fragment, &construct.features, total);
    let inside: Vec<String> = found.inside.iter().filter_map(|c| construct.features.get(c.index)).map(|f| f.name.clone()).collect();
    json!({
        "length": fragment.length,
        "start": fragment.start + 1,
        "end": fragment.end + 1,
        "through_origin": fragment.wraps_origin,
        "left_end": end_report(&fragment.left),
        "right_end": end_report(&fragment.right),
        "features_inside": inside,
        "features_cut_through": names(construct, &found.clipped),
    })
}

fn cut_report(cut: &Cut) -> Value {
    json!({
        "enzyme": cut.enzyme,
        "after_base": cut.top_cut,
        "bottom_after_base": cut.bottom_cut,
        "site_starts": cut.recognition_start + 1,
        "strand": if cut.strand == Strand::Reverse { "reverse" } else { "forward" },
        "leaves": end_text(&FragmentEnd {
            enzyme: Some(cut.enzyme),
            overhang: Some(cut.overhang.clone()),
            top_cut: None,
            bottom_cut: None,
        }),
    })
}

/// The molecule a digest is run on: the whole plasmid, or the stretch `start`..`end` of it taken
/// as a linear piece.
struct Target {
    sequence: String,
    circular: bool,
    /// The construct the features are read from, with every feature moved onto the target.
    construct: Construct,
    of: String,
}

fn target(session: &Session, args: &Args) -> Result<Target, Failure> {
    let loaded = session.load(args)?;
    let c = &loaded.construct;
    let label = refs::label(&loaded.path);
    let (start, end) = (args.opt_int("start")?, args.opt_int("end")?);
    let (Some(start), Some(end)) = (start, end) else {
        if start.is_some() || end.is_some() {
            return fail("give both start and end, or neither");
        }
        return Ok(Target { sequence: c.sequence.clone(), circular: c.circular, construct: c.clone(), of: label });
    };
    in_range(c, start, end)?;
    if end < start && !c.circular {
        return fail("that stretch runs through the origin, but this plasmid is linear");
    }
    let length = span_len(c, start, end) as usize;
    let moved = moved_features(c, (start - 1) as usize, length);
    Ok(Target {
        sequence: slice(c, start, end),
        circular: false,
        construct: Construct { features: moved, ..c.clone() },
        of: format!("{label} {start}..{end}"),
    })
}

/// The features of `c` that lie whole inside the stretch of `length` bases from `from`, at their
/// places on it.
fn moved_features(c: &Construct, from: usize, length: usize) -> Vec<Feature> {
    let whole = Fragment {
        start: from,
        end: (from + length - 1) % c.len().max(1),
        length,
        wraps_origin: from + length > c.len(),
        sequence: String::new(),
        left: FragmentEnd::as_it_came(),
        right: FragmentEnd::as_it_came(),
    };
    let found = fragment_features(&whole, &c.features, c.len());
    found.inside.iter().filter_map(|carried| c.features.get(carried.index).map(|f| at(f, carried))).collect()
}

/// `f` moved to where `carried` says it now sits.
fn at(f: &Feature, carried: &CarriedFeature) -> Feature {
    let mut moved = f.clone();
    (moved.start, moved.end, moved.segments) = (carried.start, carried.end, vec![Segment::new(carried.start, carried.end)]);
    moved
}

fn digested(target: &Target, args: &Args) -> Result<DigestResult, Failure> {
    let enzymes = args.strings("enzymes");
    if enzymes.is_empty() {
        return fail("name at least one enzyme to cut with");
    }
    digest(&target.sequence, target.circular, &enzymes).map_err(|e| Failure(e.to_string()))
}

fn digest_plasmid(session: &Session, args: &Args) -> Result<Value, Failure> {
    let target = target(session, args)?;
    let result = digested(&target, args)?;
    let total = result.total;
    let fragments: Vec<Value> = result.fragments.iter().map(|f| fragment_report(f, &target.construct, total)).collect();
    let sizes: Vec<usize> = result.fragments.iter().map(|f| f.length).collect();
    let mut notes: Vec<String> = Vec::new();
    if result.uncut_circle {
        notes.push("Nothing cut it, so it is still one circular plasmid and runs as uncut DNA.".to_string());
    }
    if result.fragments.len() == 1 && target.circular && !result.uncut_circle {
        notes.push("One cut in a circular plasmid: the whole plasmid comes off as one linear fragment.".to_string());
    }
    if !result.non_cutters.is_empty() {
        notes.push(format!("Does not cut this DNA: {}.", result.non_cutters.join(", ")));
    }
    Ok(json!({
        "of": target.of,
        "topology": if target.circular { "circular" } else { "linear" },
        "cuts": result.cuts.len(),
        "cut_sites": result.cuts.iter().map(cut_report).collect::<Vec<_>>(),
        "non_cutters": result.non_cutters,
        "fragment_count": result.fragments.len(),
        "sizes_on_a_gel": sizes,
        "total_bp": total,
        "fragments": fragments,
        "notes": notes,
    }))
}

// ---------------------------------------------------------------------------
// Restriction cloning.
// ---------------------------------------------------------------------------

/// The vector once it is open.
struct Opened {
    label: String,
    id: bios_project::ConstructId,
    construct: Construct,
    backbone: Fragment,
    /// The piece that leaves, when two enzymes take one out.
    removed: Option<Fragment>,
    enzymes: Vec<&'static str>,
}

fn open_vector(session: &Session, args: &Args) -> Result<Opened, Failure> {
    let loaded = session.load(args)?;
    let c = loaded.construct.clone();
    let asked = args.strings("enzymes");
    let mut enzymes: Vec<&'static str> = Vec::new();
    for name in &asked {
        let Some(enz) = get_enzyme(name) else {
            return fail(format!("{name} is not an enzyme Bios knows"));
        };
        if !enzymes.contains(&enz.name) {
            enzymes.push(enz.name);
        }
    }
    if !(1..=2).contains(&enzymes.len()) {
        return fail("give one enzyme that cuts the vector once, or two that each cut it once");
    }
    if !c.circular {
        return fail("the vector has to be circular: a linear one has two ends of its own and cannot be closed into a plasmid here");
    }
    let result = digest(&c.sequence, true, &enzymes).map_err(|e| Failure(e.to_string()))?;
    for enzyme in &enzymes {
        match result.cuts.iter().filter(|cut| cut.enzyme == *enzyme).count() {
            1 => {}
            0 => return fail(format!("{enzyme} does not cut {}", c.name)),
            n => return fail(format!("{enzyme} cuts {} {n} times; the vector has to be opened at one place per enzyme", c.name)),
        }
    }
    let mut fragments = result.fragments.clone();
    let Some(backbone) = fragments.first().cloned() else {
        return fail("the digest left no fragments");
    };
    fragments.remove(0);
    Ok(Opened { label: refs::label(&loaded.path), id: loaded.id, construct: c, backbone, removed: fragments.first().cloned(), enzymes })
}

/// The insert, however the caller gave it.
struct Insert {
    piece: Piece,
    /// Its features, at their places on `piece`.
    features: Vec<Feature>,
    name: String,
    /// How it was described, for the report.
    from: String,
}

fn insert_from(session: &Session, args: &Args) -> Result<Insert, Failure> {
    let ways =
        usize::from(args.has("from_enzymes")) + usize::from(args.has("from_start") || args.has("from_end")) + usize::from(args.has("sequence"));
    if ways != 1 {
        return fail("give the insert in one way: from_plasmid with from_enzymes, or from_plasmid with from_start and from_end, or sequence");
    }
    if args.has("sequence") {
        return insert_from_bases(args);
    }
    let Some(reference) = args.opt_str("from_plasmid")? else {
        return fail("from_plasmid is needed to say which plasmid the insert comes from");
    };
    let listed = session.project().list()?;
    let source = session.project().read(&refs::plasmid(&listed, &reference)?.id)?;
    let label = refs::label(&source.path);
    if args.has("from_enzymes") {
        return insert_from_fragment(&source.construct, &label, args);
    }
    insert_from_stretch(&source.construct, &label, args)
}

fn insert_from_bases(args: &Args) -> Result<Insert, Failure> {
    let given = bases(&args.str("sequence")?)?;
    if given.is_empty() {
        return fail("the insert has no bases");
    }
    let ends = args.strings("sequence_ends");
    if ends.is_empty() {
        let piece = Piece { sequence: given.clone(), left: FragmentEnd::blunt(), right: FragmentEnd::blunt() };
        return Ok(Insert { piece, features: Vec::new(), name: "insert".into(), from: format!("{} bases given, blunt both ends", given.len()) });
    }
    let result = digest(&given, false, &ends).map_err(|e| Failure(e.to_string()))?;
    let cut_by: Vec<&str> = result.cuts.iter().map(|cut| cut.enzyme).collect();
    if !result.non_cutters.is_empty() {
        return fail(format!(
            "sequence_ends names {}, but {} has no site in the bases given. Give the sequence with its recognition sites on, as it would be ordered",
            ends.join(" and "),
            result.non_cutters.join(" and ")
        ));
    }
    // The piece between the cuts: both ends have to be enzyme ends, so the outer pieces are not it.
    let middle: Vec<&Fragment> = result.fragments.iter().filter(|f| f.left.enzyme.is_some() && f.right.enzyme.is_some()).collect();
    let [piece] = middle.as_slice() else {
        let shown: Vec<String> = result.fragments.iter().map(|f| format!("{} bp at {}..{}", f.length, f.start + 1, f.end + 1)).collect();
        return fail(format!(
            "{} cuts the bases given {} times, so there is no single piece between the sites. The pieces are: {}",
            cut_by.join(" and "),
            result.cuts.len(),
            shown.join(", ")
        ));
    };
    Ok(Insert {
        piece: Piece::from(*piece),
        features: Vec::new(),
        name: "insert".into(),
        from: format!("{} bp cut out of the {} bases given with {}", piece.length, given.len(), ends.join(" and ")),
    })
}

fn insert_from_fragment(source: &Construct, label: &str, args: &Args) -> Result<Insert, Failure> {
    let enzymes = args.strings("from_enzymes");
    let result = digest(&source.sequence, source.circular, &enzymes).map_err(|e| Failure(e.to_string()))?;
    if !result.non_cutters.is_empty() {
        return fail(format!("{} does not cut {}", result.non_cutters.join(" and "), source.name));
    }
    // Longest first, so the first fragment is the source's own backbone and never the insert.
    let candidates: Vec<&Fragment> = result.fragments.iter().skip(1).filter(|f| f.left.enzyme.is_some() && f.right.enzyme.is_some()).collect();
    let shown = |fragments: &[&Fragment]| {
        fragments
            .iter()
            .map(|f| format!("{} bp at {}..{} ({} / {})", f.length, f.start + 1, f.end + 1, end_text(&f.left), end_text(&f.right)))
            .collect::<Vec<_>>()
            .join(", ")
    };
    let [fragment] = candidates.as_slice() else {
        return fail(format!(
            "cutting {} with {} leaves {} fragments, and more than one of them could be the insert: {}. Say which stretch you mean with from_start and from_end",
            source.name,
            enzymes.join(" and "),
            result.fragments.len(),
            shown(&candidates)
        ));
    };
    let found = fragment_features(fragment, &source.features, result.total);
    let features = found.inside.iter().filter_map(|carried| source.features.get(carried.index).map(|f| at(f, carried))).collect();
    Ok(Insert {
        piece: Piece::from(*fragment),
        features,
        name: format!("{} fragment", source.name),
        from: format!("{} bp of {label} at {}..{}, cut with {}", fragment.length, fragment.start + 1, fragment.end + 1, enzymes.join(" and ")),
    })
}

fn insert_from_stretch(source: &Construct, label: &str, args: &Args) -> Result<Insert, Failure> {
    let (start, end) = (args.int("from_start")?, args.int("from_end")?);
    in_range(source, start, end)?;
    if end < start && !source.circular {
        return fail("that stretch runs through the origin, but that plasmid is linear");
    }
    let length = span_len(source, start, end) as usize;
    let piece = Piece { sequence: slice(source, start, end), left: FragmentEnd::blunt(), right: FragmentEnd::blunt() };
    Ok(Insert {
        piece,
        features: moved_features(source, (start - 1) as usize, length),
        name: format!("{} {start}..{end}", source.name),
        from: format!("{length} bp of {label} at {start}..{end}, with blunt ends"),
    })
}

struct Designed {
    vector: Opened,
    insert: Insert,
    products: Vec<LigationProduct>,
    warnings: Vec<String>,
}

fn design(session: &Session, args: &Args) -> Result<Designed, Failure> {
    let vector = open_vector(session, args)?;
    let insert = insert_from(session, args)?;
    let opened = Piece::from(&vector.backbone);
    let products = ligate(&opened, &insert.piece);
    if products.is_empty() {
        return fail(unligatable(&opened, &insert));
    }
    let warnings = warnings_for(&vector, &insert, &opened, &products);
    Ok(Designed { vector, insert, products, warnings })
}

fn unligatable(opened: &Piece, insert: &Insert) -> String {
    let flipped = insert.piece.flipped();
    let which = |piece: &Piece| {
        let into = if joins(&opened.right, &piece.left) { "joins" } else { "does not join" };
        let back = if joins(&piece.right, &opened.left) { "joins" } else { "does not join" };
        format!(
            "{} {into} the vector's {} end, and {} {back} its {} end",
            end_text(&piece.left),
            end_text(&opened.right),
            end_text(&piece.right),
            end_text(&opened.left)
        )
    };
    format!(
        "these ends cannot be ligated. As given, the insert's {}. Turned round, the insert's {}. Open the vector with enzymes that leave ends matching the insert's",
        which(&insert.piece),
        which(&flipped)
    )
}

fn warnings_for(vector: &Opened, insert: &Insert, opened: &Piece, products: &[LigationProduct]) -> Vec<String> {
    let mut out = Vec::new();
    if can_self_close(opened) {
        out.push(format!(
            "The opened vector's two ends ({} and {}) can be ligated to each other, so it can close with no insert. That empty vector is the background this cloning has to beat.",
            end_text(&opened.left),
            end_text(&opened.right)
        ));
    }
    if products.len() > 1 {
        out.push("The ends allow both orientations, so the insert can go in either way round and the reaction cannot choose. Half the colonies carry each.".to_string());
    }
    if products.iter().flat_map(|p| p.junctions.iter()).any(|j| j.overhang.kind == OverhangKind::Blunt) {
        out.push(
            "A blunt ligation: there are no overhangs to set the direction, and the two ends of the vector can also join each other.".to_string(),
        );
    }
    // An enzyme that opens the vector and also cuts the insert would cut the product back open.
    let inside = digest(&insert.piece.sequence, false, &vector.enzymes);
    if let Ok(found) = inside {
        for cut in &found.cuts {
            out.push(format!(
                "{} has a site inside the insert, after base {} of it, so the same digest would cut the insert as well.",
                cut.enzyme, cut.top_cut
            ));
        }
    }
    if let Some(removed) = &vector.removed {
        let found = fragment_features(removed, &vector.construct.features, vector.construct.len());
        let lost = names(&vector.construct, &found.inside.iter().map(|c| c.index).collect::<Vec<_>>());
        if !lost.is_empty() {
            out.push(format!("The {} bp piece that leaves the vector carries: {}.", removed.length, lost.join(", ")));
        }
    }
    out
}

fn junction_report(product: &LigationProduct, junction: &Junction) -> Value {
    let mut enzymes: Vec<&'static str> = Vec::new();
    for name in [junction.upstream_enzyme, junction.downstream_enzyme].into_iter().flatten() {
        if !enzymes.contains(&name) {
            enzymes.push(name);
        }
    }
    let sites: Vec<Value> = enzymes
        .iter()
        .map(|name| {
            let survives = site_survives_at(&product.sequence, product.circular, name, junction.at);
            json!({ "enzyme": name, "site": if survives { "rebuilt: the enzyme cuts the product here again" } else { "destroyed: the enzyme no longer cuts here" } })
        })
        .collect();
    let at = if junction.at == 0 {
        format!("through the origin, between base {} and base 1", product.sequence.len())
    } else {
        format!("after base {}", junction.at)
    };
    json!({
        "at": at,
        "joined": format!("{} to {}", junction.upstream_enzyme.unwrap_or("a blunt end"), junction.downstream_enzyme.unwrap_or("a blunt end")),
        "overhang": end_text(&FragmentEnd { enzyme: None, overhang: Some(junction.overhang.clone()), top_cut: None, bottom_cut: None }),
        "sites": sites,
    })
}

fn orientation_word(orientation: Orientation) -> &'static str {
    match orientation {
        Orientation::Forward => "as given",
        Orientation::Reverse => "turned round",
    }
}

/// The product's features: the vector's that survive the opening, then the insert's, then one
/// feature marking the insert.
fn product_features(designed: &Designed, product: &LigationProduct) -> Vec<Feature> {
    let vector = &designed.vector;
    let total = vector.construct.len();
    let found = fragment_features(&vector.backbone, &vector.construct.features, total);
    let mut out: Vec<Feature> =
        found.inside.iter().filter_map(|carried| vector.construct.features.get(carried.index).map(|f| at(f, carried))).collect();
    let insert_len = designed.insert.piece.sequence.len();
    let reversed = product.orientation == Orientation::Reverse;
    for f in &designed.insert.features {
        let span = f.end + 1 - f.start;
        let (start, strand) = if reversed {
            (insert_len - f.end - 1, if f.strand == Strand::Reverse { Strand::Forward } else { Strand::Reverse })
        } else {
            (f.start, f.strand)
        };
        let (start, end) = (product.insert_start + start, product.insert_start + start + span - 1);
        let mut moved = f.clone();
        (moved.start, moved.end, moved.strand, moved.segments) = (start, end, strand, vec![Segment::new(start, end)]);
        out.push(moved);
    }
    out.push(Feature {
        name: designed.insert.name.clone(),
        kind: "misc_feature".into(),
        start: product.insert_start,
        end: product.insert_end,
        strand: Strand::Forward,
        segments: vec![Segment::new(product.insert_start, product.insert_end)],
        notes: vec![("note".into(), vec![new_bios_id()])],
        start_fuzzy: None,
        end_fuzzy: None,
        location_operator: None,
        location_unsupported: None,
    });
    out
}

fn report(designed: &Designed) -> Value {
    let vector = &designed.vector;
    let opened = Piece::from(&vector.backbone);
    let removed = match &vector.removed {
        None => format!("nothing; {} cuts once and the plasmid is only opened", vector.enzymes.join(" and ")),
        Some(piece) => format!("{} bp at {}..{}", piece.length, piece.start + 1, piece.end + 1),
    };
    let products: Vec<Value> = designed
        .products
        .iter()
        .map(|p| {
            json!({
                "insert_orientation": orientation_word(p.orientation),
                "length": p.sequence.len(),
                "topology": "circular",
                "insert_at": format!("{}..{}", p.insert_start + 1, p.insert_end + 1),
                "junctions": p.junctions.iter().map(|j| junction_report(p, j)).collect::<Vec<_>>(),
                "sequence": p.sequence,
            })
        })
        .collect();
    json!({
        "vector": {
            "plasmid": vector.label,
            "opened_with": vector.enzymes,
            "backbone": format!("{} bp at {}..{}", vector.backbone.length, vector.backbone.start + 1, vector.backbone.end + 1),
            "removed": removed,
            "ends": [end_report(&opened.left), end_report(&opened.right)],
        },
        "insert": {
            "from": designed.insert.from,
            "length": designed.insert.piece.sequence.len(),
            "ends": [end_report(&designed.insert.piece.left), end_report(&designed.insert.piece.right)],
            "sequence": designed.insert.piece.sequence,
        },
        "orientations": designed.products.iter().map(|p| orientation_word(p.orientation)).collect::<Vec<_>>(),
        "products": products,
        "warnings": designed.warnings,
    })
}

fn clone_by_restriction(session: &Session, args: &Args) -> Result<Value, Failure> {
    let designed = design(session, args)?;
    let mut out = report(&designed);
    let Some(first) = designed.products.first() else {
        return fail("no product");
    };
    let features = product_features(&designed, first);
    let shown: Vec<Value> =
        features.iter().enumerate().map(|(i, f)| feature_summary(&as_product(&designed, first, features.clone()), i, f)).collect();
    if let Some(map) = out.as_object_mut() {
        map.insert("features_on_the_product".into(), json!(shown));
    }
    Ok(out)
}

fn as_product(designed: &Designed, product: &LigationProduct, features: Vec<Feature>) -> Construct {
    Construct {
        name: designed.vector.construct.name.clone(),
        circular: product.circular,
        sequence: product.sequence.clone(),
        features,
        locus: bios_core::model::Locus { molecule: "DNA".into(), division: "SYN".into(), date: String::new() },
        ..Construct::default()
    }
}

fn save_restriction_clone(session: &Session, args: &Args) -> Result<Value, Failure> {
    let name = args.str("name")?.trim().to_string();
    if name.is_empty() {
        return fail("name is empty");
    }
    let designed = design(session, args)?;
    let Some(product) = designed.products.first() else {
        return fail("no product");
    };
    let features = product_features(&designed, product);
    let mut construct = as_product(&designed, product, features);
    construct.name = name.clone();
    let kept: Vec<Value> = construct.features.iter().enumerate().map(|(i, f)| feature_summary(&construct, i, f)).collect();
    let summary = format!(
        "{} cloned into {} cut with {} ({} bp)",
        designed.insert.name,
        designed.vector.construct.name,
        designed.vector.enzymes.join(" and "),
        construct.len()
    );
    let folder = args.opt_str("folder")?.unwrap_or_default();
    let label = designed.vector.label.clone();
    let id = designed.vector.id.clone();
    let mut made = created(session, "save_restriction_clone", construct, &folder, Some((&id, label)), summary, args)?;
    if let Some(map) = made.as_object_mut() {
        map.insert("design".into(), report(&designed));
        map.insert("features".into(), json!(kept));
    }
    Ok(made)
}
