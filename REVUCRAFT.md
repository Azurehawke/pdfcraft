# RevuCraft

RevuCraft is a construction-drawing markup and takeoff workbench, built as a fork of
[PdfCraft](https://github.com/storytold/pdfcraft) on the `revucraft` branch. It keeps
PdfCraft's engine, clean-room rules, layering and quality gates (see `CLAUDE.md` and
`AGENTS.md`), and adds the workflows estimators, architects and field crews use on
large-format drawing sets.

**Status:** started 2026-10-14. R1 (feet-and-fractional-inch lengths) passes
`cargo test -p pdfcraft-measure` and `pdfcraft-automation`'s hostile-argument tests on
Windows, and `cargo clippy -p pdfcraft-measure -p pdfcraft-automation -- -D warnings` is clean.
Run the gates below after every change.

```
cargo fmt --check
cargo clippy --workspace -- -D warnings
cargo test -p pdfcraft-measure -p pdfcraft-automation -p pdfcraft-engine
```

## Windows

Every push to `revucraft` also runs `.github/workflows/revucraft-windows.yml`, which builds
RevuCraft for Windows x64 with its own icon and name and publishes it as the
`revucraft-windows-latest` prerelease: `RevuCraft-setup.msi` (installs for all users, with Start
menu and desktop shortcuts, and offers RevuCraft under "Open with" for PDFs) and
`RevuCraft-portable.zip` (runs from any folder; settings stay in `RevuCraftData` beside it). It
installs beside PdfCraft. The files are not code-signed, so SmartScreen may ask once: More info,
then Run anyway. The app checks that release at start and offers newer installers. Packaging:
`packaging/windows/revucraft-package.ps1`, `revucraft.wxs`, `revucraft-installer-ui.wxs`.

## Linux

Every push also runs `.github/workflows/revucraft-linux.yml` (Ubuntu 22.04, so it runs on any
64-bit distro with glibc 2.35 or newer) and publishes the `revucraft-linux-latest` prerelease:
`revucraft_amd64.deb` (`sudo apt install ./revucraft_amd64.deb`; menu entry, opens PDFs, later
builds upgrade it), `RevuCraft-x86_64.AppImage` (one file for any distro: `chmod +x`, run) and
`RevuCraft-linux-x86_64.tar.gz` (a plain folder: `RevuCraft/bin/revucraft`). The app id is
`com.azurehawke.revucraft`, so it installs beside PdfCraft. The app checks that release at start
and offers its page when a newer build is out. Packaging: `packaging/linux/revucraft-package.sh`.

## Android

The `android` branch's tablet build is merged in. Every push to `revucraft` runs
`.github/workflows/revucraft-android.yml`, which builds `revucraft.apk` as its own app
(`io.github.azurehawke.revucraft`, named RevuCraft) so it installs beside PdfCraft, and publishes
it as the `revucraft-latest` prerelease. On the tablet, open the repository's Releases page and
install `revucraft.apk` from there; `docs/android.md` covers file access and pen input. To pick up
later tablet work, merge `android` into `revucraft` again.

## What PdfCraft already provides

- `pdfcraft-measure`: calibration, page viewports (`/VP`), distance, perimeter and area
  as standard measurement annotations, snapping to drawing geometry, CSV export, and
  automation tools (`measure_*`).
- `pdfcraft-annot`: markup, shapes, ink, text boxes, stamps, replies and status.
- `pdfcraft-compare`: text compare and a pixel-region visual compare.
- Layers, forms, page organizing, combine/split, incremental saves, undo, the CLI and
  MCP server, desktop and web builds.

## Next build: refinements list

Started 2026-10-15. Steve's requests are marked **(Steve)**; the rest are suggestions to confirm.
Tick items as they ship.

Panels and layout
- [x] **(Steve)** Draggable dock boxes: reorder by dragging the title bar, drag a box out to float it
      as its own window over the drawing, drag it back to dock it.
- [x] Remember the dock (which boxes, order, collapsed, width, floating positions) between sessions.
- [ ] Workspaces: save and switch dock layouts ("Office", "Field"), as Adobe apps do.
- [x] Properties box: the selected markup's colour, line width, opacity, layer, subject, label and
      measurement scale, edited in place (Revu's Properties panel).
- [ ] Markups list box: every markup and measurement in a sortable, filterable table (type, label,
      layer, page, author, status, value), click to go to it, export CSV (roadmap R4).
- [x] Status bar: cursor position in feet-inches, page scale, snapping on/off, current layer.

Markup and measuring
- [x] **(Steve)** Form tools on the ribbon: a Forms tab (or group) with PdfCraft's field tools
      (text, checkbox, radio, drop-down, list, button, image, date), Detect fields, Preview,
      tab order and Field properties. Done: a Forms tab (Fields, Form, Tab order, Data); Preview
      is in the Prepare a form box.
- [x] **(Steve)** Option to turn off the hover tooltip with the author's name on markup: it
      covers the live measurement label when drawing over other lines. Also: never show it while a
      drawing or measuring tool is placing points. Done: the Tips switch on the status bar (also in
      Preferences), and never while drafting or measuring.
- [ ] Tool chest box: saved tools with their settings (a red cloud on "Revisions", a 6" data jack
      labelled "PC"), recently used tools (roadmap R5).
- [ ] Rectangle area (two taps), angle and radius tools.
- [ ] Symbols: rotate (and snap to the nearest wall), edit the label after placing, renumber a
      selection; electrical and security libraries; make a symbol from your own drawing.

Tablet
- [ ] Touch mode: bigger buttons and handles, long-press for the context menu, two-finger tap to
      undo.

Builds
- [ ] One build number shared by Android, Windows and Linux.

Draft tab: CAD-style drawing on the PDF (direction decided 2026-10-15)

RevuCraft takes over the drafting Steve needs instead of merging with CADCraft: plans are drawn
to scale on PDF sheets, marked up, and handed over as PDF, with DWG/DXF for CAD users. CADCraft
stays its own app; its geometry, DXF and DWG crates (Rust, MIT OR Apache-2.0) are reused rather
than rewritten.
- [x] New sheet: ARCH A–E (9×12 to 36×48) and ANSI A–E (8½×11 to 34×44), portrait or landscape,
      with a drawing scale (1/4" = 1'-0", 1:50…) set on the page, so measuring works at once;
      optional simple title block (title block still to come).
- [x] Drawing by numbers: lengths in feet-inches (`15'`, `12'6"`), relative `@0,15`, polar
      `@15<90`, ortho and polar angles, a big keypad on the tablet; later fed by the laser
      meter and the LiDAR scan.
- [ ] Tools (line, polyline, rectangle, walls, circle, arc and dimension done; doors and windows to come): line, polyline, rectangle, circle and arc, walls with a thickness, doors and windows
      from the symbol library; object snaps (end, mid, intersection, perpendicular) shared with
      measuring.
- [x] Editing: grips, move, copy, rotate, offset, trim and extend; dimensions (from the
      measuring tools); text. Trim and extend work on lines and polylines against other markup
      (not yet against the PDF's own drawing); boxes and symbols rotate about the centre point
      but stay upright.
- [x] Cursor coordinates in feet-inches in the status bar; a page origin to measure from.
- [ ] Everything drawn stays editable, on layers, and saved in the PDF (annotations with their
      geometry in drawing units).

DWG/DXF round trip
- [ ] Export: drawn geometry, layers (names, colours), symbols as blocks with their labels,
      text and dimensions to DXF and DWG, at real-world size (drawing units from the page
      scale).
- [ ] Import: a DXF or DWG (changed in CAD) placed back on the PDF sheet at the same scale and
      origin, as a new revision under the existing markup, so markups, photos and counts stay
      put; or as a fresh sheet.
- [ ] Revision compare between the old and new import (overlay in two colours).

## Roadmap

Ordered by value for construction work. Each item is one or more commits with tests.

| # | Item | Status |
|---|------|--------|
| R1 | Feet and fractional inches (`12'-6 1/2"`) for lengths: format, compound `/Measure` write and read, automation `fractions` option | Done, tested |
| R2 | Calibration panel: format picker (decimal or feet-inches, 1/2 to 1/64) and common architectural and engineering presets (1/8" = 1'-0", 1:100…) | Done, tested on Windows |
| R2b | Revu-like shell: dark charcoal theme, accent colours, workflow tabs and ribbon, RevuCraft branding, update checks against this fork's releases | Done (build 2) |
| R3a | Layers you manage, as in CAD: make, rename, colour, lock, show and hide layers (standard optional content groups, so other viewers honour them); a current layer that new markups, measurements, symbols and photos go on; move a markup to another layer. Markup on a hidden layer is hidden (vendored hayro patch 23) | Written (build 3) |
| R3b | Symbol library (networking first: data, voice, dual and quad jacks, wireless AP, cameras, PoE, floor box, rack, patch panel, switch, fiber, demarc, display, card reader, speaker, J-hook, conduit stub-up). Symbols tab: pick a symbol, layer, real-world or paper size and a label (optionally auto-numbered), then tap to stamp repeatedly. Counts per symbol, label and layer exported as CSV | Written (build 3); electrical, fire alarm and security libraries next |
| R3c | Photo callouts: pin a photo (from the gallery, Drive or, on Android, the camera app) to the drawing; tapping the pin shows it with its caption. Photos are turned upright and scaled to 2048 px before they are stored | Written (build 3) |
| R3d | Angle tool | Planned |
| R4 | Markups list: a sortable, filterable table of every annotation and measurement, with status, author, page, subject, value, and custom columns stored on the annotation | Planned |
| R5 | Tool chest: saved markup and measurement presets (style, subject, scale-aware), stored as a RevuCraft file format | Planned |
| R6 | Overlay compare: two revisions drawn in contrasting colours with alignment by two points, building on the visual compare | Planned |
| R7 | Area extras: cutouts, depth to volume, dynamic fill | Planned |
| R8 | Sheet sets: title-block page labels, auto-linking of sheet references, batch stamps and slip-sheets | Planned |
| R9 | Large CAD sheets: tiled, cached vector rendering for pages with millions of paths | Depends on PdfCraft's own renderer |
| R10 | Pen-first measuring and markup on Android tablets (Galaxy Tab, S Pen) | APK build in place (from `android`); measuring with the pen not yet tuned |

Real-time shared sessions are out of scope until the single-user workflows are solid.

## Naming

"Revu" and "Bluebeam" are trademarks of Bluebeam, Inc. RevuCraft is an independent
project, not affiliated with or endorsed by Bluebeam. Behaviour is reimplemented from
public specifications and black-box observation only, following PdfCraft's clean-room
rules. Before a public release, the fork must also remove the ArtCraft names and logos
(`docs/brand/LICENSE-brand.txt`).
