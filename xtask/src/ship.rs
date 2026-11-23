//! `cargo xtask ship <host>`: pass the checks, build `dist/Bios.app`, and install it on another Mac
//! over ssh, in that Mac's `~/Applications`, quitting the copy that is running there and opening the
//! new one. For working on Bios on one Mac and using it on another: the agent connected to Bios on
//! that Mac runs the `bios-cli` inside the app, so it picks up the new commands when it reconnects.

use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};

/// What runs on the other Mac, reading the bundle as a tar stream on its standard input. The new
/// copy is unpacked beside the old one and swapped in only once it is whole, so a broken transfer
/// leaves the old app in place.
fn remote_script(bundle_id: &str, name: &str) -> String {
    format!(
        "osascript -e 'tell application id \"{bundle_id}\" to quit' >/dev/null 2>&1; sleep 2; \
         mkdir -p \"$HOME/Applications\" && cd \"$HOME/Applications\" && rm -rf {name}.app.new && mkdir {name}.app.new && \
         tar -xf - -C {name}.app.new && test -d {name}.app.new/{name}.app && rm -rf {name}.app && mv {name}.app.new/{name}.app . && \
         rmdir {name}.app.new && codesign -v {name}.app && open {name}.app && echo \"installed in $HOME/Applications/{name}.app\""
    )
}

/// A host as ssh takes it: a name from `~/.ssh/config`, `user@host` or an address. Anything that
/// could be read as an option or split by a shell is refused.
fn valid_host(host: &str) -> bool {
    !host.is_empty() && !host.starts_with('-') && host.chars().all(|c| c.is_ascii_alphanumeric() || "@.-_:".contains(c))
}

pub fn run(root: &Path, host: Option<&str>, check: bool, bundle_id: &str, name: &str) -> Result<(), String> {
    let host = host.ok_or("cargo xtask ship <host>: name the Mac to install on, as ssh knows it")?;
    if !valid_host(host) {
        return Err(format!("{host:?} is not a host name ssh would take"));
    }
    if check {
        crate::ci(root)?;
    }
    crate::app::run(root)?;
    let dist = root.join("dist");
    let mut tar = Command::new("tar")
        .args(["-cf", "-", "-C"])
        .arg(&dist)
        .arg(format!("{name}.app"))
        .stdout(Stdio::piped())
        .spawn()
        .map_err(|e| format!("tar could not start: {e}"))?;
    let stream = tar.stdout.take().ok_or("tar gave no output")?;
    let status = Command::new("ssh")
        .args(["-o", "BatchMode=yes", "-o", "ConnectTimeout=10", "-o", "ServerAliveInterval=15", host, &remote_script(bundle_id, name)])
        .stdin(stream)
        .status()
        .map_err(|e| format!("ssh could not start: {e}"))?;
    let packed = tar.wait().map_err(|e| format!("tar: {e}"))?;
    // When ssh fails, tar dies writing into the closed pipe; the cause to name is ssh's.
    if !status.success() {
        return Err(format!("installing on {host} failed (ssh exited with {status}; is it awake and reachable?)"));
    }
    if !packed.success() {
        return Err("tar failed while packing the app".to_string());
    }
    let _ = std::io::stdout().flush();
    println!("ship: {name}.app is on {host}; an agent connected to Bios there gets the new commands when it reconnects");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hosts_that_could_be_options_or_shell_are_refused() {
        for ok in ["laptop", "me@192.0.2.10", "laptop.example.ts.net"] {
            assert!(valid_host(ok), "{ok}");
        }
        for bad in ["", "-oProxyCommand=x", "mac book", "host;rm", "a$(b)", "x'y"] {
            assert!(!valid_host(bad), "{bad}");
        }
    }

    #[test]
    fn the_remote_script_swaps_only_a_whole_copy() {
        let s = remote_script("com.example.app", "Bios");
        assert!(s.contains("test -d Bios.app.new/Bios.app && rm -rf Bios.app"));
        assert!(s.contains("tell application id \"com.example.app\" to quit"));
    }
}
