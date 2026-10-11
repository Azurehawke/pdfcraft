package ai.storyteller.pdfcraft;

import android.app.NativeActivity;
import android.content.Intent;

/**
 * PdfCraft's activity: Android's NativeActivity (which loads libpdfcraft_android.so) plus one
 * thing it can't do from Rust. The activity is single-task, so a PDF opened from another app
 * while PdfCraft runs arrives here instead of starting a second copy; keeping it as the
 * activity's intent lets the Rust side find it (apps/pdfcraft-android/src/incoming.rs).
 */
public class PdfCraftActivity extends NativeActivity {
    @Override
    protected void onNewIntent(Intent intent) {
        super.onNewIntent(intent);
        setIntent(intent);
    }
}
