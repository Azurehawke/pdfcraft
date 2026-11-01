# Handover: RevuCraft (branch `revucraft`)

Written for the next session. Read this, then `REVUCRAFT.md` (the queue, newest entries near the
build 220/221 notes), then `CLAUDE.md` and `AGENTS.md`.

## Where things stand

- Repo: `Azurehawke/pdfcraft`, branch `revucraft`. Head `61201663`, which is pushed and green.
  All four CI jobs passed (RevuCraft checks, Linux, Windows, Android APK).
- Working tree was clean at handover. Nothing is stashed.
- Nothing has been run locally: crates.io was blocked in the previous session, so every change
  was checked only by rustfmt and CI. First job in a session with crates.io: `cargo check -p`
  on the touched crates (`pdfcraft-annot`, `pdfcraft-engine`, `pdfcraft-ui-egui`), then their
  tests, under `timeout`, with `-j 2`.
- Steve's rule: pause for confirmation before each push, unless he says "push". Commits use the
  Co-Authored-By and Claude-Session trailers from the environment.

## Commits from the last stretch (oldest first)

- `346f21ad`, `71d98bbb`: padlock on each space (`FLAG_LOCKED`, `locked_at`). A click on the
  padlock emits `Edit::LockAnnotation`. `space_lock_icons` returns early for pages with no spaces,
  which was needed to pass the frame-budget perf test.
- `cb80a32c`: a new space goes to the bottom of the page's drawing order. `punch::set_space` now
  returns the space's index and moves a fresh space to index 0 of `/Annots`, via the new
  `crate::send_to_bottom`. `punch_ui::make_space` names it and keeps the selection on index 0.
  Spaces made before this keep their place. The test is `a_new_space_goes_to_the_bottom_of_the_drawing_order`.
- `64b6edbf`: one box for the selection and the comment. `comments::Composer` has a `quote`
  (`comments::Quote { text, quads }`). After Add comment, the card shows Highlight, Underline,
  Strikethrough, Replace text and Copy above the text, with Cancel and Post beneath. The
  selection bar is hidden while a card is open. A text box card has a font size control
  (`ComposerKind::TextBox { size, font_size }`, `TEXT_BOX_FONT = 12.0`).
- `4588fb2a`: the Properties rail button gets a dot while a markup is selected.
- `61201663`: CI fix. The test index assumed an empty page, and an unused binding failed clippy.
- Earlier, already on the branch: space name placement and its handle (`/PCLabelAt`, build 221),
  callout glue stage 1 and 2 (`/PCGlue`, `callout_glue.rs`), option 2 glued arrows
  (`/PCGlueStart`, `/PCGlueEnd`, `arrow_glue.rs`, redraw on box move), and the straight callout
  leader (test reads CL[2..4]).

## Open items (Steve's list)

Done in code, untested by Steve on a device:
- Spaces at the bottom layer (new spaces only).
- Merged selection/comment card, with the markup row and Cancel/Post beneath.
- Padlock per space.
- Properties dot.

Not done:
- Font family in the text box card. Font size is done, for text boxes only.
- Font size for callout comments.
- Making Properties easier to find beyond the dot. The rail button is still icon-only at 34px.
  Ideas: a right-click "Properties…" entry on a markup, or opening the box on first selection.
- Bottom layer for spaces made before this change. Existing indices would have to move, so it
  was left alone on purpose.
- Stage 2 and option 2 are marked "done, untested" in REVUCRAFT.md. Steve has tested option 2's
  redraw and the straight leader; the padlock and merged card are not tested yet.
- Optional: an ATTRIBUTION.md credit for WordCraft's connector design (MIT/Apache-2.0).

## Design notes that bite

- Annotation order in `/Annots` is drawing order, and the UI addresses annotations by index
  `(page, index)`. Any change that reorders `/Annots` shifts indices on that page. Reroute and
  glue lookups use box names, not indices, so they survive a reorder. Selection does not, so
  after reordering, select the moved annotation by its new index.
- Glued arrows are re-routed by `arrow_glue::reroute_all`, which runs after each comment or
  content edit in the engine.
- Connection sites: eight per box (four side middles, then four corners). Snap distance is
  `SNAP_PX = 12.0` screen points. Alt forces a free arrow.
- `apply_edit` returns a bool, and comment edits go through `comment_page(...)` in `editing.rs`.
  New edit variants need a label in `EDIT_LABELS` and a match arm in `Label::of`.
- i18n: `crates/ui-egui/src/i18n/*.tsv`. New strings need every language file. The helper used
  before was a small Python script that takes a JSON map of key to 11 translations (ar, bg, de,
  fr, hu, it, ja, pt-br, ru, uk, zh-hans). `Font size` and `Strikethrough` already exist.
- Never panic: no `unwrap`, `expect` or raw indexing on input-derived positions. Use `get`,
  `get_mut` and `checked_add`, as in `send_to_bottom`.

## Environment notes

- The previous sandbox could not resolve `index.crates.io` and returned 403 for
  `static.crates.io`, even after the domains were added to the allowlist in Settings. A new
  session may not have the allowlist applied; if cargo still fails, check the allowlist first.
- `gh run list -R Azurehawke/pdfcraft --branch revucraft` shows CI. Failed logs:
  `gh run view <id> --log-failed`.
