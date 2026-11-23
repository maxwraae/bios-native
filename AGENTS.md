<!-- craft-house-rules:start core source=dde09ea31e2e457f5b4653a6e76f7cdf55944bdf sha256=540e6cea75d12d0710c3dca89658328e81e508ecef102446ca5bb95b7c0b92bb -->
## House rules

These apply in every Craft repository. A later rule may be stricter, not weaker. The source is `standards/house-rules-core.md` in `storytold/craft-contrib`. Propose changes there, not in a copy.

1. **A person is accountable.** Whoever runs an agent answers for what it opens and for its review comments.
2. **Check, then claim.** Before writing code, read the issue comments, search pull requests for `#N` and the keywords, and check recent commits on `main`. Comment on the issue first. Release a claim you will not finish.
3. **Verify, don't guess.** Open a definition before using it. Confirm the cause in the code, even when someone names a location.
4. **Keep fixes narrow and tested.** Fix what the issue describes, where the bug is. Add a test that fails without the fix.
5. **Review the diff before opening a pull request.** Identifiers exist as used, the test fails without the fix, nothing unrelated changed, and the description is true.
6. **Look for a better pull request.** If another fixes the same thing, close yours when it is better or equal, and say why.
7. **Report only what ran.** Say what you tested and what you could not run. Without a local build or test, say so and rely on CI before marking ready.
8. **`main` changes through a pull request.** Do not push to it. Branch protection enforces this. There is no exception.
9. **Stop and ask only about** licensing outside the allowlist, publishing, releases, a new remote, secrets, spending money, destructive git, and anything else hard to undo. Otherwise keep working. `plan/` is a gitignored notebook, not rules. Without it, do not ask for it and do not stop.
10. **Keep secrets and personal details out** of the repository, logs and requests.
11. **CI stays shared.** Workflows call `storytold/craft-common-actions`. Do not change CI in a feature or fix pull request. If CI cannot cover a change, open an issue there.
12. **Guards say why.** A check that looks removable says what it prevents. Read that before changing it.
13. **Instructions must be readable by everyone.** No notes about one machine, and no links to files a reader cannot open. `craftrules` is retired. Do not link to it.
14. **Keep pull requests moving.** Concurrent pull requests are fine; unanswered actionable feedback is not. Respond in 7 days, a reminder at 14, closure eligible at 30, from when an actionable request waits on the author. Suggestions and unconfirmed bot findings do not start the clock. A fix, explanation or agreed plan resets it; unrelated activity does not. Waiting on a reviewer or maintainer pauses it and does not count against the author. A maintainer decides closure; it is never automatic. Reuse search results, do not poll, and stop at a rate limit.
15. **No agent attribution.** A pull request does not say an agent wrote it. A repository rule may not require that.
<!-- craft-house-rules:end -->
<!-- craft-house-rules:start rust source=dde09ea31e2e457f5b4653a6e76f7cdf55944bdf sha256=387e0ccb4b49e567f60951d7d6fff7305054c89f43ad812b9993dc7c38528013 -->
## Rust Craft repositories

These apply to native Craft apps and to shared Rust libraries. Select them by repository type. A repository that merely contains some Rust does not take this block. Rules later in a repository's file may be stricter, but not weaker. The source is `standards/house-rules-rust.md` in [storytold/craft-contrib](https://github.com/storytold/craft-contrib).

1. **Never crash.** Non-test code does not panic and has no `unsafe`, except where the repository file names an exception. Return errors, treat input as hostile, and bound recursion and allocations. New production crate roots copy the `deny` attribute from an existing crate: `clippy::unwrap_used`, `clippy::expect_used`, `clippy::panic`, `clippy::unimplemented`, `clippy::todo`, `clippy::unreachable`. Every crash fix comes with a test that panicked before the fix.
2. **Clean room.** Learn from observed behaviour and public specifications. Do not copy another product's code, assets, strings or artwork. Check a dependency's licence before adding it.
3. **Assets and fonts.** Every asset is original or suitably licensed, and is recorded in `ATTRIBUTION.md` in the same change. Font files live in [storytold/craft-fonts](https://github.com/storytold/craft-fonts), not in the app repository.
4. **Rust only for native Craft apps.** No handwritten JavaScript or TypeScript, and no Tauri, Electron or webview shell, in those apps. Shared tooling follows its documented technical scope and maintainer-approved exceptions. Craft PR Review's existing Tauri and TypeScript UI is one such exception. It does not relax this rule for native apps. Preserve any existing web build.
5. **Time goes through `chrono`.** Do not hand-roll calendar conversion. For dates beyond what `chrono` can represent, follow [standards/time.md](https://github.com/storytold/craft-contrib/blob/main/standards/time.md).
6. **Contributor credits.** Add or edit only your own person's entry in `contributors/people.toml`, and only after asking them. Never guess a name or hand-edit `contributors.json`.

## Licence allowlist

Dependencies and assets use SPDX identifiers. An expression is allowed when every identifier is in this list. A repository may name a shorter list. It may not add an identifier here by editing its own copy of these rules.

- `ISC`
- `MIT`
- `BSD-2-Clause`
- `BSD-3-Clause`
- `Apache-2.0`
- `OFL-1.1`
- `Ubuntu-font-1.0`
- `Bitstream-Vera`
- `CC0-1.0`
- `CC-BY-4.0`
- `CC-BY-SA-4.0`
- `LicenseRef-Public-Domain`

`LicenseRef-Storyteller-Trademark` is not in this list. It covers Storyteller brand marks only, where a repository already says so.
<!-- craft-house-rules:end -->

# AGENTS.md — rules for every agent and contributor

Bios is an open-source workspace for designing DNA: projects that stay organised, a plasmid
editor, and every command open to any agent over MCP. It is native Rust, runs on your own machine
and needs no account. The two blocks above are the ArtCraft house rules, copied unchanged because
Bios is built on ArtCraft's open libraries and follows their way of working.

## Where Bios differs from the blocks above

Bios is not an ArtCraft repository, so four rules are met differently. Nothing else is relaxed.

- **Fonts (Rust rule 3).** Bios's two interface fonts live in this repository under
  `crates/bios-ui/assets/fonts/`, each with its licence file and a row in `ATTRIBUTION.md`.
- **Clean room (Rust rule 2).** Bios began as a web app by the same owner. Porting that app's code,
  tests and recorded answers into this repository is the project moving its own work, not copying
  another product. The clean-room rule applies in full to SnapGene and every other
  product: learn from what they visibly do and from public specifications, never from their code,
  files or artwork.
- **CI (core rule 11).** There are no workflows yet. Until there are, `cargo xtask ci` on your own
  machine is the gate, and rule 7 applies: report what you ran.
- **Contributor credits (Rust rule 6).** There is no `contributors/` folder yet.

## What Bios is for

1. **Projects that stay organised and easy to find.** One copy of each construct, in a project,
   never a pile of renamed files.
2. **Sharing** a project with other people.
3. **A good editor.** The look and feel of the Bios web app is the reference; match it before
   changing it.
4. **Any agent, over MCP.** Bios has no agent of its own and must not grow one. A person's own
   agent connects and works on their projects with them.
5. **Lineage and history.** What was built from what; every change kept with who made it; any
   change, a person's or an agent's, can be undone.
6. **Open to reshaping.** The code is laid out so that a person's agent can change their copy.

## The one rule of the design

**Everything Bios can do is a command.** The window, the command line, MCP and the change log all
go through one command registry in `bios-engine`. A feature that exists only as a button, or only
as an MCP tool, is a bug. To add a capability, add a command; the surfaces follow.

## Crates and layers

A crate may depend only on crates in strictly lower layers. `cargo xtask layers` enforces it, and a
new crate is added to the table in `xtask/src/layers.rs`.

| Layer | Crate | Holds |
|---|---|---|
| 0 | `bios-core` | The sequence model, file formats and DNA maths. No I/O beyond parsing. |
| 1 | `bios-layout` | Where every base, lane and label sits. Numbers only, no drawing. |
| 2 | `bios-project` | Project homes (a folder now, a server later), identity, the change journal, lineage. |
| 3 | `bios-sync` | Keeping a project folder in step with the same project on a Bios server, for sharing. Never merges. |
| 4 | `bios-engine` | The session and the command registry. Every edit is one journaled command. Also `editor`: what a key press does to a construct being edited by hand, with no drawing and no saving. |
| 5 | `bios-mcp` | The MCP tools, built on the registry. |
| 5 | `bios-update` | Bios updating itself from its GitHub releases (craft-update): the updater the window and `bios-cli` hand the engine for `check_for_update` and `update_bios`. |
| 6 | `bios-ui` | Drawing. The only library crate that may use egui. |
| 7 | `bios`, `bios-cli`, `xtask` | The window, the headless command line, the checks. |


## A project on disk

A project is any folder, wherever it is: Bios opens folders the way a code editor does. There is
no central folder of projects.

```
My Project/
  bios.json                      the project's id and name
  <folders>/<name>.gb            plain GenBank, readable by any other tool
  <folders>/<name>.dna|.fa|...   SnapGene, FASTA, EMBL: read as they are
  .bios/
    journal/<construct-id>.jsonl one line per write, only ever added to
    objects/<sha256>.gb          the text of every version, so any of them can be restored
    originals/<path>             a file that was not GenBank, kept when Bios first changed it
    lock                         held while a write is in progress
```

- **Opening writes nothing.** `Project::open` takes any folder. One without `bios.json` is read
  from its sequence files, named after itself, and stays untouched by every read. The first write
  (an edit, an import, a move, anything journaled) makes `bios.json` and `.bios/` and writes down
  each file as it was found, so going back to it still works. `Project::is_project` and
  `Project::open_saved` are for code that needs a folder that already is one.
- **Identity.** A construct's id is a `bios_construct_id:` comment inside its own file, so the file
  is the truth and nothing breaks when a folder is moved. A file that does not carry one yet goes
  by an id made from its path until a change writes the id into it. A file that is not GenBank
  becomes `<name>.gb` on its first change, and the original goes to `.bios/originals/`.
- **Finding projects again.** `bios_project::recents` keeps the folders opened, the latest first,
  in `recents.json` in Bios's settings folder (beside the window's `config.toml`;
  `BIOS_CONFIG_DIR` moves both), with the projects page's pins and hand order. The window, the
  command line and every agent read and add to that one list; `bios-cli mcp` with no folder
  serves it. A deleted project goes to the system Trash; "remove from list" never touches the
  folder.

`bios-project` is the only crate that reads or writes inside a project folder.

## A folder that means something

Most folders are organisation and know nothing about themselves. A folder a feature made is not:
a build folder is one backbone opened one way, a verification folder is one submission measured
against its maps. Such a folder carries a record and a drawer of its own, inside the folder it
describes, so that renaming or moving it carries them along at no cost:

```text
<folder>/.bios/folder.json        its type, when it was made, and what that type remembers
<folder>/.bios/<dir>/<name>.json  one item of that type, too big to ride on the record
```

`FolderMeta` is `{ v, kind, created_at, data }`, written flat into `folder.json` so the file reads
the way the Bios web app writes it (`{"v":1,"type":"build","createdAt":...,...}`). The kind is a
string and `data` is free JSON: `bios-project` knows a folder has a type, never what a build or a
verification is. A new kind of folder is new commands in `bios-engine` and nothing in this crate.

```rust
project.write_folder_meta(folder, &FolderMeta::new("verification", data))?;
let meta = project.folder_meta(folder)?;      // None for an ordinary folder, and for a damaged record
let typed = project.folder_metas()?;          // every folder that has one
project.write_folder_file(folder, &item_file_name("runs", name, "run"), &json)?;
project.folder_file(folder, name)?;           // None when it was never written
project.folder_files(folder, "runs")?;        // the names in one drawer directory, sorted
project.remove_folder_file(folder, name)?;
```

A drawer name is minted, never typed: one directory deep at most, and `item_file_name` turns an
item's own name into one — a slug kept honest by a digest of the original, so two names that clean
up alike cannot overwrite each other and writing the same item again replaces its own file.

## Adding a command

1. Write the change as a function over a `Construct` in `crates/bios-engine/src/commands/`, and add
   a `CommandSpec` for it to that file's `COMMANDS`: its name, a description written for an agent
   that has never seen Bios, and its parameters.
2. If it edits a construct, go through `Session::edit`. That is what checks the version, saves and
   journals; a command that writes any other way is a bug.
3. Add a test in `crates/bios-engine/tests/` that calls it through `Session::execute`.

Names and parameters follow the Bios MCP tools (`read_plasmid`, `insert_sequence`, ...). Positions
in parameters and results are 1-based and inclusive, as a person counts them.

## Correctness comes from recorded answers

A wrong melting temperature or a missed cut site costs someone weeks at the bench. DNA maths and
file reading are checked against recorded reference answers under each crate's `tests/fixtures/`.

- A port keeps the original's tests, in the original's order, with the original's numbers.
- Do not change a recorded answer to make a test pass. If an answer is wrong, say so and show why.
- Coordinates are 0-based and inclusive at both ends. A feature whose end is before its start
  wraps through the origin of a circular construct.
- Qualifiers Bios does not understand are kept and written back unchanged.
- Test material committed here is public or synthetic (pUC19 from GenBank L09137, made-up
  constructs). A few answers were recorded from private data that cannot be published; their
  tests sit in a `private` module, are ignored, and read those files from the folder
  `BIOS_PRIVATE_FIXTURES` names (laid out like this repository). Without it they are skipped; with
  it, `cargo xtask ci` runs them too.

## Files that are not yours

- A file opened from outside a project is imported as a copy. The original is never written.
- Inside a project: one write at a time per file, check the file has not changed before writing,
  read it back before reporting it saved, and never write an empty construct.
- History is only added to. Going back restores an earlier version as a new entry.

## Before you call it done

```
cargo xtask ci
```

It runs the formatter, clippy with warnings denied, every test, the asset check and the layer
check. For anything that draws, also look at it: `bios <project or file> --shot out.png` saves a
picture of the window and quits (`--open <plasmid>` picks what is open, `--after <seconds>` waits
first, so the picture can show a change made from outside while the window was up).

For anything an agent can call, try it as an agent would: `bios-cli run <project> <command>
'<json>'`, or connect a real agent to `bios-cli mcp <project>` (or `bios-cli mcp`, which serves the
folders opened in Bios; set `BIOS_CONFIG_DIR` to a scratch folder so a test leaves the person's
own list alone).
