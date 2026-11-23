# Parity with the Bios web app

The native app is meant to be the web Bios, 1:1, running on your own machine with no account.
This file tracks every piece of the web app's Edit view and how far its port has come. The
spec is the web app's own code; each piece names the files it comes from.

Status: **missing**, **partial** (exists but differs), **matches** (checked side by side against
the web).

Score: the share of drawn pixels that match the web reference within 2 points, for that region,
on a private lab construct at 1440×900, against the web app built without its assistant
(`VITE_AI=off`), which is the layout Bios native follows. Measured with the parity harness. The harness drives the web
app with JavaScript, so it lives outside this repository (in `bios-parity`, next to it).

Left out on purpose: sharing, accounts and the account menu, the assistant chat panel (your own
agent connects over MCP instead, so Build's "Ask Bios" is not here either), the old pickme
plate grid and BLAST in Verify, project avatars and invitations.

## Foundation

| Piece | Web source | Status |
|---|---|---|
| Colour, size, radius and shadow tokens | `design/tokens.css` | matches (`theme.rs`) |
| Font weights 400–700 incl. 550 and 650 | `design/fonts.css` | matches (`theme.rs`) |
| Ghost / solid button, segmented group, glass | `design/primitives/button.css` | ported (`primitives.rs`) |
| Lucide icons | `lucide-react` imports | ported (`icon.rs`, `lucide.rs`) |
| Focus ring, row hover/active | `styles.css` `:focus-visible`, tokens §rows | ported |

## Shell

| Piece | Web source | Status | Score |
|---|---|---|---|
| Head band, columns, gaps | `App.tsx`, `layout/AppLayout.tsx` | matches | |
| Places (Edit / Build / Verify) | `layout/PlaceStrip.tsx`, `placeStrip.css`, `mode.tsx` | **differs on purpose** (Max, 2026-11-22: a Claude-style sidebar): no 60px strip and no Bios mark; the places are 30px rows in the sidebar under the project's name, the place's colour at 12% and weight 600 on the one you are in (the strip's `.place.is-active`); pressing it again does nothing, the toggle in the head's first row (right of the traffic lights) puts the panel away and sits at the tab strip's left end while it is away | |
| Project sidebar: header, rows, folders | `layout/SessionNav.tsx`, `explorer.css` | **differs on purpose** (Max, 2026-11-22): one column from the window's left edge; the project's name leads to all projects (no "‹ All projects" row); folders are collapsible sections (small muted header, chevron on the right, + for a new plasmid in it on hover), open until shut, the shut ones remembered per project; plasmids in no folder under "No folder" at the end; Verify's list sits under the same head | |
| Sidebar right-click menus, rename, delete confirm, drag | `layout/RailMenu.tsx`, `SessionNav.tsx` | partial (folder delete has no command; no keyboard tree navigation) | |
| Sidebar resize 168–420 | `layout/RailResizer.tsx`, `railWidth.ts` | matches | |
| Plasmid tabs | `layout/TabStrip.tsx`, `tabStrip.css` | matches (a tab dragged over its neighbours moves as it goes) | tabs 99% |
| Name, length, description over the map | `editor/PlasmidTitle.tsx` | matches | |
| All projects page | `projects/ProjectsHome.tsx`, `ProjectsRail.tsx`, `ProjectMap.tsx`, `projectFilter.ts`, `pinned.ts` | differs on purpose: it lists the folders opened lately (Bios opens folders anywhere, like a code editor), a card's foot has a third line saying where its folder is, an Open folder tile sits beside New project, and a card can be removed from the list; otherwise as checked side by side at 1440×900 (no Shared row, ask box, people or Share: local only) | |
| Settings: Appearance, Keyboard, About | `platform/SettingsModal.tsx`, `keys/KeyboardSettings.tsx`, `keymap.ts` | matches (checked side by side; no Account, update check or demo switch) | |
| ⌘P Go to plasmid | `explorer/FilePalette.tsx`, `matchTree.ts` | matches (looked at; lists the project's plasmids, not files) | |
| How Build / Verify work (sheet, ⌘/, ?) | `intro/*` | matches (looked at; the web has no Edit sheet) | |
| Welcome sheet (Help > How Bios Works, ⌘/ on the projects page) | none | native only (Max, 2026-11-23): opens by itself on the first launch until closed; folders, history, the agent connect line with Copy, updates; New project and Open folder | |
| Documentation under Help | `docs/DocsPage.tsx`, `manifest.ts`, `public/docs/*.html` | matches (checked side by side; the text is the web's and still speaks of the browser and the assistant) | |
| Tabs come back when a project opens; quitting with an unsaved change asks | `workspace/tabs.ts`, `restoreTabs`, `platform/unsaved.ts` | ported (the quit question is not exercised by a test: it needs a real window close) | |

## Editor

| Piece | Web source | Status | Score |
|---|---|---|---|
| Map / rows split with handle | `editor/PlasmidEditor.tsx` | matches | |
| View modes: map, both, sequence | `editor/MapControls.tsx` | matches | |
| Map style: circular, linear, fold | `editor/MapControls.tsx` | partial (fold greyed: Bios cannot fold) | |
| Floating glass toolbar | `MapControls.tsx`, `HistoryControls.tsx` | matches (scrim blur not drawn) | toolbar+nav 100% |
| Navigator strip with range label | `editor/overview/MapMinimap.tsx` | matches | |
| Zoom and pan on the map | `overview/PlasmidOverview.tsx`, `viewport.ts`, `useViewport.ts` | matches (the zoom eases toward an accumulating target at `ZOOM_RATE` 22, frame-rate independent; a circular map unrolls into the line between widths 0.62 and 0.3, `renderMorph`: backbone, bands, heads and names along the band, no scale or cuts mid-unroll, as the web; clicks mid-unroll read off the ring or the line by the viewport's projection and catch no feature, as the web's `fracAt`) | widths 1 / 0.5 / 0.4 / 0.3 against the web: same drawing |
| The flywheel (wind a ring up, it spins, sparks, blurs, burns and rolls off; Go get it) | `overview/flywheel.ts`, `useFlywheel.ts`, `PlasmidOverview.tsx` (carriage, `Flames`, `RpmReadout`), `styles.css` `.overview-rolled` | matches (physics and its 22 tests ported exactly; the blur is five faint copies of the ring, egui having no blur filter; the glow is drawn where browsers draw it, inside the rim; the spark seed starts at 1, not the clock; never turned off for reduced motion) | each stage against the web, caught with `--flywheel` |
| Cut-site names on the map | `PlasmidOverview.tsx` `layoutLineAnnotations` | matches | map 100% |
| Map drawing (ring and line) | `editor/overview/` | matches | |
| Amino acids and ORFs in the rows | `rows/RowTranslations.tsx`, `translations.ts`, `SequenceRows` `findOrfs` | matches (a coding feature through the origin is not translated) | |
| The rows follow a zoom of the map | `editor/PlasmidEditor.tsx` 660-824 | matches (a zoom by hand scrolls the rows to the map's window, a pan does not, an echo of the rows' own click does not; the map alone at 9 px a base turns to the sequence behind a 500 ms door, and zooming the rows back out returns to the focus it came from) | |
| The strip | `editor/PlasmidEditor.tsx` 706-830, `rows/SequenceRows.tsx` (`strip`, `zoomLine`, 248-460, 764-832, 1153-1241, 1316-1335, 1566-1585) | partial (in the split the map pane becomes the rows' own row, unwrapped, at 9 px a base, opening on the map's window behind a 500 ms door: features, primers, parts, ORFs, cuts with their names, amino acids and the selection, the weld on the pane's middle, the last base in view at the right; plain scroll slides it, pinch or ⌘-wheel zooms about the base under the pointer to 48 px a base; the seam draws and drags its window; the rows below follow its zoom; a caret put outside it from elsewhere slides it; zoomed under 7 px a base it hands back and the map flies out to that stretch at 5 px a base; leaving the split or the construct puts the map back. Not yet: Find's captions, as on the web; a primer's end grip shows but does not drag, as on the web) | strip side by side by eye |
| Rows zoom, edge scroll, double-click a selection | `rows/SequenceRows.tsx` (`onWheel`, `edgeScroll`, `onPointerUp`) | matches (pinch or ⌘-wheel about the base under the pointer; under 7 px a base the rows hand back to the map at the base in their middle, the map zooming to a quarter of the construct or 5 px a base, and come back at 16 px a base; a drag held at an edge scrolls on; a double-click opens the selection's menu, a single click collapses after 280 ms) | |
| Sequence rows | `editor/rows/` | matches (native also draws joined features as segments and cut staircases across rows, from web work that was never shipped) | rows 100% |
| Layers, base colour, label density | `leftpanel/ViewTab.tsx`, `editor/layers.ts` | partial (Features, Enzyme names, Binders, ORFs, Amino acids, Reverse strand, Position ruler, Interactions, Parts, the counted Splice-map hits toggle, base colour and density honoured; the rest greyed) | |
| Interactions layer | `rows/RowInteractions.tsx`, `overview/chords.ts`, `interactions.ts` | matches (a lane of brackets and stubs per row, chords through the ring, brackets over the line, hover in either view greys back what is not wired; checked side by side on wrap_demo and the lab construct) | |
| Parts lane | `rows/RowFeatures.tsx` (`TRACK.parts`), `SequenceRows.tsx` | ported (`/pragma="Teselagen_Part"` features in their own lane, outermost; left off the map and the feature lists. Not compared side by side: the parity harness seeds the web through its server, which drops parts) | |
| Enzyme dial and Enzymes panel | `EnzymesMenu.tsx`, `EnzymesPanel.tsx`, `enzymeList.ts` | matches (the dial's names-off note and All enzymes door; the panel's search, Common / All / IIS / Blunt, groups, on-the-map toggle and sites; also the `list_enzymes` command. Enzymes put on the map by name are not drawn lit) | |
| Contents panel | `editor/Inspector.tsx`, `leftpanel/PropertiesTab.tsx`, `propParts.tsx` | matches (General, Interactions with the idle line, Features with show/hide, Parts from the parts lane and Binders from the held binders, one row per oligo with its strays and remove) | |

## Cards, menus, find

| Piece | Web source | Status |
|---|---|---|
| Feature card | `editor/FeatureCard.tsx`, `feature/ActsOn.tsx` | matches (Acts on is written with `set_feature_actions`: free verbs with the construct's own as suggestions, several targets, remove, lands on a tick or a 600 ms pause, turns Interactions on; checked side by side) |
| Primer hover card, cut hover card | `primer/PrimerCard.tsx`, `CutHoverCard.tsx` | matches for the primer card (checked side by side on the lab construct and wrap_demo, 2026-11-22: the whole drawn box with its name answers, the card sits by that box, stays up with the menu open and through an end drag, and comes in with `sr-in`); the cut card is not shown, the rows always print the names |
| Selection readout pill and its verbs | `editor/SelectionReadout.tsx` | matches (+ Primer's direction guess, name, verdict and ✓ checked side by side; landing parks the caret at the footprint's end; the verbs ease into their hover; the pill comes in with `sr-in`) |
| Right-click menu | `editor/rows/RowMenu.tsx` | partial (Binder and Remove binder ported; Translate this floats frames ±1–3 over the selection, held per open plasmid and never saved, and removes them; adds Remove feature, which the web did through its assistant) |
| Find in the plasmid (⌘F) | `editor/FindBar.tsx` | matches (Alignment the default and Exact; see Binders and Find below for the size limit) |
| Binders, binder hover card, Find's alignment drawn with captions, primer end drag | `binders.ts`, `BinderCard.tsx`, `alignment.ts`, `find/useProbeHits.ts`, `rows/RowProbe.tsx`, `readoutYield.ts`, `primer/PrimerDrag.tsx` | ported (2026-11-22). Differences: every query is aligned by Smith-Waterman on its own thread, up to 60 M cells (about 2 s; a 5.3 kb query on a 6 kb circle), and the bar says the limit where the web hands larger queries to minimap2 up to 50 kb |
| New-feature sheet: name, why it's here, direction, what it serves, what it is | `editor/AskSheet.tsx`, `feature/newFeature.ts` | partial (every field, through `add_feature`'s new category, why and serves; drawn with the native sheet's chips rather than the web's glass) |

## Last 10%

| Piece | Web source | Status |
|---|---|---|
| Hover, focus, cursors, tooltips on every control | `styles.css` | partial (the Edit place checked 2026-11-22: hovering a feature or primer washes it into its bases; the primer grip, the ask's toggle (pointer) and verdict (help) have the web's cursors; tooltips draw ⌘ ⇧ ⌥ ⌃; the grip says Drag to resize the primer. Tooltips come up after egui's 0.5 s, where a browser's `title` waits on the system) |
| Motion: 120 ms ease, reduced motion respected | `DESIGN-PHILOSOPHY.md` §6, `styles.css` | partial (buttons, segments and the pill's verbs ease into hover over 0.12 s with CSS's `ease`; the pill, primer card and feature card come in as `sr-in` / `feature-card-in`; the primer grip fades over 0.1 s. Not yet: other panels' entrances, reduced motion, which egui cannot read. The map's motion is its own piece above) |
| Keyboard: ⌘1–9, ⌘W, Ctrl-Tab, ⌘B, ⌘\ | `keys/commands.ts` | matches (every row of `COMMANDS` has its chord, checked 2026-11-22; ⌘\ opens the assistant, which is left out; the Windows-only ⌥⌘← → are not bound) |
| Empty states, drop overlay, floating banners | `PlasmidEditor.tsx`, `AppLayout.tsx`, `layout/fileDrop.ts` | partial (several files drop at once behind the overlay, with one sentence for what could not open; it says "project" where the web says "session") |

## Scores so far

| Date | Change | lab construct | wrap_demo |
|---|---|---|---|
| 2026-11-21 | Baseline before the port | 17.6% | 22.0% |
| 2026-11-21 | Shell, editor chrome, cards and rows ported | 99.6% | 91.7% (the web shot's ring keeps the turn of the plasmid opened before it) |

## Functions still to build (from the functional inventory, 2026-11-21)

Everything the web app does, except sharing, accounts, the assistant and ordering from vendors.
Size: S under an hour, M half a day, L several days.

### Edit

| Function | Web source | Size |
|---|---|---|
| Import SBOL | `packages/formats` | L |
| Fold (ViennaRNA): waiting on a licence decision | `fold/`, `packages/fold` | L |

### Build (the whole place)

Ported. A backbone opened with two enzymes or a region, inserts pasted or taken from the project,
one Gibson design and one plasmid each, kept in a build folder that is the backbone, with a table
of deduplicated pieces to order. Everything goes through the engine, so an agent can do all of it.

| Piece | Web source | Status |
|---|---|---|
| Typed folders on disk: `.bios/folder.json` and the drawer beside it | `shared/folders.ts`, `server/workspace/sidecar.ts` | matches (payloads are plain JSON, not gzipped, so a design can be read with `head`) |
| Naming a build folder, and reusing one holding these bases | `layout/folderNaming.ts`, `build/buildFolder.ts` | matches (its own tests, with the web's cases) |
| Which enzymes can open a backbone, ranked by cut count | `build/openings.ts` | matches |
| Pasted FASTA / sheet / lines into rows | `build/intake.ts` | matches |
| The order across a folder, TSV and .xlsx | `build/order.ts` | matches (the workbook is written by hand, no zip crate) |
| Commands: list/read a build folder, set its cut, design, build, list/drop designs, the order table | the folder routes in `server/http/app.ts` | matches (eight commands, each tested through `Session::execute`) |
| The cut bar on the map: pickers, the other arc, unique/all, how much leaves | `build/CutBar.tsx` | partial (the swap says "other arc": the web's ⇄ is not in the interface face) |
| Picking a cut site on the map | `useBuild.tsx` `chooseCut` | partial (a press near a site picks it; the ticks do not yet wear 1 and 2) |
| The design on the map: band, arms, one run per piece, seams and their temperatures | `PlasmidOverview.tsx` band layer, `useBuild.overlay` | matches (ring and line; the ring gives up margin for it) |
| The opening washed out, and the features leaving with it drawn dim | `PlasmidOverview.tsx` `dimSpan`, `bands.ts` `overlapFraction` | matches |
| Before/after scrub | `CutBar.tsx`, `useBuild.tsx` `stage` | matches (the product is read for the row on screen, when it is asked for) |
| The console: intake, rows with the pieces bar and warnings, problems first, the bottom overview | `build/BuildConsole.tsx` | matches |
| Files in: dropped anywhere, or picked; sheets through the intake parser, the rest read as sequences | `useBuild.tsx` `addFiles`, `layout/fileDrop.ts` | matches |
| The list windowed, with an overdraw either side | `BuildConsole.tsx` the window | matches (a thumb instead of the host's scrollbar) |
| Compute off the drawing thread, with progress | `useBuild.tsx` `compute` | matches (a thread with its own session, one design per insert) |
| The folder's page: backbone, cut, gel bar, clipped features, designs with recipes, linear map per design, to-order table | `build/FolderBuild.tsx` | matches (a stale design says so in words, not on a badge) |
| Build folders in the panel: BUILD's icon and colour, New build | `layout/folderTypes.ts`, `SessionNav.tsx` | matches |
| Ask Bios | `BuildConsole.tsx` `askAI` | left out on purpose (no assistant) |

### Verify (the whole place)

Drop a run, place its samples on maps, align them on this machine (read mapping is not in this build of Bios yet), read the consensus differences, call
each file, copy the picks. Every action is an engine command (`create_verification` …
`export_alignment`, beside `map_reads`); the window drives them. Checked by looking on a real
paired, multi-member-gzip plate (8 wells, 2 maps) against the web's front door and board.

| Function | Web source | Status |
|---|---|---|
| FASTQ, plain and multi-member gzip | `reads/fastq.ts` | matches (`bios-core::reads::fastq`, the web's tests) |
| R1/R2 by name, then by the first record's cluster id | `reads/pairs.ts` | matches (`reads::pairs`, the web's tests) |
| Front door: name the run, drop reads and maps | `verify/LocalVerify.tsx`, `startVerification.ts` | matches |
| Verification folders side by side in the panel: folder → maps → runs with dots and notes | `SessionNav.tsx`, `sessionConstructs.ts` (`nestRuns`, `runNote`, `siblingLabels`) | matches (no right-click menu on a verification; delete is on the board's ⋯) |
| Maps copied into the folder, base fingerprints, staleness | `verificationFolder.ts`, `FolderVerify.tsx` | matches |
| The board: rows, chips (call colour + finding dot), drag between rows and to the tray, ×, clear with undo, add map from project or file, change map, rename in place, delete with confirm, open row with whole names and numbers, "every sample differs" sentence, tray hint | `verify/Board.tsx`, `boxOrder.ts`, `matchNames.ts` | matches (the web's tab for the folder is a static tab here; a row has no drop target of its own, a drop lands on the page) |
| Name matcher | `verify/matchNames.ts` | matches (the web's tests, on the real plate names) |
| Run per sample off the window's thread, progress, cancel | `useReads.tsx`, `ReadsDrop.tsx` | partial (cancel stops between samples; the mapper itself has no stop) |
| Results to differences and the three verdicts | `reads/mapperResult.ts`, `runView.ts` | matches (a circular run's overhang columns past the consensus are accepted, the web refused them) |
| Runs stored in the folder | `runCodec.ts`, server `saveRun` | partial (gzipped mapper JSON in the folder's drawer, not codec v5) |
| Alignment view: linear map with pins, pileup with zoom, pan, lane scroll and base hover, depth strip with navigator, ← → through changes, shift ← → file to file, Run again, Esc | `AlignmentView.tsx`, `MapperPileup.tsx`, `DepthTrack.tsx` | partial (no feature-click framing, no column mark, no "go to base" field; the map is drawn here, not by the editor's overview) |
| Calls 1/2/3 apart from the finding, advance to the next uncalled | `AlignmentView.tsx`, `callOrder.ts` | matches |
| Copy the picks | `Board.tsx` `copyWells` | matches |
| Downloads: consensus, SAM, coverage TSV, differences TSV, zip | `MapperFiles.tsx` | matches (zip written by Bios with the same nine files) |
| Help panel on an alignment | `AlignmentHelp.tsx` | matches (the intro sheet is the shared `intro.rs`) |
| Reads pane in Edit | `reads/ReadsPane.tsx`, `useReads.tsx` | missing |
| Free the reads | VERIFY.md | missing (not in the web either) |

### Everything else

Built since this list was made: the all-projects page, Settings with Keyboard, ⌘P, the Build and
Verify sheets, the documentation, tabs that come back and the quit question (see Shell above),
and GFF3, JBEI SEQ XML and Teselagen JSON import (`bios-core::interchange`). A project deleted
from its card goes to the system Trash after an inline confirm, and can be put back from there;
"remove from list" only forgets it. There is no `~/Bios`: any folder opens as a project, read from
its files, and becomes one (`bios.json`, `.bios/`) on its first change. The recent folders are the
registry the window, `bios-cli` and MCP share. One window only: eframe can draw more viewports, but
each would need its own copy of the window's whole state, which is not cheap today.

## Primers, side by side (2026-11-22)

lab construct and wrap_demo, the web at :5173 driven by Playwright, the native window by
`--select`, `--ask`, `--hover`, `--caret` and `--find`, with a forward primer (324..343, binds
twice), a reverse one (400..421) and one through the origin of wrap_demo (791..10).

| Step | Result |
|---|---|
| Select, the pill, + Primer: direction guessed from the drag, name, verdict | matches (`21 bp · Tm 54°C · 52% GC`, `unique · 2 flags`; a right-to-left drag guesses 3'←5') |
| The made primer in the rows: ribbon, chevrons, letters, name, both strands, through the origin | matches |
| Hovering it: the card | matches (`twice_F … 2 off-targets · 1 flag`, `ori_F … 79 off-targets`) |
| Dragging an end: grip, live Tm, limits | matches, with two differences below |
| Right-click, Remove primer, undo and redo; following an upstream delete | matches (the primer slides with its bases; undo puts it back) |
| A 5' tail drawn lifted | matches on Find's alignment; the web's made primers carry no oligo of their own, so a made primer with a tail exists only here |

Still different, on purpose or for later:

- A drag is one step in the history here (`update_primer` once, on release); the web writes
  every base the end passes through, so its ⌘Z walks the drag back one base at a time.
- The web lays its grips on the drawn box's outer edges, which take in the name beside the 5'
  end and the 3' chevron; here they sit on the first and last base. The web also puts grips on a
  primer through the origin, where a drag collapses it (`PrimerDrag.tsx` 69-73 clamps against an
  end that is smaller than the start); here such a primer has none.

