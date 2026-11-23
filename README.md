# Bios

Bios is a free, open-source plasmid editor for the Mac. Your projects are ordinary folders of
GenBank, SnapGene, FASTA and EMBL files, and every change to them is kept, so you can go back to
any version. Bios has no assistant of its own: you connect the agent you already use, over MCP,
and it can do everything Bios can do.

## Download and install

```
curl -fsSL https://raw.githubusercontent.com/maxwraae/bios-native/main/install.sh | sh
```

It installs Bios in Applications and prints the line that connects your agent. Bios needs a Mac
with Apple silicon.

Or download `bios-<version>-macos-aarch64.dmg` from
[Releases](https://github.com/maxwraae/bios-native/releases) and drag Bios to Applications. Bios is
not notarised by Apple yet, so the first time a downloaded copy is opened macOS stops it: go to
System Settings → Privacy & Security and choose Open Anyway. The command above avoids this.

## Connect your agent

For Claude Code, run this in a terminal:

```
claude mcp add --scope user bios -- "/Applications/Bios.app/Contents/MacOS/bios-cli" mcp
```

If Bios is installed somewhere else, Help → Connect an Agent copies the line with the right path.
Other MCP clients (Claude Desktop, Cursor and the rest) take the same command: `bios-cli mcp`,
inside the app.

The agent sees the folders you have opened in Bios. Ask it, in plain words, to do something to a
plasmid, and leave the window open to watch: what it changes shows up as it happens and can be
undone.

## First steps

1. Open Bios. Make a **New project**, or **Open folder** on any folder of sequence files you
   already have. Opening a folder writes nothing into it until you change something.
2. Drop a plasmid file on the window to bring it in.
3. Edit on the map or in the sequence. What you type is saved a moment later, and every change is
   in the history.

Help → How Bios Works shows the welcome sheet again.

## Updates

Bios checks GitHub Releases for a new version and updates itself. To check now, use Help → Check
for Updates.

## Build from source

Needs Rust 1.95 or later.

```
cargo build

# make a project and put a plasmid in it
target/debug/bios-cli new ~/bios-projects/Lab
target/debug/bios-cli run ~/bios-projects/Lab import_plasmid '{"file": "/full/path/to/plasmid.gb"}'

# see every command
target/debug/bios-cli commands

# open the window on the project, or on any folder of sequence files
target/debug/bios ~/bios-projects/Lab
```

The folders you open are listed on the window's start page and under File > Open Recent. The list
is `recents.json` in Bios's settings folder (on a Mac, `~/Library/Application Support/bios/`;
`BIOS_CONFIG_DIR` moves it), and the command line and any agent read and add to the same list.

To build the Mac app for your own machine:

```
cargo xtask app
open dist/Bios.app
```

`bios-cli mcp` with no folder serves the folders opened in Bios; `bios-cli mcp <folder>` serves
just that one. The agent starts with `list_projects`, and every tool takes `project`, a name from
that list or any folder's absolute path.

## Contributing

Before you call a change done:

```
cargo xtask ci
```

It runs the formatter, clippy, every test, the asset check and the layer check. `AGENTS.md` holds
the design and the rules, for people and agents alike.

## Licence

MIT or Apache-2.0, at your option (`LICENSE-MIT`, `LICENSE-APACHE`). Third-party material is
listed in `NOTICE` and `ATTRIBUTION.md`.
