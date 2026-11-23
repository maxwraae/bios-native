//! The whole installation, for real, against a release served from 127.0.0.1: download, checksum,
//! open the disk image, check the app inside and swap it in where the old copy is. It needs a
//! built app and its disk image, so it runs only when asked:
//!
//! ```sh
//! cargo xtask release --no-check
//! BIOS_UPDATE_TEST_APP=dist/Bios.app BIOS_UPDATE_TEST_DMG=dist/bios-0.1.0-macos-aarch64.dmg \
//!   cargo test -p bios-update --features local-mirror --test local_release -- --ignored
//! ```
#![cfg(all(feature = "local-mirror", target_os = "macos"))]

use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::Command;

use craft_update::{Asset, Http, Paths, Release};
use sha2::{Digest, Sha256};

/// Serves `bytes` for every request, on a port of its own.
fn serve(bytes: Vec<u8>) -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            let mut line = String::new();
            while reader.read_line(&mut line).is_ok_and(|n| n > 2) {
                line.clear();
            }
            let mut stream = stream;
            let _ = write!(stream, "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", bytes.len());
            let _ = stream.write_all(&bytes);
        }
    });
    port
}

fn version_of(bundle: &Path) -> String {
    let out = Command::new("/usr/bin/defaults").arg("read").arg(bundle.join("Contents/Info")).arg("CFBundleShortVersionString").output().unwrap();
    String::from_utf8(out.stdout).unwrap().trim().to_string()
}

/// An older copy of the app, installed in a folder of its own.
fn old_copy(app: &Path, dir: &Path) -> PathBuf {
    let bundle = dir.join("Applications/Bios.app");
    std::fs::create_dir_all(bundle.parent().unwrap()).unwrap();
    assert!(Command::new("/usr/bin/ditto").arg(app).arg(&bundle).status().unwrap().success());
    for key in ["CFBundleShortVersionString", "CFBundleVersion"] {
        assert!(
            Command::new("/usr/bin/plutil")
                .args(["-replace", key, "-string", "0.0.1"])
                .arg(bundle.join("Contents/Info.plist"))
                .status()
                .unwrap()
                .success()
        );
    }
    std::fs::write(bundle.join("Contents/old-copy"), "").unwrap();
    bundle
}

fn release(name: &str, version: &str, port: u16, sha: String, size: u64) -> (Release, Asset) {
    let asset = Asset { name: name.to_string(), size, url: format!("http://127.0.0.1:{port}/{name}"), sha256: Some(sha) };
    let release = Release {
        tag: format!("v{version}"),
        version: craft_update::parse_version(version),
        assets: vec![asset.clone()],
        repo: bios_update::REPO.to_string(),
        ..Release::default()
    };
    (release, asset)
}

#[test]
#[ignore = "needs a built app and disk image: see the file's notes"]
fn a_release_served_locally_replaces_the_copy_where_it_is() {
    let app = PathBuf::from(std::env::var("BIOS_UPDATE_TEST_APP").unwrap());
    let dmg = PathBuf::from(std::env::var("BIOS_UPDATE_TEST_DMG").unwrap());
    let version = version_of(&app);
    let bytes = std::fs::read(&dmg).unwrap();
    let size = bytes.len() as u64;
    let sha = format!("{:x}", Sha256::digest(&bytes));
    let name = dmg.file_name().unwrap().to_str().unwrap().to_string();
    let port = serve(bytes);
    let dir = tempfile::tempdir().unwrap();
    let http = Http::new("Bios-test/0").unwrap();
    let paths = Paths::under(&dir.path().join("updater"));

    // A download that is not the published file installs nothing.
    let bundle = old_copy(&app, dir.path());
    let (wrong, wrong_asset) = release(&name, &version, port, "0".repeat(64), size);
    assert!(bios_update::install_release(&wrong, &wrong_asset, &bundle, &http, &paths).is_err());
    assert_eq!(version_of(&bundle), "0.0.1");

    let (release, asset) = release(&name, &version, port, sha, size);
    let placed = bios_update::install_release(&release, &asset, &bundle, &http, &paths).unwrap();
    assert_eq!(placed, bundle, "the new copy goes where the old one was");
    assert_eq!(version_of(&bundle), version);
    assert!(!bundle.join("Contents/old-copy").exists(), "the old copy is gone");
    assert!(bundle.join("Contents/MacOS/bios-cli").is_file());
    let verified = Command::new("/usr/bin/codesign").args(["--verify", "--deep", "--strict"]).arg(&bundle).status().unwrap();
    assert!(verified.success(), "the installed app's ad-hoc signature is intact");
    // Nothing is left behind beside it.
    let beside: Vec<_> = std::fs::read_dir(bundle.parent().unwrap()).unwrap().flatten().map(|e| e.file_name()).collect();
    assert_eq!(beside, vec![std::ffi::OsString::from("Bios.app")]);
}
