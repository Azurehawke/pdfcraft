# PdfCraft for Android's changes to winit

This is storytold/winit at `a517b3846ee8457d84ee4975110bfda6797c44dc` (the revision the workspace
pins), with these changes in `src/platform_impl/android/mod.rs` only:

1. Touch events carry the tool type (finger, stylus, eraser, mouse) so the app can tell an S Pen
   from a finger.
2. A paired mouse (`SOURCE_MOUSE`) moves the cursor and reports its buttons and wheel instead of
   arriving as touches.
3. A touchpad's two-finger swipe scrolls and its pinch zooms. Only button-less drags are turned
   into scrolling, so clicks and the cursor still work.

The changes only build on Android. Docs, the changelog and other repository files that the build
doesn't need were left out.
