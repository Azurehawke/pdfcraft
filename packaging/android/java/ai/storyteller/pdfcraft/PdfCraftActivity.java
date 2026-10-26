package ai.storyteller.pdfcraft;

import android.app.NativeActivity;
import android.app.PendingIntent;
import android.content.ClipData;
import android.content.Intent;
import android.content.pm.PackageInfo;
import android.content.pm.PackageInstaller;
import android.content.pm.PackageManager;
import android.net.Uri;
import android.provider.Settings;

import java.io.ByteArrayOutputStream;
import java.io.File;
import java.io.FileInputStream;
import java.io.FileOutputStream;
import java.io.IOException;
import java.io.InputStream;
import java.io.OutputStream;
import java.net.HttpURLConnection;
import java.net.URL;
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
 * - Updates (apps/pdfcraft-android/src/updater.rs): HTTP for the release listing and the APK
 *   download, and installing the downloaded APK through PackageInstaller. Android answers an
 *   install with an intent back to this activity: either "ask the user" (the system's install
 *   confirmation, started here) or the outcome, kept for the Rust side to collect.
 */
public class PdfCraftActivity extends NativeActivity {
    private static final int REQUEST_DOCUMENTS = 0x5043;
    private static final String INSTALL_STATUS = "ai.storyteller.pdfcraft.INSTALL_STATUS";
    private static final String USER_AGENT = "PdfCraft-Android-updater";

    private volatile long downloaded;
    private volatile long downloadTotal;
    private volatile String installStatus;

    private final Object lock = new Object();
    private String[] picked;
    private boolean done;

    @Override
    protected void onNewIntent(Intent intent) {
        super.onNewIntent(intent);
        if (intent != null && INSTALL_STATUS.equals(intent.getAction())) {
            installAnswered(intent);
            return;
        }
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

    /** This app's versionCode (the CI build number). */
    public long pcVersionCode() throws PackageManager.NameNotFoundException {
        PackageInfo info = getPackageManager().getPackageInfo(getPackageName(), 0);
        return info.getLongVersionCode();
    }

    /** Send a GET for `url` (with `accept` as its Accept header when not null) and check its status. */
    private HttpURLConnection open(String url, String accept) throws IOException {
        HttpURLConnection c = (HttpURLConnection) new URL(url).openConnection();
        // Headers go before the request is sent: getResponseCode() below sends it.
        c.setRequestProperty("User-Agent", USER_AGENT);
        if (accept != null) {
            c.setRequestProperty("Accept", accept);
        }
        c.setConnectTimeout(15000);
        c.setReadTimeout(30000);
        c.setInstanceFollowRedirects(true);
        int code = c.getResponseCode();
        if (code / 100 != 2) {
            c.disconnect();
            throw new IOException("HTTP " + code + " from " + new URL(url).getHost());
        }
        return c;
    }

    /** GET `url` as text (the GitHub release listing). Blocking: call off the UI thread. */
    public String pcHttpGet(String url) throws IOException {
        HttpURLConnection c = open(url, "application/vnd.github+json");
        try (InputStream in = c.getInputStream()) {
            ByteArrayOutputStream out = new ByteArrayOutputStream();
            byte[] buf = new byte[16384];
            int n;
            while ((n = in.read(buf)) >= 0) {
                out.write(buf, 0, n);
                if (out.size() > (8 << 20)) {
                    throw new IOException("answer too large");
                }
            }
            return out.toString("UTF-8");
        } finally {
            c.disconnect();
        }
    }

    private File updateFile() {
        return new File(getCacheDir(), "update.apk");
    }

    /** Download `url` to the app's cache as the update APK. Blocking: call off the UI thread. */
    public void pcDownload(String url) throws IOException {
        downloaded = 0;
        downloadTotal = 0;
        File tmp = new File(getCacheDir(), "update.apk.part");
        HttpURLConnection c = open(url, null);
        downloadTotal = Math.max(0, c.getContentLengthLong());
        try (InputStream in = c.getInputStream(); OutputStream out = new FileOutputStream(tmp)) {
            byte[] buf = new byte[65536];
            int n;
            while ((n = in.read(buf)) >= 0) {
                out.write(buf, 0, n);
                downloaded += n;
            }
        } finally {
            c.disconnect();
        }
        File apk = updateFile();
        apk.delete();
        if (!tmp.renameTo(apk)) {
            throw new IOException("couldn't keep the download");
        }
    }

    /** Bytes downloaded so far. */
    public long pcDownloaded() {
        return downloaded;
    }

    /** The download's size (0 when the server didn't say). */
    public long pcDownloadTotal() {
        return downloadTotal;
    }

    /**
     * Install the downloaded APK. Returns false (and opens the setting) when PdfCraft may not
     * install apps yet; Android then asks the user to confirm the install itself.
     */
    public boolean pcInstall() throws IOException {
        if (!getPackageManager().canRequestPackageInstalls()) {
            Intent s = new Intent(Settings.ACTION_MANAGE_UNKNOWN_APP_SOURCES, Uri.parse("package:" + getPackageName()));
            s.addFlags(Intent.FLAG_ACTIVITY_NEW_TASK);
            startActivity(s);
            return false;
        }
        installStatus = null;
        File apk = updateFile();
        PackageInstaller installer = getPackageManager().getPackageInstaller();
        PackageInstaller.SessionParams params = new PackageInstaller.SessionParams(PackageInstaller.SessionParams.MODE_FULL_INSTALL);
        params.setAppPackageName(getPackageName());
        params.setSize(apk.length());
        int id = installer.createSession(params);
        PackageInstaller.Session session = installer.openSession(id);
        try {
            try (InputStream in = new FileInputStream(apk); OutputStream out = session.openWrite("pdfcraft.apk", 0, apk.length())) {
                byte[] buf = new byte[65536];
                int n;
                while ((n = in.read(buf)) >= 0) {
                    out.write(buf, 0, n);
                }
                session.fsync(out);
            }
            Intent status = new Intent(this, PdfCraftActivity.class).setAction(INSTALL_STATUS);
            // Mutable: the installer adds the outcome to the intent.
            PendingIntent pending = PendingIntent.getActivity(this, id, status,
                    PendingIntent.FLAG_UPDATE_CURRENT | PendingIntent.FLAG_MUTABLE);
            session.commit(pending.getIntentSender());
        } catch (IOException | RuntimeException e) {
            session.abandon();
            throw e;
        } finally {
            session.close();
        }
        return true;
    }

    private void installAnswered(Intent intent) {
        int status = intent.getIntExtra(PackageInstaller.EXTRA_STATUS, PackageInstaller.STATUS_FAILURE);
        if (status == PackageInstaller.STATUS_PENDING_USER_ACTION) {
            Intent confirm = intent.getParcelableExtra(Intent.EXTRA_INTENT);
            if (confirm != null) {
                confirm.addFlags(Intent.FLAG_ACTIVITY_NEW_TASK);
                startActivity(confirm);
                return;
            }
        }
        if (status == PackageInstaller.STATUS_SUCCESS) {
            installStatus = "ok";
        } else {
            String message = intent.getStringExtra(PackageInstaller.EXTRA_STATUS_MESSAGE);
            installStatus = (status == PackageInstaller.STATUS_FAILURE_ABORTED ? "cancelled: " : "failed: ")
                    + (message == null ? ("status " + status) : message);
        }
        updateFile().delete();
    }

    /** The install's outcome once Android reports it ("ok", "cancelled: …", "failed: …"), once. */
    public String pcTakeInstallStatus() {
        String s = installStatus;
        installStatus = null;
        return s;
    }
}
