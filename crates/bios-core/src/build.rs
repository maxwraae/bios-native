//! What Build is made of, apart from the Gibson maths: which enzymes can open a backbone, what a
//! pile of pasted text turns into, and the order across a folder.
//!
//! Ported from the web app's `build/openings.ts`, `build/intake.ts` and `build/order.ts`, with
//! their own tests and their numbers.

use std::collections::HashMap;

use crate::enzymes::{COMMON_ENZYMES, find_cut_sites, get_enzyme};

// ── WHICH ENZYMES CAN OPEN THIS BACKBONE ────────────────────────────────────────────────────
//
// A dropdown of every enzyme in the table is not a choice, it is a list. The fact that decides is
// how many times each one cuts: an enzyme that cuts ONCE gives you an opening, one that cuts five
// times gives you a mess. So the count is the sort key and it is on the row. Multi-cutters stay in
// the list — "PstI cuts 5×" is the answer to a question about to be asked — and enzymes that do
// not cut at all are dropped rather than dimmed, because that is a fact about this backbone.

/// One place an enzyme cuts. Both coordinates, because they are different questions: `at` is where
/// the strand is severed, `site_start` where the bases it READS begin. Build is the one screen
/// where where the site is decides where the backbone opens.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EnzymeCut {
    pub at: usize,
    pub site_start: usize,
}

/// An enzyme that cuts this backbone, with every place it does.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EnzymeOption {
    pub enzyme: String,
    /// The recognition site, e.g. `GAATTC`, shown beside the name.
    pub site: String,
    /// How many times it cuts the whole molecule. Always at least 1.
    pub count: usize,
    /// Every place it cuts, ascending by cut position.
    pub cuts: Vec<EnzymeCut>,
}

/// A unique cutter is an opening. Anything else has to be chosen deliberately.
pub fn is_opening(option: &EnzymeOption) -> bool {
    option.count == 1
}

/// Every enzyme in `names` that cuts `sequence`, unique cutters first then by name.
///
/// The order never depends on what the View menu happens to be showing: the picker's contents must
/// not change because someone changed a display preference.
pub fn enzyme_options(sequence: &str, circular: bool, names: &[&str]) -> Vec<EnzymeOption> {
    if sequence.is_empty() {
        return Vec::new();
    }
    let mut by_enzyme: HashMap<&'static str, Vec<EnzymeCut>> = HashMap::new();
    // One pass over the table. `find_cut_sites` already folds isoschizomers onto one canonical
    // name, so an enzyme appears once however many aliases matched.
    for site in find_cut_sites(sequence, names, circular) {
        by_enzyme.entry(site.enzyme).or_default().push(EnzymeCut { at: site.top_cut, site_start: site.recognition_start });
    }
    let mut out: Vec<EnzymeOption> = by_enzyme
        .into_iter()
        .map(|(enzyme, mut cuts)| {
            cuts.sort_by_key(|c| c.at);
            EnzymeOption {
                site: get_enzyme(enzyme).map(|e| e.site.to_string()).unwrap_or_default(),
                count: cuts.len(),
                enzyme: enzyme.to_string(),
                cuts,
            }
        })
        .collect();
    out.sort_by(|a, b| a.count.cmp(&b.count).then_with(|| a.enzyme.cmp(&b.enzyme)));
    out
}

/// The same over the enzymes a person is usually offered.
pub fn common_enzyme_options(sequence: &str, circular: bool) -> Vec<EnzymeOption> {
    enzyme_options(sequence, circular, &COMMON_ENZYMES)
}

/// The two arcs a pair of cuts defines, in bases.
///
/// This is the whole reason the swap control exists: EcoRI at 4500 with BamHI at 200 on a 5 kb
/// plasmid can mean "take out 4300 bp" or "take out 700 bp", and nothing in the pair says which.
/// On most plasmids the big arc is the origin and the resistance marker.
pub fn arcs_between(a: usize, b: usize, length: usize) -> (usize, usize) {
    let between = a.abs_diff(b);
    (between, length.saturating_sub(between))
}

// ── WHAT A PASTED PILE OF INSERTS TURNS INTO ────────────────────────────────────────────────
//
// Nobody types an insert's name in Build. A name comes with the thing when it has one — a FASTA
// header, the first column of a sheet, a file's own name — and otherwise the row is numbered.

/// One row of intake: what it is called, and its bases.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IntakeRow {
    pub name: String,
    pub seq: String,
}

fn letters(text: &str) -> String {
    text.chars().filter(char::is_ascii_alphabetic).collect()
}

/// Does this cell hold bases rather than a label? Short strings are labels; `TATA` is a name.
fn looks_like_bases(cell: &str) -> bool {
    let s = letters(cell);
    s.len() >= 12 && s.bytes().all(|b| b"ACGTUNRYKMSWBDHVacgtunrykmswbdhv".contains(&b))
}

/// Rows out of pasted text.
///
/// Three shapes, decided from the text itself rather than asked for:
/// - FASTA — every record is a row, the header's first word is its name.
/// - a sheet (Excel pastes tab-separated, a CSV comma-separated) — one row per line; the column
///   that holds bases is the sequence, the first column that does not is the name. A header line
///   holds no bases and is skipped for that reason, not by matching words.
/// - one sequence per line — a row each, unnamed.
///
/// Whitespace inside a sequence is tolerated everywhere: bases copied out of a GenBank ORIGIN
/// block arrive with spaces and numbers in them.
pub fn parse_insert_text(text: &str) -> Vec<IntakeRow> {
    let text = text.replace("\r\n", "\n").replace('\r', "\n");
    let text = text.trim();
    if text.is_empty() {
        return Vec::new();
    }
    if text.starts_with('>') {
        return parse_fasta(text);
    }
    let mut rows = Vec::new();
    for line in text.split('\n') {
        if line.trim().is_empty() {
            continue;
        }
        let cells: Vec<&str> = line.split(['\t', ',', ';']).map(str::trim).collect();
        if cells.len() == 1 {
            let seq = letters(cells[0]);
            if looks_like_bases(&seq) {
                rows.push(IntakeRow { name: String::new(), seq: seq.to_uppercase() });
            }
            continue;
        }
        let Some(seq_cell) = cells.iter().find(|c| looks_like_bases(c)) else {
            continue;
        };
        let name = cells.iter().find(|c| !c.is_empty() && *c != seq_cell && !looks_like_bases(c)).copied().unwrap_or_default();
        rows.push(IntakeRow { name: name.to_string(), seq: letters(seq_cell).to_uppercase() });
    }
    rows
}

fn parse_fasta(text: &str) -> Vec<IntakeRow> {
    let mut rows: Vec<IntakeRow> = Vec::new();
    let mut name: Option<String> = None;
    let mut seq = String::new();
    let keep = |name: &mut Option<String>, seq: &mut String, rows: &mut Vec<IntakeRow>| {
        if let Some(header) = name.take() {
            let bases = letters(seq).to_uppercase();
            if !bases.is_empty() {
                rows.push(IntakeRow { name: header, seq: bases });
            }
        }
        seq.clear();
    };
    for line in text.split('\n') {
        match line.strip_prefix('>') {
            Some(header) => {
                keep(&mut name, &mut seq, &mut rows);
                name = Some(header.split_whitespace().next().unwrap_or_default().to_string());
            }
            None => seq.push_str(line),
        }
    }
    keep(&mut name, &mut seq, &mut rows);
    rows
}

/// Sheet-shaped files are read as text, not through the sequence-format sniffer.
pub fn is_sheet_file(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    [".csv", ".tsv", ".txt"].iter().any(|ext| lower.ends_with(ext))
}

// ── THE ORDER ACROSS A BUILD FOLDER ─────────────────────────────────────────────────────────
//
// A folder holds one design per plasmid and each keeps its own pieces. Read one at a time that is
// a list per row; read across the folder it is a SET. Two designs that share a piece need it
// ordered once, and the recipe for each plasmid is then a list of numbers into the same table.
//
// Identity is the SEQUENCE. Names are what the console happened to call a piece; the bases are
// what a supplier makes, so they are what decides "the same piece".

/// One piece to order, as a row of the table.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OrderPiece {
    /// 1-based: the row number, and the number a design's recipe cites.
    pub n: usize,
    pub name: String,
    pub length: usize,
    /// The bases to order, uppercase, exactly as the design produced them.
    pub order: String,
}

/// The table, and which rows of it each design is made of.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FolderOrder {
    pub pieces: Vec<OrderPiece>,
    /// Design name, then the numbers of its pieces in assembly order.
    pub recipes: Vec<(String, Vec<usize>)>,
}

impl FolderOrder {
    pub fn recipe(&self, design: &str) -> Option<&[usize]> {
        self.recipes.iter().find(|(name, _)| name == design).map(|(_, nums)| nums.as_slice())
    }
}

/// One design's pieces, as the order reads them.
pub struct DesignPieces<'a> {
    pub design: &'a str,
    /// Each piece: its name and the bases to order. Empty for a design with nothing kept.
    pub pieces: &'a [(String, String)],
}

/// Number every distinct piece across the folder, oldest design first.
///
/// Oldest first so a number never changes under you: a design added later takes new numbers at the
/// end and the ones already on somebody's order sheet keep meaning what they meant.
pub fn order_across(designs: &[DesignPieces]) -> FolderOrder {
    let mut pieces: Vec<OrderPiece> = Vec::new();
    let mut by_seq: HashMap<String, usize> = HashMap::new();
    let mut first_in: Vec<String> = Vec::new();
    let mut recipes: Vec<(String, Vec<usize>)> = Vec::new();
    for d in designs {
        let mut numbers = Vec::new();
        for (name, order) in d.pieces {
            let seq = letters(order).to_uppercase();
            if seq.is_empty() {
                continue;
            }
            let at = match by_seq.get(&seq) {
                Some(at) => *at,
                None => {
                    pieces.push(OrderPiece { n: pieces.len() + 1, name: name.clone(), length: seq.len(), order: seq.clone() });
                    first_in.push(d.design.to_string());
                    by_seq.insert(seq, pieces.len() - 1);
                    pieces.len() - 1
                }
            };
            numbers.push(pieces[at].n);
        }
        recipes.push((d.design.to_string(), numbers));
    }
    // A NAME ON AN ORDER FORM HAS TO NAME ONE TUBE. Nobody types an insert's name, so the console
    // numbers them per design: two designs can each hold an `Insert_1`, which across a folder is
    // two different molecules under one name, and a supplier's form would take both. A name used
    // once is left alone; a name used twice says which design it came from.
    let mut used: HashMap<String, usize> = HashMap::new();
    for p in &pieces {
        *used.entry(p.name.clone()).or_default() += 1;
    }
    for (i, p) in pieces.iter_mut().enumerate() {
        if used.get(&p.name).is_some_and(|n| *n > 1) {
            p.name = format!("{} · {}", p.name, first_in.get(i).cloned().unwrap_or_default()).trim().to_string();
        }
    }
    FolderOrder { pieces, recipes }
}

/// Four columns, tab-separated, a header row: what pastes into a spreadsheet or an order form as is.
pub fn order_tsv(pieces: &[OrderPiece]) -> String {
    let mut out = String::from("#\tname\tlength\tsequence");
    for p in pieces {
        out.push_str(&format!("\n{}\t{}\t{}\t{}", p.n, p.name, p.length, p.order));
    }
    out
}

fn xml(text: &str) -> String {
    text.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

/// The same four columns as a real `.xlsx`.
///
/// A workbook is a zip of XML, and this writes the five files a spreadsheet needs and nothing
/// else: no styles, no shared strings (inline strings are valid and mean one file fewer), no
/// theme. It is what an order form wants — one sheet, one row per piece.
pub fn order_xlsx(pieces: &[OrderPiece], sheet: &str) -> Vec<u8> {
    let cell_text = |col: &str, r: usize, v: &str| format!("<c r=\"{col}{r}\" t=\"inlineStr\"><is><t>{}</t></is></c>", xml(v));
    let cell_num = |col: &str, r: usize, v: usize| format!("<c r=\"{col}{r}\"><v>{v}</v></c>");
    let mut rows = format!(
        "<row r=\"1\">{}{}{}{}</row>",
        cell_text("A", 1, "#"),
        cell_text("B", 1, "name"),
        cell_text("C", 1, "length"),
        cell_text("D", 1, "sequence")
    );
    for (i, p) in pieces.iter().enumerate() {
        let r = i + 2;
        rows.push_str(&format!(
            "<row r=\"{r}\">{}{}{}{}</row>",
            cell_num("A", r, p.n),
            cell_text("B", r, &p.name),
            cell_num("C", r, p.length),
            cell_text("D", r, &p.order)
        ));
    }
    let sheet_xml = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\
<worksheet xmlns=\"http://schemas.openxmlformats.org/spreadsheetml/2006/main\">\
<cols><col min=\"1\" max=\"1\" width=\"5\" customWidth=\"1\"/><col min=\"2\" max=\"2\" width=\"24\" customWidth=\"1\"/>\
<col min=\"3\" max=\"3\" width=\"9\" customWidth=\"1\"/><col min=\"4\" max=\"4\" width=\"80\" customWidth=\"1\"/></cols>\
<sheetData>{rows}</sheetData></worksheet>"
    );
    let content_types = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\
<Types xmlns=\"http://schemas.openxmlformats.org/package/2006/content-types\">\
<Default Extension=\"rels\" ContentType=\"application/vnd.openxmlformats-package.relationships+xml\"/>\
<Default Extension=\"xml\" ContentType=\"application/xml\"/>\
<Override PartName=\"/xl/workbook.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml\"/>\
<Override PartName=\"/xl/worksheets/sheet1.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml\"/>\
</Types>";
    let rels = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\
<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\">\
<Relationship Id=\"rId1\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument\" Target=\"xl/workbook.xml\"/>\
</Relationships>";
    let workbook = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\
<workbook xmlns=\"http://schemas.openxmlformats.org/spreadsheetml/2006/main\" xmlns:r=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships\">\
<sheets><sheet name=\"{}\" sheetId=\"1\" r:id=\"rId1\"/></sheets></workbook>",
        xml(sheet)
    );
    let workbook_rels = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\
<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\">\
<Relationship Id=\"rId1\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet\" Target=\"worksheets/sheet1.xml\"/>\
</Relationships>";
    zip(&[
        ("[Content_Types].xml", content_types),
        ("_rels/.rels", rels),
        ("xl/workbook.xml", &workbook),
        ("xl/_rels/workbook.xml.rels", workbook_rels),
        ("xl/worksheets/sheet1.xml", &sheet_xml),
    ])
}

// ── A ZIP OF FIVE SMALL FILES ───────────────────────────────────────────────────────────────
//
// Written here rather than taken from a crate, because a workbook of four columns needs exactly
// this much of the format: entries stored rather than deflated (valid, and a spreadsheet opens
// them), no directories, no zip64, no extra fields. A compressor would be a dependency, a licence
// to check and several thousand lines, to save a few kilobytes on a file nobody keeps.

fn crc32(bytes: &[u8]) -> u32 {
    let mut crc = 0xffff_ffff_u32;
    for byte in bytes {
        crc ^= u32::from(*byte);
        for _ in 0..8 {
            // The reflected CRC-32 polynomial (0xedb88320), as PKZIP uses it.
            crc = if crc & 1 == 1 { (crc >> 1) ^ 0xedb8_8320 } else { crc >> 1 };
        }
    }
    !crc
}

fn zip(files: &[(&str, &str)]) -> Vec<u8> {
    let mut out: Vec<u8> = Vec::new();
    let mut directory: Vec<u8> = Vec::new();
    let mut count: u16 = 0;
    for (name, text) in files {
        let bytes = text.as_bytes();
        let offset = out.len() as u32;
        let crc = crc32(bytes);
        let size = bytes.len() as u32;
        out.extend_from_slice(&[0x50, 0x4b, 0x03, 0x04]);
        // Version 2.0, no flags, stored, a fixed DOS timestamp (a workbook is not dated by its zip).
        out.extend_from_slice(&[20, 0, 0, 0, 0, 0, 0, 0, 0x21, 0]);
        out.extend_from_slice(&crc.to_le_bytes());
        out.extend_from_slice(&size.to_le_bytes());
        out.extend_from_slice(&size.to_le_bytes());
        out.extend_from_slice(&(name.len() as u16).to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
        out.extend_from_slice(name.as_bytes());
        out.extend_from_slice(bytes);

        directory.extend_from_slice(&[0x50, 0x4b, 0x01, 0x02]);
        directory.extend_from_slice(&[20, 0, 20, 0, 0, 0, 0, 0, 0, 0, 0x21, 0]);
        directory.extend_from_slice(&crc.to_le_bytes());
        directory.extend_from_slice(&size.to_le_bytes());
        directory.extend_from_slice(&size.to_le_bytes());
        directory.extend_from_slice(&(name.len() as u16).to_le_bytes());
        directory.extend_from_slice(&[0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
        directory.extend_from_slice(&offset.to_le_bytes());
        directory.extend_from_slice(name.as_bytes());
        count += 1;
    }
    let start = out.len() as u32;
    let size = directory.len() as u32;
    out.extend_from_slice(&directory);
    out.extend_from_slice(&[0x50, 0x4b, 0x05, 0x06, 0, 0, 0, 0]);
    out.extend_from_slice(&count.to_le_bytes());
    out.extend_from_slice(&count.to_le_bytes());
    out.extend_from_slice(&size.to_le_bytes());
    out.extend_from_slice(&start.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out
}
