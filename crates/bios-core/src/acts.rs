//! What a feature does, and to what: the construct as a graph. Ported from Bios
//! `editor/identity.ts` (`readActions`, `actionsOf`, `writeActions`, `actedOnBy`) and
//! `editor/interactions.ts` (`graphOfFeatures`, `isLit`).
//!
//! A statement lives on the ACTOR, as a `/note` line `bios-act: <verb> > <id>, <id>`: a verb in
//! free prose, then the Bios ids of what it acts on, or the literal `construct` for the whole
//! molecule. A feature may carry several, one per verb. The other end reads it back through
//! [`acted_on_by`] without anyone typing it twice, so two features can never disagree.
//!
//! The older single-slot form, `bios-for: <id>`, still reads as the verb `serves`, so a file written
//! before verbs existed keeps its one relationship.

use crate::model::Feature;

/// The target that means the whole construct rather than one feature.
pub const FOR_CONSTRUCT: &str = "construct";
const ACT_PREFIX: &str = "bios-act:";

/// One statement: a verb in free prose, and everything it acts on.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Action {
    pub verb: String,
    /// Feature ids, or [`FOR_CONSTRUCT`]. Never empty in a stored action.
    pub targets: Vec<String>,
}

impl Action {
    /// Whitespace runs folded in the verb, targets trimmed and deduplicated in order (`clean`).
    pub fn cleaned(&self) -> Action {
        let verb = self.verb.split_whitespace().collect::<Vec<_>>().join(" ");
        let mut targets: Vec<String> = Vec::new();
        for t in self.targets.iter().map(|t| t.trim()).filter(|t| !t.is_empty()) {
            if !targets.iter().any(|seen| seen == t) {
                targets.push(t.to_string());
            }
        }
        Action { verb, targets }
    }

    /// A verb and at least one target: anything less is not a statement and is never stored.
    pub fn is_statement(&self) -> bool {
        !self.verb.is_empty() && !self.targets.is_empty()
    }

    /// The `/note` line this statement is stored as.
    pub fn line(&self) -> String {
        format!("{ACT_PREFIX} {} > {}", self.verb, self.targets.join(", "))
    }
}

fn notes(f: &Feature) -> &[String] {
    f.note("note").unwrap_or_default()
}

/// The value of a `bios-<key>: value` note that is not an act line; the last one wins.
fn field<'a>(f: &'a Feature, key: &str) -> Option<&'a str> {
    let mut out = None;
    for line in notes(f) {
        if line.starts_with(ACT_PREFIX) {
            continue;
        }
        let Some(rest) = line.strip_prefix("bios-") else { continue };
        let Some((k, v)) = rest.split_once(':') else { continue };
        if k.trim() == key {
            out = Some(v.trim());
        }
    }
    out
}

/// The feature's Bios id (`featureId`).
pub fn feature_id(f: &Feature) -> Option<&str> {
    field(f, "id").filter(|id| !id.is_empty())
}

/// What the feature is, as Bios was told (`bios-category`).
pub fn category(f: &Feature) -> Option<&str> {
    field(f, "category").filter(|c| !c.is_empty())
}

/// The statements written on this feature, in file order. The verb ends at the LAST `>`: a target
/// is an id or `construct` and never holds one, so a verb like `cleaves 5' > 3'` survives.
pub fn read_actions(f: &Feature) -> Vec<Action> {
    notes(f)
        .iter()
        .filter_map(|line| line.strip_prefix(ACT_PREFIX))
        .filter_map(|body| body.rsplit_once('>'))
        .map(|(verb, targets)| Action { verb: verb.to_string(), targets: targets.split(',').map(str::to_string).collect() }.cleaned())
        .filter(Action::is_statement)
        .collect()
}

/// What this feature does, including a statement in the old single-target form (`bios-for`), which
/// reads as `serves`.
pub fn actions_of(f: &Feature) -> Vec<Action> {
    let acts = read_actions(f);
    if !acts.is_empty() {
        return acts;
    }
    match field(f, "for").filter(|t| !t.is_empty()) {
        Some(legacy) => vec![Action { verb: "serves".into(), targets: vec![legacy.to_string()] }],
        None => Vec::new(),
    }
}

/// Replaces this feature's statements (`writeActions`). An action with no verb or no target is
/// dropped. `bios-for` is kept pointed at the first target, as the web does, so a reader that only
/// knows the old field still finds the main relationship. Every other note stays where it was; the
/// act lines go last.
pub fn write_actions(f: &mut Feature, actions: &[Action]) {
    let kept: Vec<Action> = actions.iter().map(Action::cleaned).filter(Action::is_statement).collect();
    let first = kept.first().and_then(|a| a.targets.first()).cloned();
    let position = f.notes.iter().position(|(k, _)| k == "note");
    let values = match position {
        Some(i) => match f.notes.get_mut(i) {
            Some((_, values)) => values,
            None => return,
        },
        None => {
            f.notes.push(("note".into(), Vec::new()));
            match f.notes.last_mut() {
                Some((_, values)) => values,
                None => return,
            }
        }
    };
    let is_for = |l: &String| l.strip_prefix("bios-").and_then(|r| r.split_once(':')).is_some_and(|(k, _)| k.trim() == "for");
    let for_at = values.iter().position(is_for);
    values.retain(|l| !is_for(l) && !l.starts_with(ACT_PREFIX));
    if let Some(target) = first {
        let line = format!("bios-for: {target}");
        match for_at {
            Some(at) => values.insert(at.min(values.len()), line),
            None => values.push(line),
        }
    }
    values.extend(kept.iter().map(Action::line));
    f.notes.retain(|(k, v)| k != "note" || !v.is_empty());
}

/// Who acts on each feature, and how: `(actor index, verb)` per target index, from every feature's
/// statements. Statements on the whole construct and targets that no longer exist are left out.
pub fn acted_on_by(features: &[Feature], target: usize) -> Vec<(usize, String)> {
    let Some(id) = features.get(target).and_then(feature_id) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for (actor, f) in features.iter().enumerate() {
        if feature_id(f).is_none() {
            continue;
        }
        for a in actions_of(f) {
            if a.targets.iter().any(|t| t == id) {
                out.push((actor, a.verb));
            }
        }
    }
    out
}

/// What one statement points at, resolved.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Target {
    Feature(usize),
    Construct,
}

/// One edge of the graph: a feature acting on a feature, or on the whole construct.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Edge {
    /// Index of the feature doing it.
    pub actor: usize,
    pub target: Target,
    pub verb: String,
}

/// Every stated relationship on a construct, by feature index (`graphOfFeatures`).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Graph {
    pub edges: Vec<Edge>,
}

impl Graph {
    /// The graph of these features. A target that no longer exists is dropped rather than drawn
    /// pointing at nothing, and a feature acting on itself draws nothing useful.
    pub fn of(features: &[Feature]) -> Graph {
        let ids: Vec<Option<&str>> = features.iter().map(feature_id).collect();
        let index_of = |id: &str| ids.iter().position(|i| *i == Some(id));
        let mut edges = Vec::new();
        for (actor, f) in features.iter().enumerate() {
            if ids.get(actor).copied().flatten().is_none() {
                continue;
            }
            for action in actions_of(f) {
                for t in &action.targets {
                    let target = if t == FOR_CONSTRUCT {
                        Target::Construct
                    } else {
                        match index_of(t) {
                            Some(i) if i != actor => Target::Feature(i),
                            _ => continue,
                        }
                    };
                    edges.push(Edge { actor, target, verb: action.verb.clone() });
                }
            }
        }
        Graph { edges }
    }

    /// Whether `a` and `b` are wired together, in either direction.
    pub fn partners(&self, a: usize, b: usize) -> bool {
        self.edges.iter().any(|e| (e.actor == a && e.target == Target::Feature(b)) || (e.actor == b && e.target == Target::Feature(a)))
    }

    /// Is this feature part of the question being asked (`isLit`)? With nothing lit, everything is:
    /// a map that dims itself when the pointer is nowhere would always be half off.
    pub fn is_lit(&self, lit: Option<usize>, index: usize) -> bool {
        match lit {
            None => true,
            Some(l) => l == index || self.partners(l, index),
        }
    }

    /// Every edge touching this feature, in either direction (`edgesTouching`).
    pub fn touching(&self, index: usize) -> impl Iterator<Item = &Edge> {
        self.edges.iter().filter(move |e| e.actor == index || e.target == Target::Feature(index))
    }

    /// The verbs already used on the construct, first use first: what the verb box offers.
    pub fn verbs(features: &[Feature]) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        for a in features.iter().flat_map(actions_of) {
            if !out.contains(&a.verb) {
                out.push(a.verb);
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn feature(name: &str, notes: &[&str]) -> Feature {
        Feature {
            name: name.into(),
            kind: "misc_feature".into(),
            notes: if notes.is_empty() { Vec::new() } else { vec![("note".into(), notes.iter().map(|n| n.to_string()).collect())] },
            start: 0,
            end: 0,
            strand: crate::Strand::Forward,
            segments: Vec::new(),
            start_fuzzy: None,
            end_fuzzy: None,
            location_operator: None,
            location_unsupported: None,
        }
    }

    #[test]
    fn the_verb_ends_at_the_last_arrow_and_targets_are_cleaned() {
        let f = feature("pol", &["human", "bios-id: a", "bios-act: cleaves  5' > 3' > b, b , construct"]);
        assert_eq!(read_actions(&f), [Action { verb: "cleaves 5' > 3'".into(), targets: vec!["b".into(), "construct".into()] }]);
    }

    #[test]
    fn the_old_single_slot_reads_as_serves() {
        let f = feature("tag", &["bios-id: a", "bios-for: b"]);
        assert_eq!(actions_of(&f), [Action { verb: "serves".into(), targets: vec!["b".into()] }]);
    }

    #[test]
    fn writing_keeps_human_notes_and_points_for_at_the_first_target() {
        let mut f = feature("pol", &["made by hand", "bios-id: a", "bios-for: old", "bios-act: x > old"]);
        write_actions(
            &mut f,
            &[Action { verb: " transcribes ".into(), targets: vec!["b".into(), "c".into()] }, Action { verb: "".into(), targets: vec!["d".into()] }],
        );
        assert_eq!(f.note("note").unwrap(), ["made by hand", "bios-id: a", "bios-for: b", "bios-act: transcribes > b, c"]);
        write_actions(&mut f, &[]);
        assert_eq!(f.note("note").unwrap(), ["made by hand", "bios-id: a"]);
    }

    #[test]
    fn the_graph_drops_ghosts_and_self_edges_and_lights_both_ends() {
        let features = vec![
            feature("pol", &["bios-id: a", "bios-act: transcribes > b, gone, a, construct"]),
            feature("promoter", &["bios-id: b"]),
            feature("other", &["bios-id: c"]),
        ];
        let g = Graph::of(&features);
        assert_eq!(g.edges.len(), 2);
        assert_eq!(g.edges[0].target, Target::Feature(1));
        assert_eq!(g.edges[1].target, Target::Construct);
        assert!(g.is_lit(Some(1), 0) && g.is_lit(Some(0), 1) && !g.is_lit(Some(0), 2) && g.is_lit(None, 2));
        assert_eq!(acted_on_by(&features, 1), [(0, "transcribes".to_string())]);
        assert_eq!(Graph::verbs(&features), ["transcribes"]);
    }
}
