package ai.storyteller.pdfcraft;

import android.app.NativeActivity;
import android.content.ClipData;
import android.content.Intent;
import android.net.Uri;

import java.util.ArrayList;

/**
 * PdfCraft's activity: Android's NativeActivity (which loads libpdfcraft_android.so) plus what
 * Rust can't do through JNI alone, because it means overriding an Activity method.
 *
 * - The activity is single-task, so a PDF opened from another app while PdfCraft runs arrives
 *   in onNewIntent instead of starting a second copy; keeping it as the activity's intent lets
 *   the Rust side find it (apps/pdfcraft-android/src/incoming.rs).
 * - Google Drive and other document apps: Android's system picker (the Storage Access
 *   Framework) answers in onActivityResult. The answer is kept for the Rust side to collect
 *   (pcTakePicked), with lasting read and write access to each document
 *   (apps/pdfcraft-android/src/cloud.rs).
 */
public class PdfCraftActivity extends NativeActivity {
    private static final int REQUEST_DOCUMENTS = 0x5043;

    private final Object lock = new Object();
    private String[] picked;
    private boolean done;

    @Override
    protected void onNewIntent(Intent intent) {
        super.onNewIntent(intent);
        setIntent(intent);
    }

    /** Show the system picker for existing documents of these MIME types. */
    public void pcOpenDocuments(final boolean multiple, final String[] mimes) {
        runOnUiThread(new Runnable() {
            @Override
            public void run() {
                Intent i = new Intent(Intent.ACTION_OPEN_DOCUMENT);
                i.addCategory(Intent.CATEGORY_OPENABLE);
                i.setType(mimes.length == 1 ? mimes[0] : "*/*");
                if (mimes.length > 1) {
                    i.putExtra(Intent.EXTRA_MIME_TYPES, mimes);
                }
                i.putExtra(Intent.EXTRA_ALLOW_MULTIPLE, multiple);
                start(i);
            }
        });
    }

    /** Show the system picker for a new document, suggesting its name. */
    public void pcCreateDocument(final String name, final String mime) {
        runOnUiThread(new Runnable() {
            @Override
            public void run() {
                Intent i = new Intent(Intent.ACTION_CREATE_DOCUMENT);
                i.addCategory(Intent.CATEGORY_OPENABLE);
                i.setType(mime);
                i.putExtra(Intent.EXTRA_TITLE, name);
                start(i);
            }
        });
    }

    private void start(Intent i) {
        i.addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION | Intent.FLAG_GRANT_WRITE_URI_PERMISSION
                | Intent.FLAG_GRANT_PERSISTABLE_URI_PERMISSION);
        synchronized (lock) {
            picked = null;
            done = false;
        }
        try {
            startActivityForResult(i, REQUEST_DOCUMENTS);
        } catch (Exception e) {
            // No document picker on this device: answer "nothing chosen".
            synchronized (lock) {
                picked = new String[0];
                done = true;
            }
        }
    }

    @Override
    protected void onActivityResult(int request, int result, Intent data) {
        super.onActivityResult(request, result, data);
        if (request != REQUEST_DOCUMENTS) {
            return;
        }
        ArrayList<String> uris = new ArrayList<>();
        if (result == RESULT_OK && data != null) {
            ClipData clip = data.getClipData();
            if (clip != null) {
                for (int k = 0; k < clip.getItemCount(); k++) {
                    add(uris, clip.getItemAt(k).getUri());
                }
            } else {
                add(uris, data.getData());
            }
        }
        synchronized (lock) {
            picked = uris.toArray(new String[0]);
            done = true;
        }
    }

    /** Keep access to the document after a restart (read and write when the app allows it). */
    private void add(ArrayList<String> uris, Uri uri) {
        if (uri == null) {
            return;
        }
        try {
            getContentResolver().takePersistableUriPermission(uri,
                    Intent.FLAG_GRANT_READ_URI_PERMISSION | Intent.FLAG_GRANT_WRITE_URI_PERMISSION);
        } catch (Exception e) {
            try {
                getContentResolver().takePersistableUriPermission(uri, Intent.FLAG_GRANT_READ_URI_PERMISSION);
            } catch (Exception ignored) {
                // Access lasts while PdfCraft runs.
            }
        }
        uris.add(uri.toString());
    }

    /** The URIs chosen in the system picker (empty when cancelled), or null while it shows. */
    public String[] pcTakePicked() {
        synchronized (lock) {
            if (!done) {
                return null;
            }
            done = false;
            String[] r = picked;
            picked = null;
            return r == null ? new String[0] : r;
        }
    }
}
