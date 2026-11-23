//! Three interchange formats the web app opens: GFF3 with its sequence, JBEI SEQ XML and the
//! Teselagen sequence JSON. Read from the formats' public descriptions (the GFF3 specification of
//! the Sequence Ontology project, the JBEI SEQ XML schema, and the JSON shape Teselagen tools
//! exchange), each into a construct with its features. Coordinates in the result are 0-based and
//! inclusive, as everywhere in Bios.

use crate::model::{Construct, Feature, Locus, Segment, Strand};

/// The most features a file may bring. A map with more is not a plasmid map.
const MAX_FEATURES: usize = 100_000;

fn feature(name: &str, kind: &str, start: usize, end: usize, strand: Strand, notes: Vec<(String, Vec<String>)>) -> Feature {
    Feature {
        name: name.to_string(),
        kind: if kind.is_empty() { "misc_feature".to_string() } else { kind.to_string() },
        start,
        end,
        strand,
        segments: vec![Segment::new(start, end)],
        notes,
        start_fuzzy: None,
        end_fuzzy: None,
        location_operator: None,
        location_unsupported: None,
    }
}

fn construct(name: &str, sequence: &str, circular: bool, features: Vec<Feature>) -> Construct {
    let sequence: String = sequence.chars().filter(char::is_ascii_alphabetic).map(|c| c.to_ascii_uppercase()).collect();
    let length = sequence.len();
    // A feature that runs off the end of the bases is not drawn anywhere true; it is dropped.
    let features = features.into_iter().filter(|f| f.start < length && f.end < length).take(MAX_FEATURES).collect();
    Construct {
        name: name.to_string(),
        circular,
        sequence,
        features,
        locus: Locus { molecule: "DNA".into(), ..Locus::default() },
        ..Construct::default()
    }
}

/// GFF3's percent-encoding (`%3B` for `;`), undone.
fn unescape_gff(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%'
            && let Some(hex) = text.get(i + 1..i + 3)
            && let Ok(b) = u8::from_str_radix(hex, 16)
        {
            out.push(b);
            i += 3;
            continue;
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Reads a GFF3 file: its features on the first sequence it names, and that sequence's bases from
/// the `##FASTA` section. `None` when it holds no bases: a GFF3 of annotations alone is not a map.
pub fn read_gff(text: &str, fallback_name: &str) -> Option<Construct> {
    let mut seqid: Option<String> = None;
    let mut features = Vec::new();
    let mut circular = false;
    let mut fasta: Vec<(String, String)> = Vec::new();
    let mut in_fasta = false;
    for line in text.lines() {
        let line = line.trim_end_matches('\r');
        if in_fasta {
            if let Some(header) = line.strip_prefix('>') {
                fasta.push((header.split_whitespace().next().unwrap_or_default().to_string(), String::new()));
            } else if let Some((_, bases)) = fasta.last_mut() {
                bases.push_str(line.trim());
            }
            continue;
        }
        if line.starts_with("##FASTA") {
            in_fasta = true;
            continue;
        }
        if line.starts_with('#') || line.trim().is_empty() {
            continue;
        }
        let cols: Vec<&str> = line.split('\t').collect();
        let [id, _source, kind, start, end, _score, strand, _phase, attributes] = cols.as_slice() else {
            continue;
        };
        let id = unescape_gff(id);
        if seqid.is_none() {
            seqid = Some(id.clone());
        }
        if seqid.as_deref() != Some(id.as_str()) {
            continue;
        }
        let (Ok(start), Ok(end)) = (start.trim().parse::<usize>(), end.trim().parse::<usize>()) else {
            continue;
        };
        if start == 0 || end < start {
            continue;
        }
        let mut notes: Vec<(String, Vec<String>)> = Vec::new();
        for pair in attributes.split(';').filter(|p| !p.trim().is_empty()) {
            let (key, value) = pair.split_once('=').unwrap_or((pair, ""));
            notes.push((unescape_gff(key.trim()), value.split(',').map(unescape_gff).collect()));
        }
        let note = |key: &str| notes.iter().find(|(k, _)| k == key).and_then(|(_, v)| v.first().cloned());
        if *kind == "region" && note("Is_circular").is_some_and(|v| v == "true") {
            circular = true;
        }
        // The region that spans the whole sequence describes it; it is not a feature on it.
        if *kind == "region" && start == 1 {
            continue;
        }
        let name = note("Name").or_else(|| note("label")).or_else(|| note("ID")).unwrap_or_else(|| kind.to_string());
        let strand = if *strand == "-" { Strand::Reverse } else { Strand::Forward };
        notes.retain(|(k, _)| !matches!(k.as_str(), "Name" | "ID" | "Is_circular"));
        features.push(feature(&name, kind, start - 1, end - 1, strand, notes));
    }
    let name = seqid.clone().unwrap_or_else(|| fallback_name.to_string());
    let bases = fasta.iter().find(|(id, _)| Some(id) == seqid.as_ref()).or_else(|| fasta.first()).map(|(_, b)| b.clone())?;
    if bases.is_empty() {
        return None;
    }
    Some(construct(&name, &bases, circular, features))
}

/// The text inside the first `<tag ...>…</tag>` in `xml`, and what follows it.
fn element<'a>(xml: &'a str, tag: &str) -> Option<(&'a str, &'a str)> {
    let open = format!("<{tag}");
    let mut from = 0;
    loop {
        let at = xml.get(from..)?.find(&open)? + from;
        let after = xml.get(at + open.len()..)?;
        // `<seq:feature` must not match `<seq:features`.
        if after.starts_with(['>', ' ', '\t', '\n', '\r', '/']) {
            let gt = after.find('>')?;
            if after.get(..gt)?.ends_with('/') {
                return Some(("", after.get(gt + 1..)?));
            }
            let body = after.get(gt + 1..)?;
            let close = format!("</{tag}>");
            let end = body.find(&close)?;
            return Some((body.get(..end)?, body.get(end + close.len()..)?));
        }
        from = at + open.len();
    }
}

fn xml_text(xml: &str, tag: &str) -> Option<String> {
    element(xml, tag)
        .map(|(t, _)| t.trim().replace("&lt;", "<").replace("&gt;", ">").replace("&quot;", "\"").replace("&apos;", "'").replace("&amp;", "&"))
}

/// Reads a JBEI SEQ XML file (`<seq:seq>` with `seq:name`, `seq:circular`, `seq:sequence` and
/// `seq:feature`s, each with its `seq:location`s, 1-based).
pub fn read_jbei(text: &str, fallback_name: &str) -> Option<Construct> {
    let (seq, _) = element(text, "seq:seq")?;
    let name = xml_text(seq, "seq:name").filter(|n| !n.is_empty()).unwrap_or_else(|| fallback_name.to_string());
    let circular = xml_text(seq, "seq:circular").is_some_and(|c| c.eq_ignore_ascii_case("true"));
    let bases = xml_text(seq, "seq:sequence")?;
    let mut features = Vec::new();
    let mut rest = seq;
    while let Some((body, after)) = element(rest, "seq:feature") {
        rest = after;
        let label = xml_text(body, "seq:label").unwrap_or_default();
        let kind = xml_text(body, "seq:type").unwrap_or_default();
        let complement = xml_text(body, "seq:complement").is_some_and(|c| c.eq_ignore_ascii_case("true"));
        let mut segments = Vec::new();
        let mut locations = body;
        while let Some((location, after)) = element(locations, "seq:location") {
            locations = after;
            let at = |tag: &str| xml_text(location, tag).and_then(|v| v.parse::<usize>().ok()).filter(|v| *v > 0);
            if let (Some(start), Some(end)) = (at("seq:genbankStart"), at("seq:end")) {
                segments.push(Segment::new(start - 1, end - 1));
            }
        }
        let (Some(first), Some(last)) = (segments.first().copied(), segments.last().copied()) else {
            continue;
        };
        let mut f = feature(&label, &kind, first.start, last.end, if complement { Strand::Reverse } else { Strand::Forward }, Vec::new());
        if segments.len() > 1 {
            f.segments = segments;
        }
        features.push(f);
    }
    Some(construct(&name, &bases, circular, features))
}

/// A JSON value, read by [`parse_json`]. Bios core has no dependencies, and a sequence JSON needs
/// no more than this.
#[derive(Clone, Debug, PartialEq)]
pub enum Json {
    Null,
    Bool(bool),
    Number(f64),
    Text(String),
    List(Vec<Json>),
    Object(Vec<(String, Json)>),
}

impl Json {
    pub fn get(&self, key: &str) -> Option<&Json> {
        match self {
            Json::Object(pairs) => pairs.iter().find(|(k, _)| k == key).map(|(_, v)| v),
            _ => None,
        }
    }

    pub fn as_str(&self) -> Option<&str> {
        if let Json::Text(t) = self { Some(t) } else { None }
    }

    pub fn as_bool(&self) -> Option<bool> {
        if let Json::Bool(b) = self { Some(*b) } else { None }
    }

    /// A whole number, in range.
    pub fn as_i64(&self) -> Option<i64> {
        match self {
            Json::Number(n) if n.fract() == 0.0 && n.abs() < 9.0e15 => Some(*n as i64),
            _ => None,
        }
    }

    fn shown(&self) -> String {
        match self {
            Json::Null => "null".into(),
            Json::Bool(b) => b.to_string(),
            Json::Number(n) => n.to_string(),
            Json::Text(t) => t.clone(),
            Json::List(_) | Json::Object(_) => String::new(),
        }
    }
}

/// How deep lists and objects may nest. A sequence JSON is three levels deep.
const MAX_DEPTH: usize = 64;

struct Reader<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl Reader<'_> {
    fn space(&mut self) {
        while self.bytes.get(self.at).is_some_and(u8::is_ascii_whitespace) {
            self.at += 1;
        }
    }

    fn eat(&mut self, word: &str) -> Option<()> {
        let end = self.at + word.len();
        if self.bytes.get(self.at..end)? == word.as_bytes() {
            self.at = end;
            Some(())
        } else {
            None
        }
    }

    fn string(&mut self) -> Option<String> {
        self.eat("\"")?;
        let mut out = String::new();
        loop {
            let c = *self.bytes.get(self.at)?;
            self.at += 1;
            match c {
                b'"' => return Some(out),
                b'\\' => {
                    let e = *self.bytes.get(self.at)?;
                    self.at += 1;
                    match e {
                        b'n' => out.push('\n'),
                        b't' => out.push('\t'),
                        b'r' => out.push('\r'),
                        b'b' | b'f' => {}
                        b'u' => {
                            let hex = std::str::from_utf8(self.bytes.get(self.at..self.at + 4)?).ok()?;
                            self.at += 4;
                            out.push(char::from_u32(u32::from_str_radix(hex, 16).ok()?).unwrap_or('\u{fffd}'));
                        }
                        other => out.push(char::from(other)),
                    }
                }
                _ => {
                    // A character of several bytes is copied whole.
                    let start = self.at - 1;
                    let mut end = self.at;
                    while self.bytes.get(end).is_some_and(|b| (b & 0xc0) == 0x80) {
                        end += 1;
                    }
                    out.push_str(std::str::from_utf8(self.bytes.get(start..end)?).ok()?);
                    self.at = end;
                }
            }
        }
    }

    fn value(&mut self, depth: usize) -> Option<Json> {
        if depth > MAX_DEPTH {
            return None;
        }
        self.space();
        match *self.bytes.get(self.at)? {
            b'{' => {
                self.at += 1;
                let mut pairs = Vec::new();
                self.space();
                if self.eat("}").is_some() {
                    return Some(Json::Object(pairs));
                }
                loop {
                    self.space();
                    let key = self.string()?;
                    self.space();
                    self.eat(":")?;
                    pairs.push((key, self.value(depth + 1)?));
                    self.space();
                    if self.eat(",").is_none() {
                        self.eat("}")?;
                        return Some(Json::Object(pairs));
                    }
                }
            }
            b'[' => {
                self.at += 1;
                let mut items = Vec::new();
                self.space();
                if self.eat("]").is_some() {
                    return Some(Json::List(items));
                }
                loop {
                    items.push(self.value(depth + 1)?);
                    self.space();
                    if self.eat(",").is_none() {
                        self.eat("]")?;
                        return Some(Json::List(items));
                    }
                }
            }
            b'"' => self.string().map(Json::Text),
            b't' => self.eat("true").map(|()| Json::Bool(true)),
            b'f' => self.eat("false").map(|()| Json::Bool(false)),
            b'n' => self.eat("null").map(|()| Json::Null),
            _ => {
                let start = self.at;
                while self.bytes.get(self.at).is_some_and(|b| b.is_ascii_digit() || b"+-.eE".contains(b)) {
                    self.at += 1;
                }
                std::str::from_utf8(self.bytes.get(start..self.at)?).ok()?.parse::<f64>().ok().filter(|n| n.is_finite()).map(Json::Number)
            }
        }
    }
}

/// Reads JSON text, or `None` when it is not JSON.
pub fn parse_json(text: &str) -> Option<Json> {
    let mut reader = Reader { bytes: text.as_bytes(), at: 0 };
    let value = reader.value(0)?;
    reader.space();
    (reader.at == reader.bytes.len()).then_some(value)
}

/// Reads a Teselagen sequence JSON (`{ name, sequence, circular, features }`, features as a list
/// or as an object of them, each with `start` and `end` 0-based and `strand` 1 or -1 or
/// `forward`), or a list of them, of which the first is opened. Gives the number of sequences too.
pub fn read_teselagen(text: &str, fallback_name: &str) -> Option<(Construct, usize)> {
    let value = parse_json(text)?;
    let (record, count) = match &value {
        Json::List(items) => (items.first()?, items.len()),
        other => (other, 1),
    };
    // Some tools wrap the sequence as `parsedSequence`.
    let record = record.get("parsedSequence").unwrap_or(record);
    let bases = record.get("sequence")?.as_str()?;
    let name = record.get("name").and_then(Json::as_str).filter(|n| !n.is_empty()).unwrap_or(fallback_name);
    let circular = record.get("circular").and_then(Json::as_bool).unwrap_or(false);
    let listed: Vec<&Json> = match record.get("features") {
        Some(Json::List(items)) => items.iter().collect(),
        Some(Json::Object(pairs)) => pairs.iter().map(|(_, v)| v).collect(),
        _ => Vec::new(),
    };
    let mut features = Vec::new();
    for item in listed.into_iter().take(MAX_FEATURES) {
        let at = |key: &str| item.get(key).and_then(Json::as_i64).and_then(|v| usize::try_from(v).ok());
        let (Some(start), Some(end)) = (at("start"), at("end")) else {
            continue;
        };
        let reverse = item.get("strand").and_then(Json::as_i64) == Some(-1) || item.get("forward").and_then(Json::as_bool) == Some(false);
        let text_of = |key: &str| item.get(key).and_then(Json::as_str).unwrap_or_default();
        let mut notes = Vec::new();
        if let Some(Json::Object(pairs)) = item.get("notes") {
            for (key, value) in pairs {
                let values: Vec<String> = match value {
                    Json::List(vs) => vs.iter().map(Json::shown).collect(),
                    other => vec![other.shown()],
                };
                notes.push((key.clone(), values));
            }
        }
        features.push(feature(text_of("name"), text_of("type"), start, end, if reverse { Strand::Reverse } else { Strand::Forward }, notes));
    }
    Some((construct(name, bases, circular, features), count))
}
