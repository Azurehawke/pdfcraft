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
- [x] **(Steve)** Workspaces: save and switch dock layouts ("Office", "Field"), as Adobe apps do.
      Done: View ▸ Workspaces and the rail's Workspaces button; Markup, Drafting and Review to start.
- [x] Properties box: the selected markup's colour, line width, opacity, layer, subject, label and
      measurement scale, edited in place (Revu's Properties panel).
- [x] Markups list box: every markup and measurement in a sortable, filterable table (type, label,
      layer, page, author, status, value), click to go to it, export CSV (roadmap R4).
- [x] Status bar: cursor position in feet-inches, page scale, snapping on/off, current layer.

Markup and measuring
- [x] **(Steve)** Form tools on the ribbon: a Forms tab (or group) with PdfCraft's field tools
      (text, checkbox, radio, drop-down, list, button, image, date), Detect fields, Preview,
      tab order and Field properties. Done: a Forms tab (Fields, Form, Tab order, Data); Preview
      is in the Prepare a form box.
- [x] **(Steve)** Dimension tools (aligned, linear, radius, diameter) on the Markup tab as well,
      since dimensions are mostly added to existing PDFs; they stay in the Draft tools panel too.
- [x] **(Steve)** Photo pins drawn as a camera icon (not a plain pin), so anyone reading the
      PDF can tell what they are. Done: a camera on a disc in the layer's colour.
- [x] **(Steve)** Photo callout: a box on the sheet with the photo scaled down to fit and a
      description beside it, on the side the user chooses (left or right of the photo), with a
      leader line to the spot. Placed like a callout (the spot, then the box); pick or take the
      photo, type the description; an existing photo pin can become one. Prints with the drawing.
      Done: Symbols tab ▸ Photos (side and photo height); Properties flips the side, views the
      photo and turns a pin into a callout.
- [x] **(Steve)** Scaled line width for Draft lines: a Width option on the ribbon for the
      drawing tools in real-world units (presets 4 1/2" interior wall, 6" exterior wall, thin
      linework; or plain paper points), converted with the page's scale (e.g. 6" at 2" = 15'
      is about 4.8 pt); square ends and mitred corners for wide lines; the width stays with the
      line through edits and scales with Scale. The Wall tool stays as it is.
- [ ] **(Steve, later update)** Existing PDF linework in the drafting tools: Trim and Extend use the PDF's
      own lines as cutting and boundary edges; Explode turns PDF linework into editable markup
      lines (taking the originals out of the page, recorded and undoable), which can then be
      trimmed and extended like any other line.
- [x] **(Steve)** Scale by dragging: after the base point, the pointer sets the factor (with a
      live preview and the factor shown), as well as typing it.
- Kept as is (Steve): the Measure tab's ruler (two clicks, plain measuring line).
- [x] **(Steve)** Option to turn off the hover tooltip with the author's name on markup: it
      covers the live measurement label when drawing over other lines. Also: never show it while a
      drawing or measuring tool is placing points. Done: the Tips switch on the status bar (also in
      Preferences), and never while drafting or measuring.
- [x] **(Steve)** Tool chest box: saved tools with their settings (a red cloud on "Revisions", a 6" data jack
      labelled "PC"), recently used tools (roadmap R5). Done: the Tool chest box (rail and View ▸
      Side panels): Save current tool, then click it to use it with those settings.
- [x] **(Steve)** Rectangle area (two taps), angle and radius tools.
- [x] **(Steve)** Symbols: rotate (and snap to the nearest wall), edit the label after placing, renumber a
      selection.
- [x] **(Steve, build 174)** Symbols: electrical and security libraries; make a symbol from your own drawing.
      The Symbols panel lists Networking, Electrical (outlets, GFCI, dedicated, floor outlet, switches,
      lights, exit sign, junction box, panel) and Security (motion, door contact, glass-break, keypad,
      siren, smoke and heat detectors, electric strike, mag lock, request-to-exit). Make symbol (a
      selection's menu) turns drawn lines and shapes into a symbol of your own, named and listed under a
      trade or My symbols. Your symbols are kept in this device's settings.

- [x] **(Steve)** Calibrate on the Measure tab: click two ends of a known length, type it in the
      value box beside the point (12'6", 150, 3.5m), on this page or every page.
- [x] **(Steve)** One way to type values everywhere: click on the drawing, the value
      box opens at the pointer, type, Enter (as Calibrate does). For Scale (factor), Rotate (angle),
      Move/Copy (distance or @x,y), Offset (distance), line/polyline/wall lengths while drawing,
      photo pin and callout captions, and a symbol's label after placing. The ribbon entry stays.
- [x] **(Steve)** Snapping: perpendicular (the foot of a line from the last point, for
      drafting and measuring), and midpoints of drawn markup lines (the PDF's own lines already
      snap to midpoints); the snap kind shown at the pointer while drafting, as measuring does.
- [x] **(Steve)** Custom columns and statuses: your own fields on markups (drop ID, port,
      patch panel, cable type) and a status list (Pulled / Terminated / Tested), set in Properties,
      shown in the Markups list and exported with it.
- [x] **(Steve)** Cable runs: a run measured along its route plus a fixed allowance per
      run (drop down the wall, service loop at the rack), with totals by layer for ordering.
- [x] **(Steve)** Legends that build themselves: a legend box on the sheet listing every
      symbol used, with its picture and count, kept up to date.
- [x] **(Steve)** Summary report: markups, photos, comments, statuses and custom fields as
      a PDF report grouped by layer, with the photos in it (the customer report).
- [x] **(Steve)** Your own symbols: import SVG files into the symbol library (Symbols
      tab), placed, sized, rotated, labelled and counted like the built-in ones; kept with the app.
- [ ] **(Steve, later update)** The drafting value box takes the cursor as it opens after each click, so
      a length or @x,y (e.g. `@2<45`) can be typed without clicking into it first (Backspace in the
      empty box takes back the last point; Enter and Escape as now).
- [ ] **(Steve, later update)** Turn angles while drafting: `@2<<90` turns 90° from the previous line
      (anticlockwise; `<<-90` turns the other way) and goes 2, for walking around a room; `@2<90`
      stays the absolute angle (0 right, 90 up).
- [x] **(Steve, build 65)** "Open" becomes "New": one dialog with a new drawing from a page size
      (Recent, ANSI, ARCH, ISO, Saved; Custom; width, height, units, orientation, drawing scale,
      pages; Create) on the top half and a file browser (places, folder path, search, files, file
      name and type, Open) on the bottom half, after VectorCraft's New Document. Home's New or open and
      Draft's New sheet open it too; Android shows recent files and Open from device… instead of the browser.
- [x] **(Steve, build 75)** Simplified Scale panel (mock-up approved): a card with the page's scale
      in large type; a Preset / Custom / Calibrate switch (one dropdown; "1 in = 8 ft"; two steps
      with Pick two points); one "Show lengths as" dropdown in place of unit, decimals, format and
      fraction; Use on This page / All pages / An area (drag a box instead of typing the viewport);
      Apply scale; a tidy Measurements list with Export CSV. The instructions move to the status
      bar, the snap settings under its Snap button, and the Distance/Perimeter/Area tabs go.
      Label, viewport name and decimals sit under More options.
- [x] **(Steve, build 75)** Select several markups at once (pen strokes, shapes, symbols, Draft lines):
      drag a box on empty page with the Select tool to pick everything inside it; Shift+click or
      Ctrl+click (⌘ on a Mac) toggles a markup in or out, as in Acrobat and Bluebeam. The group moves,
      deletes, copies (Ctrl+C, then Ctrl+V pastes copies beside them on the same page) and takes
      colour, fill, thickness, layer, lock and custom-column changes together, with one box around it.
- [ ] **(later)** The New dialog's Saved tab: page sizes of your own, kept by name.
- [x] **(Steve, build 65)** Tool icons in the theme's accent colour (ribbon, left tools, rail, Draft
      tools, fly-outs, dock and panel buttons); the chosen tool a solid accent tile with its icon in
      the accent's contrast colour; captions and text stay neutral (mock-up made). An "Icons" setting
      beside the accent (Display theme / Preferences): Accent, Neutral, or a colour of their own
      (Amber, Blue, Teal, Green, Red, Violet), the accent staying the chosen tool's highlight. Draft
      tools: what the tool makes in the icon colour, existing lines grey.
- [x] **(Steve, build 65)** Home screen for RevuCraft's work instead of PDF chores: cards for
      Draw a floor plan, Measure and calibrate, Place symbols, Photos on the plan, Markups and
      reports, Cable runs, and New / Open (the New dialog); a tagline about plans, markup and
      takeoff.

- [x] **(Steve, build 102)** Windows and Linux: choosing Download in Check for updates starts the
      download and then closes RevuCraft, so the installer can replace it. Unsaved work asks first
      (Save / Don't save / Cancel, as when quitting); Cancel keeps RevuCraft open. Android: the
      same unsaved-work prompt before the download starts, but RevuCraft stays open (Android's
      installer stops it when it replaces the app, which would otherwise lose unsaved changes).
- [x] **(Steve, build 102)** Place drawing… is easy to find: the Links icon is always on the rail,
      and Place drawing… is in the File menu, the command palette and the Draft ribbon's Sheet group.
- [x] **(Steve, build 102)** Closing CADCraft ends Edit original: the drawing goes back to Up to date
      (a save just before still updates the sheet). Done editing in the Links pane ends it by hand
      when CADCraft was already open or the system's opener started it.
- [x] **(Steve, build 104)** Linked drawings work on any device: links record the path from the PDF,
      and each device looks there, then at the full path, then by name next to the PDF and in the
      folders below it. A copy with the same drawing (fingerprint) stays Up to date; Relink also
      relinks the other missing drawings found in the same folder.
- [x] **(Steve, build 116)** Merged storytold/pdfcraft (1,383 commits, PdfCraft 0.10.1): reading
      colours, Enhance camera photos, reverse pages and Ctrl-drag copy in Organize, fills and border
      widths on text boxes and callouts, a Folders section on Home, custom document properties,
      faster rendering of heavy pages, and many fixes. RevuCraft keeps its ribbon, dock, rail and
      status bar; its own wording is translated in its 11 languages. Builds are now numbered along
      RevuCraft's own history.
- [x] **(Steve, build 121)** Linked tables from GridCraft workbooks: Place table… (File menu, Draft
      ribbon, Links pane) places a sheet's range, named range or print area from an XLSX, XLSM, XLS,
      ODS, CSV or TSV file, printed by GridCraft's engine with its formulas worked out, on its own
      layer. It is listed in Links, updates in place (keeping its top-left corner) when the workbook
      changes, and Edit original opens it in GridCraft. The Workbook box shows the whole workbook
      read-only; CSV and TSV columns are as wide as their text.
- [x] **(Steve, build 125)** Named measurements and Send quantities: measurements are named as
      drawn (their label, else Area_1, Distance_2, Cable_run_3…) and renamed in the Properties box;
      the Markups list shows the names. Send quantities (Measure ribbon, Markups list, Links pane,
      Workbook box) writes every measurement and symbol count to an XLSX workbook's Takeoff sheet,
      each quantity a named cell (`=Kitchen_floor*6.50`); only that sheet, the workbook's names and
      its calculation setting are rewritten. The Links pane says when the drawing changed since, or
      the workbook was saved without the latest quantities, and sends them again; tables placed
      from the workbook update.
- [x] **(Steve, build 128)** Placed tables resize: drag the corner while moving (Move or resize on
      the sheet), or type a size (10% to 1000%) in the Links pane; an update keeps the size and the
      top-left corner. Placed tables and drawings can be removed from the sheet (Links pane, or
      Delete while moving them), with their layers; Undo brings them back.
- [x] **(Steve, build 129)** Wall justification: the Wall tool draws from the centre line, the inside
      face (the thickness goes out of the room, for tracing existing walls) or the outside face; the
      room is the side the run turns towards, and Flip side sends a straight run's thickness the
      other way. The preview shows the side before each click.
- [x] **(Steve, build 131)** Stroke alignment, as VectorCraft's Align Stroke: closed shapes (rectangles,
      circles, ellipses, polygons, closed polylines and splines; drafted or marked up) stroke centred
      on their path, inside it or outside it. The Draft ribbon sets it for new shapes and the
      Properties box for any closed shape (several at once); the shape stays where it was drawn.
      Open lines stroke centred.
- [x] **(Steve, build 136)** Insert sheet (Document ribbon and Pages menu): one click adds a sheet
      after the current one, its size and drawing scale (New sheet and Insert blank page made one);
      Insert sheet… picks another size and scale. The Draft ribbon's New sheet button is gone (New
      in the tab bar starts a new drawing).
- [x] **(Steve, build 136)** Menus grouped and ordered: File (new and open, save and close, Create and
      Export submenus, placed drawings and tables, Optimize and Security, print), Edit, Pages (insert,
      extract, Rotate submenu, page setup, bookmarks), View, Help; a test keeps every menu command in.
- [x] **(Steve, build 136)** Drafting tools put themselves down once their shape is drawn, as in
      CADCraft (a polyline, wall, spline or cloud once it is finished): the Select tool comes back to
      pick the new shape. Doors, windows and the modify tools stay for the next one. Measuring and
      markup tools are as they were.
- [x] **(Steve, build 137)** A typed length goes toward where the pointer last was on the page, even
      when the pointer has gone by the time Enter is pressed (Windows hides it while typing; the
      ribbon's entry box takes it off the page). It went right (0°) instead.
- [x] **(Steve, build 138)** A pasteboard, as in Illustrator and VectorCraft: a line or shape that runs
      off the sheet shows on the area around it (faded; dark lines lightened on the dark theme), and
      can still be selected, moved, grip-edited and snapped to; drafting and markup tools draw there
      too. The sheet stays the print border. Measuring tools stay on the sheet.
- [x] **(Steve, build 144)** Rulers along the top and left of the drawing, as in Illustrator: a marker on
      each follows the pointer; the numbers read in the sheet's drawing scale (feet and inches at
      1/8" = 1'-0") or in paper units on an uncalibrated sheet, and the ticks thin out or fill in
      with the zoom. Zero at the sheet's corner; shown or hidden from View ▸ Rulers (Ctrl+R).
- [x] **(Steve, build 146)** Guides, as in Illustrator: drag one out of a ruler, move it, type its exact
      position (double-click it with the Select tool; in drawing units or paper inches), drop it on a
      ruler to remove it, lock or clear them from the corner where the rulers meet. Drafting and markup
      tools snap to them. Kept with the sheet in the PDF but never printed.
- [x] **(Steve, build 146)** Printable area: a margin preset (e.g. 1/4" all round, or your printer's own
      margins) shown as a dashed line, so title blocks and linework stay where the printer reaches;
      chosen from the corner where the rulers meet.
- [x] **(Steve, build 147)** Every tool tab works like Draft: a left-hand tools panel (collapsible to an
      icon strip) holds the tools in groups, and the ribbon above shows the chosen tool's options.
      Markup and Review become one Markup tab: Text markup, Draw, Shapes, Text, Stamp, Photos (Pin a
      photo and Photo callout, moved from Symbols), Dimensions, Comments (list, summarize, hide,
      flatten, import, export). Measure gets a panel too (calibrate, the measuring tools, cable runs;
      scale, snap and export stay on its ribbon). Symbols keeps its ribbon, which is already the
      symbol's options (library, layer, size, label), legend and counts. Compare moves to Document;
      Document and Forms, being one-off commands rather than tools, keep their buttons in the ribbon.
- [x] **(Steve, build 157)** Format copy, as Illustrator's eyedropper: click a markup, drafted shape or
      PDF/DWG line and its stroke colour, width, dash, line ends, stroke alignment, fill, opacity and
      (for text) font become the current style for every markup and drafting tool. With markups
      selected first, the click restyles them instead (one undo). From a PDF/DWG line only its width
      and colour come. Shift-click (or an option) copies the layer too.
- [x] **(Steve, build 157)** Photo callouts: the dot stays anchored to the drawing; moving the photo box
      moves only the box, and the leader re-runs from the dot to the box's nearest point.
- [x] **(Steve, build 165)** Hairlines in the drafting tools only: the Draft ribbon's Line width gets a
      Hairline (0 pt) choice, a layer's drafting width can be Hairline, and Format copy from a hairline
      PDF line makes the drafting width a hairline, so drafted lines match an all-hairline drawing on
      any printer. Markup and comment tools keep a real width: from a hairline they take only its colour.
      Format copy always says what it copied ("Copied: black, hairline (drafting); colour only (markup)").
- [x] **(Steve, build 165)** Photo callouts: resize the photo and the text separately. With the callout
      selected, a handle on the photo's outer corner scales the photo (keeping its proportions; the box's
      height follows), and a handle on the text's outer edge sets the text width (the description
      rewraps and the box grows or shrinks to fit). The dot stays put; the leader re-attaches. One undo
      each; the photo height and text width also in the Properties box for exact sizes.
- [x] **(Steve, build 165)** Layers as Illustrator's: sublayers nested in layers (stored as the PDF's
      nested layer order, so other viewers show the tree), a parent's eye and lock acting on everything
      inside, drag rows to reorder or nest, drag down the eye column to set several (the lock column
      not yet). Drafted
      lines go on a Drafting layer by default (its own sublayers for walls, doors, dimensions) unless
      the current layer is inside it. Flatten per layer from its menu (Flatten Drafting burns only the
      drafting into the page; comments stay comments). Reuse VectorCraft's Layers panel (rows, columns,
      drag and drop), mapped onto PDF layers.
- [x] **(build 166)** Layers on repaired files: a file whose cross-reference table had to be rebuilt
      (CADCraft's and LibreCAD's often are) is rewritten at every edit, which renumbers its objects.
      The Layers panel lost new layers' colour, fold, lock and rename controls and their current-layer
      highlight; a hidden layer came back on after the next edit; Print could show hidden layers.
      Layers are now matched by their place in the file. Each layer's colour dot has a ring, so a
      black layer (Drafting) shows on the dark panel.
- [x] **(Steve, build 171)** Drafting kept apart from comments: drafted linework (anything on the
      Drafting layer or its sublayers, or drawn with a Draft tool and marked so in the PDF) is left
      out of the Comments panel, so only real comments and markups show there. The Markups list
      keeps it under its own Drafting filter (off by default) for counting and editing; on the page
      it still selects and edits as before.
- [x] **(Steve, build 175)** CADCraft brought up to date (169 commits): layouts in millimetres plot in
      millimetres, plot windows kept, second-layout entities, DWG dimension text and DGN-bloated
      drawings, thin wide polylines stay visible, import and rendering bounded against hostile drawings.
- [x] **(Steve, build 176)** PdfCraft brought up to date (about 1,080 commits since Oct 21, upstream a2ec27b): a
      merge build on its own. RevuCraft keeps its scale panel, dock rail and layer panel; upstream's
      line and polygon reshaping, comment hover order, page gaps and repair work come in.
- [x] **(Steve, build 182)** From BIMCraft (its snaps and schedules, after Revit's):
  - Drafting: a direction sticks to angle steps (15° to start; 5, 30, 45, 90 or off in the Snap
    settings) from east and from the line before, along a dashed guide, landing where the guide
    meets a line; markup lines snap where they cross; a ruler with graduations along the line being
    drawn and the angle at its corner; SE, SM, SI, SP, SN typed for one snap alone on the next point
    (SO none, SS all).
  - Markups list: calculated columns (a formula over Value, Count, Page and your columns), filter
    rules (contains, equals, greater or less than, has or hasn't a value), grouping by any column with
    each group's count and totals, and the totals of everything shown.
  - Cable runs: a system (network, link, circuit) and cable type for new runs, kept in your System
    and Cable type columns (added when first used); cable totals by layer, system or cable type.
- [x] **(Steve)** Smart walls lite, after BIMCraft's walls: walls stay PDF markup (other viewers
      see the drawing) but know about each other (`pdfcraft_annot::walls`, `/PCWall`, `/PCOpening`).
  - [x] Joins: a run's corners, and a free end that stops on a smart wall: at its end an L (both
        mitred, no line across), along it a T (the new wall stops at the face, which opens for it).
  - [x] Doors and windows hosted by their wall: one markup each, they move, stretch, copy and are
        deleted with it; moved on their own they slide along it, and the wall re-opens.
  - [ ] Joined walls follow: dragging one wall's end stretches the walls joined to it (next).
  - [x] Room checked (build 186): a door or window only goes where its wall has room (no other
        opening, wall junction or joined corner there; the preview turns red, "No room here"),
        and one slid along its wall stops at its neighbour.
  - [x] Flip a placed door's hinge side or swing (Properties box and context menu).
  - [x] Door and window marks (D1, W1…) numbered through the document, drawn by the opening and
        listed (as Door/Window, drafted or not) in the Markups list.
- [x] **(Steve, build 186)** Undo and Redo buttons in the top bar (beside Save), naming the step.
- [x] **(Steve, build 186)** The distance box opens on the far side of the line's start from the
      pointer, so the ruler, angle and live length stay in view whichever way a line is drawn. On a touch screen (build 188) the last place
      the finger or pen was stands in for the pointer.
- [x] **(Steve, build 189)** Photos from the camera: Photo pin and Photo callout get Camera or Files on
      the ribbon (Camera on the tablet). With Camera a tap on the plan opens the camera app at once,
      no file picker, and the tool stays on for the next spot (tap, shoot, caption, tap…).
- [ ] **(Steve, next)** Photos on comments: Add photo on a sticky note (camera or file), pinned beside
      it and joined to its thread, so it shows in the Comments panel and the summary report.
      Straighten as a document for photos of paper (the scan's edge finding and Colour clean-up).
      One camera bridge with the android branch's Scan with camera (`pcTakePhoto`), with Keep a copy
      in the gallery (on for site photos, off for scans).
- [x] **(Steve, build 190)** Camera on Windows (the Surface Pro 7's rear camera): Camera or Files on the
      photo tools there too. Camera opens Windows' Camera app; RevuCraft watches its Camera Roll
      (wherever Windows says it is, OneDrive included) for the new photo, places it once saved and
      comes back to the front. Open the Camera app again, Choose a file instead, or Cancel meanwhile.
- [ ] **(Steve, later)** Camera on Linux (check the Surface's cameras there); maybe a camera view inside
      RevuCraft on Windows too, if going through the Camera app gets in the way.
- [x] **(Steve, build 191)** Phone layout and touch, first pass (Pixel 10a, Galaxy Tab):
  - Narrower than 600 points: the tools panel shows icons only, the side boxes open from the
    bottom, the rulers stay off, the ribbon swipes sideways, Print and Properties leave the top bar.
  - Two fingers move and zoom the page with any tool, about the point between them.
  - A finger dragging the Select tool over empty paper pans (mouse and pen still draw a box).
  - A finger's flick glides on and slows to a stop; a touch stops it.
  - Pen-only drawing waits for a pen: on a phone (or a tablet without its pen) fingers draw.
- [x] **(Steve, build 192)** The file picker on a phone: fits the screen's width, the whole box scrolls
      so Open, Save and Cancel are always reachable, a shorter file list, and the path as buttons
      (every folder on the way goes straight there).
- [x] **(Steve, build 194)** A finger's tap on the tablet (where fingers pan once the S Pen has been used)
      opens a photo pin's photo and selects a markup again, as the pen and mouse do.
- [x] **(Steve, build 196)** The New dialog on a phone: the screen's width, its sections one under
      another, the whole box scrolls. Android's app info shows "build 196" rather than 0.13.0.
- [ ] **(Steve, next)** Touch, second pass from use on the devices: double tap to zoom, the drag
      threshold, the glide's feel, long press; quick Add photo button and field flow on the phone;
      Properties, caption box and keypad sized for the phone.
- [ ] **(Steve, build 200: build 1 of 2)** Markdown notes, placed like photo pins. Each note is a real `.md` file in a
      project folder named after the PDF (`Smith Residence.pdf` → `Smith Residence/notes/`, photos
      at full size in `Smith Residence/photos/`), linked relative to the PDF like drawings and
      workbooks, so Obsidian can open the folder as (part of) a vault and edits there show in
      RevuCraft (Links box: Modified). The PDF keeps a copy (the note's text in its sticky note,
      the photo scaled down) so it still stands alone when sent; the files are the master copy.
      Rename job (PDF and folder together) and Package job (zip of both).
      Toolbar after Obsidian's Editing Toolbar: undo, redo, format painter, clear formatting, H2,
      H3, Hn, bold, italic, strikethrough, code, highlight, links (web and `[[markup]]` on the
      plan), file link (paperclip), checklist, format menu, lists. No underline or colours (HTML in
      Markdown shows raw elsewhere), no tables or callouts yet.
  - [x] Build 1 (build 200): project folder, Note tool (Photos group, the page-with-`#` icon),
    full-screen editor (the toolbar scrolls sideways on a phone), reading view first, tap Edit to
    write, tickable checklists, `#tags` in a Tags column of the Markups list (and "2 of 5 done"),
    date and author stamp, `[[links]]` to markups (by first line, note title, symbol or
    measurement label). The PDF must be saved first (the folder sits beside it). The PDF's copy
    of the text is refreshed when the note is saved in RevuCraft; an edit made in Obsidian shows
    when the note is next opened.
  - Build 2: paperclip links to `.xlsx` (a live read-only preview of a sheet or range), templates,
    notes in the summary report, Rename job and Package job. Photos stay separate pins a note
    links to.
  - Later: voice notes.
- [ ] **(Steve, back burner)** Object styles, as Illustrator's graphic styles: save a markup's look (stroke,
      width, dash, line ends, alignment, fill, opacity, font) under a name ("Existing walls", "Demo")
      in a Styles box; apply one to the selection or make it the current tool style. A text style
      sets a whole text box's font, size, colour and alignment. Styles are kept in RevuCraft's
      settings (every drawing), can be exported/imported as a file, and the ones used travel in the
      PDF. Based on: a style can follow another and change only what differs.
      Reuse VectorCraft's graphic styles and style libraries (its Graphic Styles panel and library
      files), mapped onto markup properties.
- [ ] **(Steve, queued)** Bluebeam Revu compatibility, so a PDF can go back and forth with a team on
      Revu (Studio Sessions/Projects, Bluebeam's own cloud service, stay out of reach):
  - [ ] Round trip: Revu-marked PDF → open, add and change markups in RevuCraft → reopen in Revu.
        Check looks, Revu's markups intact, ours in its Markups list (subject, author, layer,
        measurements and their scale). Synthetic test files only; Steve runs the Revu half once Revu is
        installed. Fixed findings get synthetic regression tests.
  - [ ] Keep, and show in the Markups list, what Revu stores on markups beyond the standard
        (custom columns, statuses, subjects), learned from Revu's public documentation and from files
        it writes; edits leave it intact.
  - [ ] Import Revu tool sets (.btx) into the Tool Chest, so company-standard markups match.
  - [ ] Check Revu's view of RevuCraft's extras (hairlines, photo callouts, drafted shapes, nested
        layers) and degrade gracefully where it differs.
- [ ] **(Steve, queued after Revu compatibility)** Live sessions, as Studio Sessions but self-hosted:
      a small Rust session server (its own app) on Steve's home server, reached over Tailscale
      (guests get the server node shared with them; HTTPS from Tailscale). RevuCraft
      connects only once a server address is set in Settings (off by default). Not interoperable
      with Bluebeam Studio.
  - [ ] Build 1: the server; start a session (the PDF uploaded once), join by link or code;
        markup changes (add, move, restyle, delete, matched by each markup's `/NM`) synced
        both ways, last change to a markup wins.
  - [ ] Build 2: who is in and on which page, their pointers; a markup being dragged is held for
        that person; offline changes queue and catch up.
  - [ ] Build 3: invite tokens per session, End session (everyone's markups saved into the
        PDF), a session report of who changed what, Android polish.
- [ ] **(Steve, back burner)** Paragraph and character styles inside text boxes, as InDesign: a text
      box holds paragraphs with their own styles (Header, Body); Next style (Enter after a Header
      starts a Body paragraph); character styles for runs of text; nested styles (bold up to the
      first colon, then the paragraph's style). Needs rich text editing in text boxes and their
      appearance (today a text box has one font, size and colour).
      Reuse DesignCraft's style model (designcraft-doc: paragraph and character styles with
      Based on, Next style, nested and nested line styles, and how they resolve) as a pinned git
      dependency, as GridCraft is; its composer only if its bundled fonts can come from craft-fonts.
- [x] **(Steve, build 157)** While drafting (and with Format copy), the status bar shows the width of the line under the
      pointer ("PDF line 0.35 mm (1.0 pt)").
- [x] **(Steve, build 157)** A default line width per layer ("ByLayer"; set from the layer's colour dot): lines drawn on the layer take
      it unless the ribbon's width is changed.
- [ ] **(Steve, back burner)** Select existing PDF linework (click, window or crossing) to delete, restyle,
      or Explode into editable lines (brings forward the Explode item below).
- [x] **(Steve, build 174)** Folder mappings per device (`M:\Jobs` on the PC = `Sync/Jobs` on the
      tablet), for drawings kept outside the PDF's folder. Preferences ▸ Folder mappings; linked
      drawings and workbooks are looked for through them (the Links pane says so).

Tablet
- [ ] **(Steve, later update)** Touch mode: bigger buttons and handles, long-press for the context menu, two-finger tap to
      undo.
- [ ] **(Steve, later update)** Touch mode: a typed length alone has a direction on a tablet. When the
      last touch is on (or next to) the last point, the line carries on the way the previous line went
      (the first line goes right; Ortho squares it as usual). A long-press elsewhere on the drawing aims
      without placing a point, and the rubber band shows the direction before Enter. Mouse and pen
      hover keep aiming at the pointer.

Builds
- [x] **(Steve)** One build number shared by Android, Windows and Linux (from build 36: commits
      on the branch, so one commit has one number everywhere).

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
- [ ] Tools (line, polyline, spline, rectangle, circle, arc, ellipse, polygon, revision cloud, walls, doors and windows (cut into the wall at its thickness) and aligned, linear, radius and diameter dimensions done): line, polyline, rectangle, circle and arc, walls with a thickness, doors and windows
      from the symbol library; object snaps (end, mid, intersection, perpendicular) shared with
      measuring.
- [x] Editing: grips, move, copy, rotate, offset, trim and extend; dimensions (from the
      measuring tools); text. Trim and extend work on lines and polylines against other markup
      (not yet against the PDF's own drawing); boxes and symbols rotate about the centre point
      but stay upright.
- [x] Draft tools panel on the left (CADCraft's Tool Sets): Draw, Modify and Annotate groups with
      drafting icons; collapses to one icon per group, the arrow on a button flies out the rest.
- [x] More CAD tools: mirror, scale, rectangular and polar arrays, fillet (sharp or rounded),
      chamfer, break, join, explode, erase, callout; the ribbon shows the tool's options.
- [x] Cursor coordinates in feet-inches in the status bar; a page origin to measure from.
- [ ] Everything drawn stays editable, on layers, and saved in the PDF (annotations with their
      geometry in drawing units).

DWG/DXF round trip
- [x] **(Steve, build 82)** Open a DWG or DXF like a PDF (New dialog, Recent, drag and drop):
      each layout (title-block sheet) a page in tab order at 1:1, model space fitted to a sheet
      when there are no layouts; CAD layers as PDF layers; every viewport's scale as a
      measurement viewport (1/8" = 1'-0", 1:100), so measuring needs no calibrating. Through
      CADCraft's reader and plotter (cadcraft-io).
- [ ] Export: drawn geometry, layers (names, colours), symbols as blocks with their labels,
      text and dimensions to DXF and DWG, at real-world size (drawing units from the page
      scale).
- [x] **(Steve, build 93)** Linked drawings, as InDesign links (mock-up approved): a sheet opened from a DWG/DXF
      remembers its file, layout, date and fingerprint, with the plotted drawing as the backdrop
      under the markups (and a copy kept in the PDF). Place linked drawing puts one under an
      existing PDF sheet, lined up once by two points. A Links pane (right rail) lists each link:
      Up to date / Modified / Missing, with Update, Update all, Edit original (opens CADCraft on
      that layout), Relink, Reveal file; a badge on its icon when one is out of date. Edits
      started with Edit original update the sheet automatically when CADCraft saves; changes made
      elsewhere (the server, Syncthing) only show as Modified until Update. Markups, photos and
      counts stay put; viewport scales follow. Option (off by default): keep each previous
      drawing as a hidden Rev layer. Files live on Steve's server and in Syncthing folders for
      now; other storage is for the public release.
      Several linked drawings on one sheet (a suite pieced together from one DWG per room, like
      xrefs): each placed from model space at the sheet's scale on its own layer named after the
      file, placed automatically (by the drawings' own coordinates, else beside the last one),
      then movable by dragging with snapping to the other drawings' walls and corners; the
      position is remembered for every update. Updating one room redraws only that piece; the
      Links pane lists one row per placed drawing.
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
