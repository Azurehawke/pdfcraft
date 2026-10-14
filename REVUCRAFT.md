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

## Roadmap

Ordered by value for construction work. Each item is one or more commits with tests.

| # | Item | Status |
|---|------|--------|
| R1 | Feet and fractional inches (`12'-6 1/2"`) for lengths: format, compound `/Measure` write and read, automation `fractions` option | Done, tested |
| R2 | Calibration panel: format picker (decimal or feet-inches, 1/2 to 1/64) and common architectural and engineering presets (1/8" = 1'-0", 1:100…) | Written, needs Windows test run |
| R3 | Count tool (symbols counted per page, grouped by subject) and angle tool | Planned |
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
