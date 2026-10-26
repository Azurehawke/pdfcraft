# Handover: RevuCraft's phone and touch work, for PdfCraft for Android

Status: open, written 2026-10-26 from the RevuCraft session for the PdfCraft for Android session.
Revision history: 2026-10-26 first version (RevuCraft builds 191 and 192); 2026-10-26 the touch
part confirmed on the Galaxy Tab and ready to port, with build 194's finger-tap fix (also
confirmed).

Steve wants these RevuCraft changes in PdfCraft for Android (the `android` branch) eventually.
They were made on the `revucraft` branch of Azurehawke/pdfcraft. Port them there; don't merge
`revucraft` into `android` (it carries all of RevuCraft). Read the commits with
`git fetch origin revucraft && git show <commit>`.

## What to port

| Change | Commit | Files |
|---|---|---|
| Phone layout: narrower than 600 points, the tools panel shows icons only, the side boxes (dock) open from the bottom at full width, the rulers stay off, Print and Properties leave the top bar | `599f500` | `crates/ui-egui/src/chrome.rs` (`phone()`, `PHONE_WIDTH`), `draft_panel.rs`, `dock.rs`, `canvas.rs` (`document_area`) |
| Touch: two fingers pan and zoom with any tool, about the point between them; a finger dragging the Select tool over empty paper pans (mouse and pen still draw a selection box); a finger's flick glides on | `599f500` | `canvas.rs` (pinch, `pans`, `touch_panning`), `comments.rs` (`finger_pan`), `fling.rs` (`drag`, `release`, test) |
| Pen-only drawing waits for a pen: until an S Pen touches the screen, fingers draw and place things (a phone has no pen); afterwards fingers pan as before | `599f500` | `pen.rs` (`PEN_SEEN`, `pen_only_enabled`, `finger`), `apps/pdfcraft-android/src/tablet.rs` (`route` checks `pen_only_enabled`) |
| A finger's tap while it pans (pen-only drawing gives it the Hand tool): a photo pin opens its photo and a markup is selected, as the Select tool's tap would (build 191 left the tap doing nothing but a flash of the hover popup) | `2f88572` | `crates/ui-egui/src/comments.rs` (`page_input`, before `match cx.tool`) |
| The in-app file picker fits a phone: the box fits the screen's width and scrolls as a whole (Open, Save and Cancel always reachable), a shorter file list on a phone, and the path as buttons (every folder on the way goes straight there) | `0e9edd7` | `crates/ui-egui/src/android_picker.rs` (`contents`, the breadcrumb) |

## What doesn't carry over

- The ribbon (`ribbon.rs`) is RevuCraft's; PdfCraft has none, so "the ribbon swipes sideways"
  doesn't apply.
- The Undo and Redo buttons in the top bar are RevuCraft's (`app.show_ribbon`); the icon count
  in `chrome.rs`'s mode bar counts them only there.
- RevuCraft's camera option in the picker (`Ask::Camera`, `show_camera`) is RevuCraft's photo
  pins; PdfCraft's Scan with camera has its own camera bridge (`pcTakePhoto ()Z`), which is
  RevuCraft's next step to adopt, not the other way round.

## Ready to port

Steve tried the touch changes on the Galaxy Tab (build 192) and navigation is much better: port
them now, with the build 194 tap fix. The numbers (drag threshold, the glide's slowing) may still
be tuned from use on the Pixel 10a; any later change will be listed here.

## Checks

- Phone-sized screenshots: `shot phone "$plan" --size 430x900 --scale 2`, as in
  `.github/workflows/revucraft-checks.yml` (also `phone-draft`, `phone-dock`).
- `fling::tests::a_finger_flick_on_the_screen_glides_and_a_stopped_finger_does_not`.
- The picker is Android-only (`#[cfg(target_os = "android")]`): it builds only in the APK job.
