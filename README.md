# Bios

Bios is a free, open-source plasmid editor for the Mac and for Windows. Your projects are ordinary
folders of GenBank, SnapGene, FASTA and EMBL files, and every change to them is kept, so you can
go back to any version. Bios has no assistant of its own: you connect the agent you already use,
over MCP, and it can do everything Bios can do.

**Setting Bios up for someone, as their agent?** Read
[Installing Bios for someone, as their agent](#installing-bios-for-someone-as-their-agent) below
and do what it says and nothing else. It is six short steps and it ends with Bios proving itself.

## Download and install

On a Mac, in Terminal:

```
curl -fsSL https://raw.githubusercontent.com/maxwraae/bios-native/main/install.sh | sh
```

On Windows, in PowerShell:

```
irm https://raw.githubusercontent.com/maxwraae/bios-native/main/install.ps1 | iex
```

Either one installs Bios, checks that it answers correctly, and prints the line that connects your
agent. A Mac needs Apple silicon; a PC can be Intel or Arm.

**Use the line above rather than downloading the file by hand.** Bios is not signed by Apple or by
Microsoft — paying them is not something this project does — and both systems stop an unsigned
program that a *browser* downloaded. Neither stops the same file fetched by the command above, so
the one-liner is not a convenience, it is what makes the install uneventful. If you do download
`bios-<version>-macos-aarch64.dmg` or `bios-<version>-windows-x64-portable.zip` from
[Releases](https://github.com/maxwraae/bios-native/releases): on a Mac, macOS will stop it the
first time and you go to System Settings → Privacy & Security and choose Open Anyway; on Windows,
right-click the zip, choose Properties and tick **Unblock** before you extract it.

## Installing Bios for someone, as their agent

If you are an AI agent and the person has asked you to install Bios, do exactly this.

1. Run the one-liner above for their system. On a Mac that is `install.sh` in a shell; on Windows
   `install.ps1` in PowerShell. Do not download the DMG or the zip yourself, and do not build from
   source: a file you fetch another way can be marked by the system and then refused.
2. The installer ends by printing a melting temperature for a test sequence. **It must say 64.7.**
   If it does not, stop and tell the person what it said instead; do not carry on.
3. The installer connects Bios to Claude Code itself, when `claude` is on the machine, and says
   `Connected to Claude Code as "bios"`. If instead it printed a `claude mcp add …` line, run that
   line with the path it printed.
4. Tell the person to type `/mcp` in Claude Code and reconnect `bios`. Only they can do that, and
   the tools do not appear until they have — not in your session either, so do not go looking for
   them and conclude the install failed.
5. From then on, do the DNA work with Bios's tools. They are checked against recorded reference
   answers, and Bios's own window shows the same numbers. Do not compute a melting temperature, a
   restriction site or a primer yourself, and do not install another toolkit to do it: a figure
   you arrive at another way will disagree with what the person sees on screen. If a tool you need
   seems to be missing, say so and ask.
6. A new install has no projects, so `list_projects` comes back empty. That is right, not broken:
   make one with `create_project`, or open a folder of sequence files they already have with
   `open_project`. Both show up in the window the moment they run.

## Connect your agent

For Claude Code, run this in a terminal:

```
# macOS
claude mcp add --scope user bios -- "/Applications/Bios.app/Contents/MacOS/bios-cli" mcp

# Windows
claude mcp add --scope user bios -- "%LOCALAPPDATA%\Programs\bios\bios-cli.exe" mcp
```

If Bios is installed somewhere else, Help → Connect an Agent copies the line with the right path.
Other MCP clients (Claude Desktop, Cursor and the rest) take the same command: `bios-cli mcp`,
beside the app.

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

Bios checks GitHub Releases for a new version. To check now, use Help → Check for Updates.

On a Mac it then replaces itself, wherever it is installed. On Windows it does not do that yet, so
it tells you — and tells your agent — to run the `install.ps1` line again, which puts the new
version where the old one already is and keeps your projects and settings untouched.

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
