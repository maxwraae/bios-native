//! Every enzyme in the table, against one molecule: the model behind the Enzymes panel. Ported
//! from the Bios web app's `editor/enzymeList.ts` and `describeEnzyme` in `editor/cutsites.ts`
//! (141-156).
//!
//! The scissors dial answers "how crowded should the map be". The questions it cannot answer are
//! list questions: does BsaI cut this, what is Eco31I, which leave blunt ends, where does NotI land.
//! This is that list, worked out in one pass over the whole table, grouped by how often each enzyme
//! cuts. An enzyme that does not cut is a row too: "nothing I picked cuts here" is an answer.

use std::collections::HashMap;
use std::collections::HashSet;

use crate::enzymes::{COMMON_ENZYMES, CutSite, enzymes, find_cut_sites, get_enzyme};

/// An enzyme described from its name alone (`EnzymeDetail`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EnzymeDetail {
    pub name: &'static str,
    /// The recognition site with `^` at the top-strand cut, as `G^AATTC`. `None` for Type IIS,
    /// which cuts outside its site, and for an entry with no supported cleavage.
    pub caret_site: Option<String>,
    pub site: &'static str,
    /// `5′ AATT`, `3′ TGCA` or `blunt`. Empty for Type IIS: its overhang depends on where it lands.
    pub overhang_text: String,
    pub type2s: bool,
}

/// `describeEnzyme`: what a hover card or a list row says about an enzyme.
pub fn describe_enzyme(name: &str) -> Option<EnzymeDetail> {
    let e = get_enzyme(name)?;
    let length = i64::try_from(e.length).ok()?;
    let top = e.top_snip_offset();
    let bottom = e.bottom_snip_offset();
    let caret_site = top
        .filter(|t| !e.type2s && *t >= 0 && *t <= length)
        .and_then(|t| usize::try_from(t).ok())
        .map(|t| format!("{}^{}", e.site.get(..t).unwrap_or_default(), e.site.get(t..).unwrap_or_default()));
    let overhang_text = match (e.type2s, top, bottom) {
        (false, Some(t), Some(b)) if t == b => "blunt".to_string(),
        (false, Some(t), Some(b)) => {
            let lo = usize::try_from(t.min(b)).unwrap_or(0);
            let hi = usize::try_from(t.max(b)).unwrap_or(0).min(e.site.len());
            format!("{} {}", if t < b { "5′" } else { "3′" }, e.site.get(lo..hi).unwrap_or_default())
        }
        _ => String::new(),
    };
    Some(EnzymeDetail { name: e.name, caret_site, site: e.site, overhang_text, type2s: e.type2s })
}

/// One enzyme, with everything this molecule has to say about it (`EnzymeRow`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EnzymeRow {
    pub name: &'static str,
    /// Isoschizomers: the other names the same site and cut go by.
    pub aliases: Vec<&'static str>,
    pub site: &'static str,
    pub caret_site: Option<String>,
    pub overhang_text: String,
    pub type2s: bool,
    pub blunt: bool,
    /// Every site on this molecule, in order. Empty means it does not cut it.
    pub sites: Vec<CutSite>,
}

/// Which enzymes the list is willing to show (`EnzymeFilter`). It rests on the common set.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum EnzymeFilter {
    #[default]
    Common,
    All,
    TypeIis,
    Blunt,
}

impl EnzymeFilter {
    /// The web's spelling: `common`, `all`, `type2s`, `blunt`.
    pub fn parse(text: &str) -> Option<EnzymeFilter> {
        match text.trim().to_ascii_lowercase().as_str() {
            "common" => Some(EnzymeFilter::Common),
            "all" => Some(EnzymeFilter::All),
            "type2s" | "iis" => Some(EnzymeFilter::TypeIis),
            "blunt" => Some(EnzymeFilter::Blunt),
            _ => None,
        }
    }
}

/// The whole table, each enzyme carrying its sites on `seq`, sorted by name (`enzymeRows`).
pub fn enzyme_rows(seq: &str, circular: bool) -> Vec<EnzymeRow> {
    let table = enzymes();
    let mut by_enzyme: HashMap<&str, Vec<CutSite>> = HashMap::new();
    if !seq.is_empty() {
        let names: Vec<&str> = table.iter().map(|e| e.name).collect();
        for c in find_cut_sites(seq, &names, circular) {
            by_enzyme.entry(c.enzyme).or_default().push(c);
        }
    }
    let mut rows: Vec<EnzymeRow> = table
        .iter()
        .map(|e| {
            let d = describe_enzyme(e.name);
            let overhang_text = d.as_ref().map(|d| d.overhang_text.clone()).unwrap_or_default();
            EnzymeRow {
                name: e.name,
                aliases: e.aliases.iter().copied().filter(|a| *a != e.name).collect(),
                site: e.site,
                caret_site: d.and_then(|d| d.caret_site),
                blunt: overhang_text == "blunt",
                overhang_text,
                type2s: e.type2s,
                sites: by_enzyme.remove(e.name).unwrap_or_default(),
            }
        })
        .collect();
    rows.sort_by(|a, b| name_order(a.name, b.name));
    rows
}

/// Alphabetical as a person reads a list: case does not split it, and only breaks a tie.
pub fn name_order(a: &str, b: &str) -> std::cmp::Ordering {
    a.to_lowercase().cmp(&b.to_lowercase()).then_with(|| a.cmp(b))
}

/// What a search matched on, so a row can say why it is listed (`EnzymeHit`).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct EnzymeHit {
    /// The isoschizomer that matched, when the canonical name did not.
    pub via: Option<&'static str>,
    /// The recognition sequence matched rather than a name.
    pub by_site: bool,
}

/// Does this row answer the query, by name, by an alias, or by its bases (`matchEnzyme`)?
pub fn match_enzyme(row: &EnzymeRow, query: &str) -> Option<EnzymeHit> {
    let q = query.trim().to_uppercase();
    if q.is_empty() || row.name.to_uppercase().contains(&q) {
        return Some(EnzymeHit::default());
    }
    if let Some(via) = row.aliases.iter().find(|a| a.to_uppercase().contains(&q)) {
        return Some(EnzymeHit { via: Some(via), by_site: false });
    }
    // Bases only, so a caret or a space copied out of a protocol is not a miss.
    let bases: String = q.chars().filter(char::is_ascii_uppercase).collect();
    (!bases.is_empty() && row.site.contains(&bases)).then_some(EnzymeHit { via: None, by_site: true })
}

fn passes(row: &EnzymeRow, filter: EnzymeFilter, common: &HashSet<String>) -> bool {
    match filter {
        EnzymeFilter::All => true,
        EnzymeFilter::TypeIis => row.type2s,
        EnzymeFilter::Blunt => row.blunt,
        EnzymeFilter::Common => common.contains(&row.name.to_uppercase()),
    }
}

/// The headings, in the order the panel draws them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GroupKey {
    Shown,
    Once,
    Twice,
    Few,
    None,
}

impl GroupKey {
    pub fn label(self) -> &'static str {
        match self {
            GroupKey::Shown => "On the map",
            GroupKey::Once => "Cuts once",
            GroupKey::Twice => "Cuts twice",
            GroupKey::Few => "Cuts 3 or more",
            GroupKey::None => "Doesn't cut",
        }
    }

    /// The web's key: `shown`, `once`, `twice`, `few`, `none`.
    pub fn key(self) -> &'static str {
        match self {
            GroupKey::Shown => "shown",
            GroupKey::Once => "once",
            GroupKey::Twice => "twice",
            GroupKey::Few => "few",
            GroupKey::None => "none",
        }
    }
}

/// One heading and its rows (`EnzymeGroup`): indices into the rows given, and why each matched.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EnzymeGroup {
    pub key: GroupKey,
    pub rows: Vec<(usize, EnzymeHit)>,
}

/// Groups the rows by how often each cuts (`groupRows`). What is on the map (`shown`) comes first
/// whatever the filter says, but a query narrows it too. A query is not held to the resting common
/// set: it looks at the whole table, while a class filter (IIS, blunt) still applies.
pub fn group_rows(rows: &[EnzymeRow], query: &str, filter: EnzymeFilter, shown: &[String]) -> Vec<EnzymeGroup> {
    let scope = if !query.trim().is_empty() && filter == EnzymeFilter::Common { EnzymeFilter::All } else { filter };
    let is_shown: HashSet<String> = shown.iter().map(|n| n.to_uppercase()).collect();
    let common: HashSet<String> = COMMON_ENZYMES.iter().map(|n| n.to_uppercase()).collect();
    let mut groups: Vec<EnzymeGroup> =
        [GroupKey::Shown, GroupKey::Once, GroupKey::Twice, GroupKey::Few, GroupKey::None].map(|key| EnzymeGroup { key, rows: Vec::new() }).into();
    for (i, row) in rows.iter().enumerate() {
        let Some(hit) = match_enzyme(row, query) else { continue };
        let key = if is_shown.contains(&row.name.to_uppercase()) {
            GroupKey::Shown
        } else if !passes(row, scope, &common) {
            continue;
        } else {
            match row.sites.len() {
                0 => GroupKey::None,
                1 => GroupKey::Once,
                2 => GroupKey::Twice,
                _ => GroupKey::Few,
            }
        };
        if let Some(group) = groups.iter_mut().find(|g| g.key == key) {
            group.rows.push((i, hit));
        }
    }
    groups
}
