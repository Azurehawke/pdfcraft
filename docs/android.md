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

## How it differs from the desktop app

- **Files:** Open, Save As, Combine and the other file choices use a file browser drawn inside
  PdfCraft (shared storage, Documents, Download, SD cards and USB drives), not Android's own
  picker. Files open and save in place.
- **Clipboard:** text copy and paste work inside PdfCraft; Create ▸ Clipboard and copying
  snapshots to other apps don't (no system clipboard for images).
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
