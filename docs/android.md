# PdfCraft on Android (experimental)

PdfCraft's desktop interface running on Android tablets, meant for use with a keyboard and a
stylus. Built from the `android` branch by `.github/workflows/android.yml` for arm64 devices
running Android 11 or later.

## Install

1. On the tablet, open the repository's **Releases** page and download `pdfcraft.apk` from the
   `android-latest` prerelease.
2. Open the downloaded file. Android asks to allow installs from your browser or Files app the
   first time; allow it, then install. If Play Protect warns about an unknown app, choose
   *More details ▸ Install anyway*.
3. Give PdfCraft file access. It is a special access, not one of the app's regular permissions
   (its Permissions page says it requests none): the first time you open or save a file, the file
   browser shows **Allow access…**, which opens the switch in Settings. Or turn it on yourself:
   **Settings ▸ Apps ▸ ⋮ ▸ Special access ▸ All files access ▸ PdfCraft**. Without it PdfCraft
   can only open and save files in its own app folder.

Later builds install over the earlier one as long as they are signed with the same key.

## Make PdfCraft the default PDF viewer

PdfCraft appears in **Open with** whenever an app opens a PDF (Files, My Files, Gmail, Drive,
Chrome downloads). Choose PdfCraft and **Always** to make it the default. To set or change it
later: **Settings ▸ Apps ▸ Choose default apps** (on Samsung: **Settings ▸ Apps ▸ ⋮ ▸ Default
apps**), or clear another app's default under its own **Set as default ▸ Clear defaults**, so
Android asks again the next time.

A PDF from a file in your storage opens as that file, and Save writes it in place (with All files
access). A PDF that only exists in another app (an email attachment, a Drive file) opens as a
copy: Save asks where to keep it. Opening another PDF while PdfCraft is running adds a tab.

## Google Drive and other apps

Open, Save As and the other file choices show **Google Drive and other apps…** (or **Save to
Google Drive or another app…**) at the top of PdfCraft's file browser. It opens Android's own
document picker, where Google Drive, Dropbox, OneDrive and any other installed document app can
be browsed. PdfCraft works on a copy of the chosen document kept on the tablet, and each time you
save, it writes the copy back to the document and says so ("Saved Report.pdf to Google Drive").
Access lasts, so a document reopened later from Recent Files still saves back.

PdfCraft doesn't notice changes made to the document elsewhere after it fetched it: open it
from Google Drive again to get the latest version.

## How it differs from the desktop app

- **Files:** Open, Save As, Combine and the other file choices use a file browser drawn inside
  PdfCraft (shared storage, Documents, Download, SD cards and USB drives), not Android's own
  picker. Files open and save in place.
- **S Pen first:** only the S Pen (or a mouse) draws, highlights, selects text or places things
  on the page. A finger drag on the page scrolls it, as the Hand tool would (except with Add
  Text, Edit Text and Fill & Sign, where a tap types), and a palm resting on the screen
  is ignored, whether it lands before the pen or while you write. Fingers still work on
  buttons, menus and tabs, and two-finger pinch zooms while the pen is up.
- **Mouse:** a paired Bluetooth or USB mouse works as on a desktop: the pointer follows it
  (hover highlights, tooltips, cursor shapes), right-click opens context menus, the wheel
  scrolls and Ctrl+wheel zooms. A hovering S Pen moves the pointer too.
- **Pens:** the Draw tool's pen button (next to its colour) chooses a pen: Pen, Fountain pen,
  Calligraphy pen, Brush, Pencil or Highlighter. Their width follows how hard you press with the
  S Pen (and, for the calligraphy pen and highlighter, the direction of the stroke). Drawings
  stay standard PDF ink comments: other viewers that show comments show them as drawn.
- **Tabs:** each tab always shows its close button (there is no hover on a touch screen); a dot
  on it marks unsaved changes.
- **Clipboard:** text copy and paste work inside PdfCraft; Create ▸ Clipboard and copying
  snapshots to other apps don't (no system clipboard for images).
- **Scan & OCR:** its two text-recognition models ship inside the APK (about 12 MB, CC BY-SA
  4.0, credited in the app folder's `models/ATTRIBUTION.txt`) and are copied to the app's own
  folder the first time a new version starts.
- **Not on Android:** printing to a printer, the update check, signing with an OS key store, and
  the `--control` automation channel.
- Settings, logs and crash recovery live in the app's private folder; uninstalling removes them.

## Build it yourself

Needs the Android SDK and NDK, `rustup target add aarch64-linux-android` and
`cargo install cargo-ndk`:

```sh
ANDROID_HOME=~/Android/Sdk ANDROID_NDK_HOME=~/Android/Sdk/ndk/<version> packaging/android/build-apk.sh
adb install -r target/android-apk/pdfcraft.apk
adb logcat -s PdfCraft   # the app's log
```

## Signing

The APK is signed with `packaging/android/debug.keystore` (password `android`), which is
public, like Android's own debug key. To use a private key instead, add the repository secrets
`ANDROID_KEYSTORE_B64` (the keystore, base64), `ANDROID_KEYSTORE_PASSWORD` and
`ANDROID_KEY_ALIAS`. Android won't install an update signed with a different key over an
installed app: uninstall the debug-signed one first.
