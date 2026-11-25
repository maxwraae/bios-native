//! A picture of a construct, written to a file an agent can open and look at.
//!
//! The drawing itself is `bios-ui`'s, handed to the engine at start ([`crate::picture`]). This
//! works out what to draw, where to put the file, and turns the SVG into a PNG with macOS's own
//! QuickLook, which needs no library and no licence check.

use std::path::{Path, PathBuf};
use std::process::Command;

use bios_core::Construct;
use serde_json::{Value, json};

use super::PLASMID;
use crate::picture::{Ask, View};
use crate::{Args, CommandSpec, Effect, Failure, Kind, Param, Run, Session, fail, refs};

pub const COMMANDS: &[CommandSpec] = &[CommandSpec {
    name: "draw_plasmid",
    description: "Draw a picture of a plasmid to a file, so you can LOOK AT IT. The answer gives the file's full path; open that file (a PNG is an image you can read directly) and you see the construct the way Bios draws it on screen, instead of having to picture it from JSON. Show it to the person when they ask what something looks like, or to check your own work after an edit. `view` picks one of the three ways Bios shows a construct: `circular` is the ring map, `linear` the same map as a straight line, `sequence` the rows of bases with their features, primers and cut sites. Features, their names and the single-cutter restriction sites are drawn on all three. Without `path` the picture goes to a scratch file outside the person's project and the answer says where; pass `path` only when the person asked for a file they will keep. It changes nothing in the project.",
    params: &[
        PLASMID,
        Param {
            name: "view",
            kind: Kind::String,
            required: true,
            description: "circular (the ring map), linear (the map as a straight line), or sequence (the rows of bases with their features).",
        },
        Param {
            name: "start",
            kind: Kind::Integer,
            required: false,
            description: "First base to draw, 1-based (default 1). Give both start and end to draw one stretch.",
        },
        Param {
            name: "end",
            kind: Kind::Integer,
            required: false,
            description: "Last base to draw, 1-based and inclusive (default the last base). Before `start` it runs through the origin of a circular plasmid, which only the circular view can draw.",
        },
        Param { name: "format", kind: Kind::String, required: false, description: "png (default), which you can open and look at, or svg." },
        Param {
            name: "path",
            kind: Kind::String,
            required: false,
            description: "Where to write the picture. Leave it out and it goes to a scratch file outside the project, which is what you want unless the person asked to keep the picture: nothing lands in their project folder unless they asked for it. A folder here puts the picture inside it under a name of its own; a missing or wrong extension is corrected to match `format`.",
        },
        Param { name: "width", kind: Kind::Integer, required: false, description: "Width in pixels, 200 to 4000 (default 1200)." },
        Param {
            name: "height",
            kind: Kind::Integer,
            required: false,
            description: "Height in pixels, 200 to 4000 (default 900). A map fills it; the sequence view uses only as much of it as its rows need, and says which bases those were. Make it taller to read more of the sequence at once.",
        },
    ],
    effect: Effect::Read,
    run: Run::Call(draw_plasmid),
}];

const DEFAULT_WIDTH: i64 = 1200;
const DEFAULT_HEIGHT: i64 = 900;
const MIN_SIDE: i64 = 200;
const MAX_SIDE: i64 = 4000;

fn draw_plasmid(session: &Session, args: &Args) -> Result<Value, Failure> {
    let painter = session.painter().ok_or_else(|| Failure("this copy of Bios was built without its drawing, so it cannot make a picture".into()))?;
    let loaded = session.load(args)?;
    let c = &loaded.construct;
    let label = refs::label(&loaded.path);
    if c.is_empty() {
        return fail(format!("{label} has no bases, so there is nothing to draw"));
    }
    let view = view_of(args)?;
    let (start, end) = range_of(args, c, view)?;
    let (width, height) = size_of(args)?;
    let png = match args.opt_str("format")?.as_deref() {
        None | Some("png") => true,
        Some("svg") => false,
        Some(other) => return fail(format!("format must be png or svg, not {other}")),
    };

    let ask = |square: bool| Ask { construct: c, view, start, end, width: f64::from(width), height: f64::from(height), square };
    let drawn = painter.draw(&ask(png)).map_err(Failure)?;
    let (made_w, made_h) = pixels(drawn.size, width, height);
    let file = where_to_write(args, c, view, png)?;
    let (file, format, trouble) = match png {
        false => {
            write(&file, drawn.svg.as_bytes())?;
            (file, "svg", None)
        }
        true => match to_png(&drawn.svg, &file, made_w, made_h) {
            Ok(()) => (file, "png", None),
            // A picture is better than no picture: the SVG is the same drawing, and every
            // editor and browser opens one.
            Err(why) => {
                let instead = file.with_extension("svg");
                write(&instead, painter.draw(&ask(false)).map_err(Failure)?.svg.as_bytes())?;
                (instead, "svg", Some(why))
            }
        },
    };

    let (from, to) = (drawn.drawn.0 + 1, drawn.drawn.1 + 1);
    let length = c.len() as i64;
    let whole = from == 1 && to == length;
    let straight = end >= start;
    let short = straight && to < end + 1;
    let wider = straight && !short && !whole && (from < start + 1 || to > end + 1);
    let path = file.display().to_string();
    let mut said = format!(
        "A {} picture of {} ({}) is at {path}. {}",
        view.name(),
        c.name,
        if whole { format!("all {length} bases") } else { format!("bases {from} to {to} of {length}") },
        if format == "png" {
            "Open that PNG and look at it; it is an image, and you can show it to the person."
        } else {
            "Open that SVG to look at it, or show it to the person."
        }
    );
    if short {
        said.push_str(&format!(
            " Only bases {from} to {to} fit in a {made_w} by {made_h} picture; ask for a shorter stretch, or a taller picture, to see the rest."
        ));
    }
    if wider {
        said.push_str(match view {
            View::Sequence => " A row is drawn whole, so the picture holds a little more than the stretch asked for.",
            _ => " The map does not zoom in closer than a hundredth of the molecule, so the picture holds more than the stretch asked for.",
        });
    }
    if let Some(why) = &trouble {
        said.push_str(&format!(" A PNG could not be made on this computer ({why}), so the picture is an SVG instead."));
    }
    Ok(json!({
        "path": path,
        "plasmid": label,
        "view": view.name(),
        "format": format,
        "start": from,
        "end": to,
        "whole_plasmid": whole,
        "width": made_w,
        "height": made_h,
        "png_unavailable": trouble,
        "said": said,
    }))
}

/// The picture's size in whole pixels, as the drawing came out. It is never bigger than was asked
/// for, and stays even so that padding it to a square and cropping it back lands on whole pixels.
fn pixels(size: (f64, f64), width: u32, height: u32) -> (u32, u32) {
    let side = |made: f64, asked: u32| {
        let whole = if made.is_finite() { made.round().max(2.0) as u32 } else { asked };
        let held = whole.min(asked).max(2);
        held - held % 2
    };
    (side(size.0, width), side(size.1, height))
}

fn view_of(args: &Args) -> Result<View, Failure> {
    let word = args.str("view")?;
    View::parse(&word).map_or_else(|| fail(format!("view must be circular, linear or sequence, not {word}")), Ok)
}

/// The stretch to draw, 0-based and inclusive, from the 1-based parameters.
fn range_of(args: &Args, c: &Construct, view: View) -> Result<(i64, i64), Failure> {
    let length = c.len() as i64;
    let start = args.opt_int("start")?.unwrap_or(1);
    let end = args.opt_int("end")?.unwrap_or(length);
    if start < 1 || end < 1 || start > length || end > length {
        return fail(format!("start and end must be bases of this plasmid (1..{length})"));
    }
    if start > end {
        if !c.circular {
            return fail("start after end only works on a circular plasmid");
        }
        if view != View::Circular {
            return fail(format!(
                "a stretch running through the origin ({start}..{end}) can only be drawn on the circular view; the {} view unrolls the molecule at the origin, so ask for it in two pieces",
                view.name()
            ));
        }
    }
    Ok((start - 1, end - 1))
}

/// The picture's size in pixels. Kept even, so that padding it out to a square and cropping it
/// back lands on whole pixels.
fn size_of(args: &Args) -> Result<(u32, u32), Failure> {
    let side = |name: &str, fallback: i64| -> Result<u32, Failure> {
        let asked = args.opt_int(name)?.unwrap_or(fallback);
        if !(MIN_SIDE..=MAX_SIDE).contains(&asked) {
            return fail(format!("{name} must be between {MIN_SIDE} and {MAX_SIDE} pixels"));
        }
        Ok(u32::try_from(asked - asked % 2).unwrap_or(0))
    };
    Ok((side("width", DEFAULT_WIDTH)?, side("height", DEFAULT_HEIGHT)?))
}

/// Where the picture goes. Without a path it goes to a scratch folder outside every project: a
/// picture nobody asked to keep never lands in the person's own folders.
fn where_to_write(args: &Args, c: &Construct, view: View, png: bool) -> Result<PathBuf, Failure> {
    let extension = if png { "png" } else { "svg" };
    let name = format!("{}-{}.{extension}", plain(&c.name), view.name());
    let Some(asked) = args.opt_str("path")?.map(PathBuf::from).filter(|p| !p.as_os_str().is_empty()) else {
        let scratch = std::env::temp_dir().join("bios-pictures").join(uuid::Uuid::new_v4().to_string());
        std::fs::create_dir_all(&scratch).map_err(|e| Failure(format!("{}: {e}", scratch.display())))?;
        return Ok(scratch.join(name));
    };
    if asked.is_dir() {
        return Ok(asked.join(name));
    }
    let right_extension = asked.extension().is_some_and(|e| e.eq_ignore_ascii_case(extension));
    // A path with the wrong extension gains the right one rather than losing it: `notes.txt` must
    // not be overwritten by a picture.
    let file = if right_extension { asked } else { add_extension(&asked, extension) };
    if let Some(dir) = file.parent().filter(|d| !d.as_os_str().is_empty()) {
        std::fs::create_dir_all(dir).map_err(|e| Failure(format!("{}: {e}", dir.display())))?;
    }
    Ok(file)
}

fn add_extension(path: &Path, extension: &str) -> PathBuf {
    let mut name = path.file_name().map(std::ffi::OsString::from).unwrap_or_default();
    name.push(".");
    name.push(extension);
    path.with_file_name(name)
}

/// A construct's name as a file name: nothing in it can make a path of its own.
fn plain(name: &str) -> String {
    let kept: String = name.chars().map(|ch| if ch.is_alphanumeric() || "-_. ".contains(ch) { ch } else { '_' }).collect();
    let trimmed = kept.trim().trim_matches('.').trim();
    if trimmed.is_empty() { "plasmid".to_string() } else { trimmed.to_string() }
}

fn write(path: &Path, bytes: &[u8]) -> Result<(), Failure> {
    std::fs::write(path, bytes).map_err(|e| Failure(format!("{}: {e}", path.display())))
}

/// The SVG as a PNG, through macOS's own QuickLook.
///
/// `qlmanage` fills a square and crops whatever overhangs it, so the drawing arrives padded out to
/// a square ([`Ask::square`]) and `sips` crops the padding back off. Both are part of macOS; a
/// picture costs Bios no dependency and no licence.
fn to_png(square_svg: &str, png: &Path, width: u32, height: u32) -> Result<(), String> {
    if !cfg!(target_os = "macos") {
        return Err("it needs macOS's QuickLook".into());
    }
    let dir = std::env::temp_dir().join("bios-pictures").join(uuid::Uuid::new_v4().to_string());
    std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let scratch = Scratch(dir);
    let svg = scratch.0.join("picture.svg");
    std::fs::write(&svg, square_svg).map_err(|e| format!("{}: {e}", svg.display()))?;
    let side = width.max(height);
    let made = Command::new("/usr/bin/qlmanage")
        .args(["-t", "-s", &side.to_string(), "-o"])
        .arg(&scratch.0)
        .arg(&svg)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map_err(|e| format!("qlmanage: {e}"))?;
    let thumbnail = scratch.0.join("picture.svg.png");
    if !made.success() || !thumbnail.is_file() {
        return Err("QuickLook made no picture from the SVG".into());
    }
    std::fs::copy(&thumbnail, png).map_err(|e| format!("{}: {e}", png.display()))?;
    if width == height {
        return Ok(());
    }
    // Centred, which is where the drawing sits in the square.
    let cropped = Command::new("/usr/bin/sips")
        .args(["-c", &height.to_string(), &width.to_string()])
        .arg(png)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status();
    if cropped.as_ref().is_ok_and(std::process::ExitStatus::success) {
        return Ok(());
    }
    // The square it was before the crop is not the picture that was asked for, so it does not
    // stay behind beside the SVG that is written instead.
    let _ = std::fs::remove_file(png);
    Err(match cropped {
        Err(e) => format!("sips: {e}"),
        Ok(_) => "the picture could not be cropped back to its own size".into(),
    })
}

/// A temporary folder that goes away with the call that made it.
struct Scratch(PathBuf);

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[cfg(test)]
mod tests {
    use super::{add_extension, plain};
    use std::path::{Path, PathBuf};

    #[test]
    fn a_name_that_could_make_a_path_of_its_own_cannot() {
        assert_eq!(plain("pUC19"), "pUC19");
        assert_eq!(plain("pBR322 v2"), "pBR322 v2");
        assert_eq!(plain("../../etc/passwd"), "_.._etc_passwd", "no separator survives, so it is one name in one folder");
        assert_eq!(plain("  ..  "), "plasmid");
        assert_eq!(plain(""), "plasmid");
    }

    #[test]
    fn a_path_keeps_its_own_name_and_gains_the_right_extension() {
        assert_eq!(add_extension(Path::new("/tmp/notes.txt"), "png"), PathBuf::from("/tmp/notes.txt.png"));
        assert_eq!(add_extension(Path::new("map"), "svg"), PathBuf::from("map.svg"));
    }
}
