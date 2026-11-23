//! Writes a small order workbook to the path given, so a real spreadsheet reader can open it.
fn main() {
    let pieces = [
        bios_core::build::OrderPiece { n: 1, name: "EGFP".into(), length: 6, order: "ATGGTG".into() },
        bios_core::build::OrderPiece { n: 2, name: "x<y & co".into(), length: 3, order: "CCC".into() },
    ];
    let bytes = bios_core::build::order_xlsx(&pieces, "To order");
    let path = std::env::args().nth(1).unwrap_or_else(|| "order.xlsx".into());
    if let Err(e) = std::fs::write(&path, bytes) {
        eprintln!("{path}: {e}");
    }
}
