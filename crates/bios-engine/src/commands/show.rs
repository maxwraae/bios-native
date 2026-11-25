//! Putting a project and a plasmid in front of the person, in the Bios window.
//!
//! The other way round from `what_is_open`: the agent leaves a request beside the window's own
//! record ([`bios_project::window::request_show`]), brings Bios up (starting it when it is not
//! running), and waits a moment for the window to take the request.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use bios_project::window::{self, Selection};
use serde_json::{Value, json};

use crate::{Args, CommandSpec, Effect, Failure, Kind, Param, Run, Session, fail, refs};

pub const COMMANDS: &[CommandSpec] = &[CommandSpec {
    name: "show_in_bios",
    description: "Show something to the person in the Bios window: opens the project there and, when `plasmid` is given, opens that plasmid in front, with bases `start`..`end` selected when they are given. Starts Bios when it is not running and brings it to the front. Use it when the person asks to see something (\"show me\", \"open it\"), or after making or changing a plasmid they will want to look at. It changes nothing in the project.",
    params: &[
        Param {
            name: "plasmid",
            kind: Kind::String,
            required: false,
            description: "The plasmid to open: its path as list_plasmids shows it, its name or its id. Leave it out to show the project alone.",
        },
        Param { name: "start", kind: Kind::Integer, required: false, description: "First base to select, 1-based. Needs `end` and `plasmid`." },
        Param {
            name: "end",
            kind: Kind::Integer,
            required: false,
            description: "Last base to select, 1-based and inclusive; before `start` runs through the origin of a circular plasmid.",
        },
    ],
    effect: Effect::Read,
    run: Run::Call(show_in_bios),
}];

/// How long to wait for a window that is running to take the request, and for one being started.
const RUNNING_WAIT: Duration = Duration::from_secs(4);
const STARTING_WAIT: Duration = Duration::from_secs(15);

fn show_in_bios(session: &Session, args: &Args) -> Result<Value, Failure> {
    let Some(file) = session.window_file() else {
        return fail("Bios has no settings folder on this computer, so it cannot reach the window");
    };
    let root = session.project().root().to_path_buf();
    let (mut plasmid, mut label, mut selection) = (None, None, None);
    if args.has("plasmid") {
        let loaded = session.load(args)?;
        let length = loaded.construct.len();
        selection = match (args.opt_int("start")?, args.opt_int("end")?) {
            (None, None) => None,
            (Some(start), Some(end)) => {
                let base = |n: i64| usize::try_from(n).ok().filter(|n| (1..=length).contains(n));
                match (base(start), base(end)) {
                    (Some(start), Some(end)) => Some(Selection { start, end }),
                    _ => return fail(format!("`start` and `end` must be bases of {} (1..{length})", loaded.construct.name)),
                }
            }
            _ => return fail("give both `start` and `end` to select bases, or neither"),
        };
        plasmid = Some(loaded.id.as_str().to_string());
        label = Some(refs::label(&loaded.path));
    } else if args.has("start") || args.has("end") {
        return fail("`start` and `end` select bases in a plasmid; say which with `plasmid`");
    }
    let id = window::request_show(&file, &root, plasmid.clone(), selection)?;
    let running = window::live(&file).is_some();
    let started = bring_up(running);
    let wait = match (running, started) {
        (true, _) => RUNNING_WAIT,
        (false, Started::Yes) => STARTING_WAIT,
        (false, Started::No) => Duration::ZERO,
    };
    let deadline = Instant::now() + wait;
    while window::show_waiting(&file, &id) && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(100));
    }
    let taken = !window::show_waiting(&file, &id);
    if taken {
        // The window takes a request one frame before it writes down what it then shows, and an
        // agent's next `what_is_open` should already see it.
        let deadline = Instant::now() + Duration::from_secs(2);
        while !in_front(&file, &root, plasmid.as_deref()) && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(50));
        }
    }
    let what =
        label.as_deref().map_or_else(|| format!("project {}", session.project().name()), |l| format!("{l} in project {}", session.project().name()));
    let said = if taken {
        format!("Bios now shows {what}.")
    } else if running || started == Started::Yes {
        format!(
            "Bios was asked to show {what} but has not shown it yet; it will when its window next wakes. Ask the person to bring Bios to the front."
        )
    } else {
        format!("Bios is not running and could not be started from here; {what} will be shown when the person opens Bios within a minute.")
    };
    Ok(json!({ "shown": taken, "project": root.display().to_string(), "plasmid": label, "selection": selection, "said": said }))
}

/// Whether the window says it shows `root`, and in it the plasmid `plasmid` when one is named.
fn in_front(file: &Path, root: &Path, plasmid: Option<&str>) -> bool {
    let Some(front) = window::live(file).map(|r| r.front) else {
        return false;
    };
    let same = |a: &Path| a == root || a.canonicalize().ok().is_some_and(|a| root.canonicalize().ok() == Some(a));
    front.project.is_some_and(|p| same(&p.path)) && plasmid.is_none_or(|id| front.plasmid.is_some_and(|p| p.id == id))
}

#[derive(PartialEq, Eq, Clone, Copy)]
enum Started {
    Yes,
    No,
}

/// Brings the Bios window up: activates the app it was installed as (starting it when it is not
/// running), or starts the `bios` program beside this one when there is no window.
fn bring_up(running: bool) -> Started {
    let Ok(exe) = std::env::current_exe() else {
        return Started::No;
    };
    if let Some(bundle) = app_bundle(&exe) {
        // `open` starts the app when it is not running and brings it to the front when it is.
        let opened = std::process::Command::new("/usr/bin/open")
            .arg("-a")
            .arg(&bundle)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .is_ok_and(|s| s.success());
        return if opened { Started::Yes } else { Started::No };
    }
    if running {
        // The window looks for requests on its own.
        return Started::Yes;
    }
    let window = exe.with_file_name(if cfg!(windows) { "bios.exe" } else { "bios" });
    if !window.is_file() || window == exe {
        return Started::No;
    }
    let spawned = std::process::Command::new(&window)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .is_ok();
    if spawned { Started::Yes } else { Started::No }
}

/// The `.app` this program runs from, on macOS: `Bios.app` for `Bios.app/Contents/MacOS/bios-cli`.
fn app_bundle(exe: &Path) -> Option<PathBuf> {
    if !cfg!(target_os = "macos") {
        return None;
    }
    let macos = exe.parent()?;
    let contents = macos.parent()?;
    let bundle = contents.parent()?;
    let is_app = macos.file_name()? == "MacOS" && contents.file_name()? == "Contents" && bundle.extension()? == "app";
    is_app.then(|| bundle.to_path_buf())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_app_is_found_only_from_inside_its_bundle() {
        let inside = Path::new("/Users/me/Applications/Bios.app/Contents/MacOS/bios-cli");
        let want = cfg!(target_os = "macos").then(|| PathBuf::from("/Users/me/Applications/Bios.app"));
        assert_eq!(app_bundle(inside), want);
        assert_eq!(app_bundle(Path::new("/repo/target/debug/bios-cli")), None);
        assert_eq!(app_bundle(Path::new("/bios-cli")), None);
    }
}
