# Handover: RevuCraft, 13 November 2026

Written for the next session. It covers the work since the earlier handover
(`docs/handover-revucraft.md`, head `61201663`), which is now out of date. Read this first, then
`REVUCRAFT.md`, `CLAUDE.md` and `AGENTS.md`.

## Where things stand

- Repo `Azurehawke/pdfcraft`, branch `revucraft`. Head is `67dfaddb`, pushed, working tree clean
  when this was written.
- Tests at `67dfaddb`: `pdfcraft-ui-egui` lib 570 passed, integration `it` 1025 passed, 23 ignored.
  `cargo fmt --check` and clippy with `-D warnings` were clean for `pdfcraft-ui-egui`.
- CI at last check: `fa0a9726` had Linux and Android green, and Windows was still running. `67dfaddb`
  was just started. Check the runs before building on this branch.
- The app's build number is shown in Help ▸ About. Steve's newest build before this session was
  **292**, and it predated `fa0a9726`. CI run numbers (179, 189, 203, 212) are GitHub counters, not
  the app build number. Don't compare them.

## Commits this session (oldest first)

- `cf52a711`: first "Place image on layer" (a stamp). Superseded by `2a519ded`.
- `2a519ded`: placed pictures are linked, on their own layer. See "Linked pictures" below.
- `caeefc2d`: wall joins reach further and heal both ways. See "Wall joins" below.
- `fb0e1a6e`: test that a thinner wall joins both thicker walls.
- `0f36dbc0`: a wall end on an edge keeps its snap under Ortho. Superseded by `67dfaddb`.
- `fa0a9726`: wall joint boxes highlight and keep the grab cursor on hover. See "Cursor".
- `67dfaddb`: Ortho and centre-line snaps work together. See "Ortho and snapping".

Earlier on this branch, before this session: `4b58ddf6` (new lines start at 0.1 pt, the minimum
printable width), `a008e27c` (alignment guides from hovered points), `5873faba` (snap to the edges
of markup lines and to where they cross), `1ec4f703` (PlanCraft references removed),
`da628ae6` (title block gets a quarter-inch frame around the page).

## Linked pictures ("Place image on layer", Document tab)

- Lives in the Document tab's Edit group as `edit.place_image` (after `edit.image`). It is listed in
  the Links pane as a row with the image icon and "picture on its own layer". Steve chose this
  design (option A) over a stamp.
- Storage: `crates/engine/src/cad_links.rs`. `LinkKind` gained `Picture`. Records live in /PCLinks
  with `Kind`, `Form`, `OCG`, `Matrix`, `W`, `H`, `Size`, `MTime`, `Hash`, `Updated`, `Placed`.
- `place_picture` fits the image to 80% of the media box, centred, scale capped at 1.0. It creates
  an OCG named after the file. `update_picture` re-reads the file and keeps the link's scale and
  top-left corner. `update_table` shares `replace_form`.
- `Edit::PlacePicture` in `crates/engine/src/lib.rs`, label "Place image on layer". The UI is
  `links_ui::place_picture_dialog`. It accepts png, jpg, jpeg, tif, tiff, gif, bmp, jp2, j2k and jpx.
- The old stamp code (`place_image`, `placed_picture_rect`) was removed from `content_ui.rs`.
  "Add image" (page content) is unchanged.
- Test: `placed_pictures_are_linked_on_their_own_layer_and_update_from_the_file` in
  `crates/engine/src/tests.rs`.
- Not yet tested by Steve on screen: the Links row, the layer eye and lock, "Remove picture", and
  moving the picture on the sheet.

## Wall joins

- `crates/engine/src/wall_joins.rs`: `join(q, end, existing) -> Option<Joint>` with
  `Joint { quad, partner, with: With }` and `With::{Corner, Butt, Gap}`. `partner_edit`, `join_end`,
  `wall_edits`, `joined_to`.
- Reach is `max(REACH, thickness)`, so an end a little short of a wall still joins.
- In-line walls butt-join end to end. They are rejected when drawn back over each other.
- Healing: when a new wall crosses an existing wall's free end, that end is joined too. The heal
  pass runs over `!used.contains` free ends and emits `Edit::SetWall`.
- `Edit::AddWall` gained `gaps: Vec<(u8, f64, f64)>`. Every `AddWall` literal in tests sets
  `gaps: Vec::new()`.
- Mutation proofs were run: reverting the reach fails two tests, disabling butt joins fails the
  butt test, and disabling the heal fails the heal test.
- Open: Steve started a new wall on a selected wall's centre joint. The start snapped, but the new
  wall did not merge into the perpendicular wall. We don't yet know which end failed or where he
  released the pointer. Ask him for the failing end and the release point, then reproduce that
  geometry in a test.

## Cursor and joint boxes

- Cause, found in `draft_edit::grips()` (about line 1263): `grips` set the grab cursor when hovering
  a corner or end connector, then returned `false`. The next handler, `comments::page_input`
  (comments.rs about line 1361), then set its own cursor over the same spot: Crosshair on point
  handles, resize on handles, Move on the body. The last cursor set in a frame wins, so the grab
  never showed.
- Fix in `fa0a9726`: `grips` returns `true` while hovering, which stops the chain. The grip under the
  pointer fills with the accent colour. Corners and connectors are drawn after the hover is computed.
- The chain in `canvas.rs` (about line 4135) uses `||` short-circuiting. A handler that returns
  `true` stops the ones after it, so pick the return value with care.
- Hover-consume side effect: `consumed` also blocks the text I-beam, image pick and link click while
  the pointer is over a grip. That is intended.
- Not verified by Steve on a build that contains the fix. Build 292 predates it.

## Ortho and snapping

- `crates/ui-egui/src/draft.rs`, the snap block (about lines 1296–1420) and the Ortho branch after it.
- `ortho_point(last, raw, point, kind, lines, tolerance)` returns the point Ortho should use:
  - An edge snap is kept where the square run from `last` (across or down) meets the snapped line
    near the pointer. `draft_snap::square_crossing` does this and reuses `guide_crossing`.
  - Otherwise a perpendicular snap is kept, and anything else is straightened with `orthogonal`.
- `ortho_lines` collects the markup segments (with the Wall tool, the walls' centre lines) and the
  shape segments. `pointed` is the pointer before any snap.
- This applies to every tool, not only walls. The Line tool also keeps an edge snap where it meets
  the square run.
- Shift flips Ortho: Ortho on with Shift is free, and Ortho off with Shift is straight.
- Not verified by Steve yet. Test: a wall approaching another smart wall's centre line with Ortho on
  should stay straight and still snap.

## Process rules (from CLAUDE.md, and as followed here)

- Work on `revucraft` only. Push with `git push origin HEAD:revucraft`. Never push `main`.
- Scope builds and tests to the crate touched: `cargo clippy -p pdfcraft-ui-egui -j 2 --all-targets -- -D warnings`
  and `cargo test -p pdfcraft-ui-egui -j 2 --lib --test it`, under `timeout`. Run `cargo fmt -p`
  before committing.
- Prove a test fails before the fix: copy the file to `$HOME/*.bak`, apply a perl mutation, run the
  test, restore with `/bin/cp -f`, and confirm with `cmp`.
- If a test fails, look at the actual value before changing the test. One test here failed on float
  rounding (59.99999999999999 against 60), and the fix was a tolerance, not a change to the logic.
- Commit trailers used this session:
  ```
  Co-Authored-By: Claude Haiku 5.5 <noreply@anthropic.com>
  Claude-Session: https://claude.ai/code/session_<id for the new session>
  ```
- Disk: `target/debug/incremental` once grew to about 21 GB. Delete it when space runs low.
- Steve's earlier rule (from the older handover) was to pause for confirmation before each push
  unless he says "push". This session he accepted commit and push after each change passed tests.
  Check with him if unsure.

## Open items

Waiting on Steve's testing:
- Joint box cursor and highlight (`fa0a9726`), on a build that includes it.
- Ortho with centre-line snap (`67dfaddb`), on a build that includes it.
- The wall that won't merge into a perpendicular wall (see "Wall joins").
- Place image on layer, on screen (see "Linked pictures").

Carried over from before this session:
- Per-sheet comment numbering.
- Print checklist discoverability.
- Android APK rebuild for the header and Back fixes.
- Android `Cargo.toml` `unsafe_code = "deny"` conflict (left unchanged).
- Markup lookup caching on crowded pages.
- Consider raising the default line width to 0.25 pt if 0.1 pt prints too faint.
- The title block layer is not moved to the bottom of the layers panel.
- From the earlier handover: font family for the text box card, font size for callout comments,
  an easier way to find Properties, and a bottom layer for spaces made before the change.

## Where to start

1. `git fetch && git status`, then check the CI runs for `67dfaddb`.
2. Ask Steve for the build number in Help ▸ About, and for his test results on the open items.
3. For the merge bug, ask which end failed and where he released the pointer, then write a
   regression test that reproduces it before changing `wall_joins.rs`.
