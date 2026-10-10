//! "All files access" (MANAGE_EXTERNAL_STORAGE), which the in-app file picker needs to browse
//! shared storage by path. It is a special app access, not a runtime permission: the app can't
//! ask for it with a dialog, only open the Settings screen where the user turns it on. Both calls
//! go through JNI to the activity that started the app.

use jni::JavaVM;
use jni::objects::{JObject, JString, JValue};

/// Run `f` with the JNI environment and the activity. `None` when JNI isn't reachable or `f`
/// failed (a Java exception is cleared and logged, never left pending).
fn with_activity<R>(what: &str, f: impl FnOnce(&mut jni::JNIEnv, &JObject) -> jni::errors::Result<R>) -> Option<R> {
    let ctx = ndk_context::android_context();
    #[allow(unsafe_code)]
    // SAFETY: android-activity fills ndk-context with the process's JavaVM and the activity
    // before `android_main` runs, and both live as long as the process.
    let vm = unsafe { JavaVM::from_raw(ctx.vm().cast()) };
    let vm = match vm {
        Ok(vm) => vm,
        Err(e) => {
            log::warn!("{what}: no JavaVM ({e})");
            return None;
        }
    };
    let mut env = match vm.attach_current_thread() {
        Ok(env) => env,
        Err(e) => {
            log::warn!("{what}: can't attach to the JavaVM ({e})");
            return None;
        }
    };
    #[allow(unsafe_code)]
    // SAFETY: a global reference to the activity, owned by android-activity; wrapping it doesn't
    // take ownership (it is never deleted through this wrapper).
    let activity = unsafe { JObject::from_raw(ctx.context().cast()) };
    let result = f(&mut env, &activity);
    if env.exception_check().unwrap_or(false) {
        let _ = env.exception_describe();
        let _ = env.exception_clear();
    }
    match result {
        Ok(r) => Some(r),
        Err(e) => {
            log::warn!("{what}: {e}");
            None
        }
    }
}

/// Whether the user has given PdfCraft All files access.
pub fn granted() -> bool {
    with_activity("All files access", |env, _| env.call_static_method("android/os/Environment", "isExternalStorageManager", "()Z", &[])?.z())
        .unwrap_or(false)
}

/// Open Settings on PdfCraft's All files access switch.
pub fn request() {
    let opened = with_activity("opening All files access settings", |env, activity| {
        let package = env.call_method(activity, "getPackageName", "()Ljava/lang/String;", &[])?.l()?;
        let package: String = env.get_string(&JString::from(package))?.into();
        let uri_text = env.new_string(format!("package:{package}"))?;
        let uri = env.call_static_method("android/net/Uri", "parse", "(Ljava/lang/String;)Landroid/net/Uri;", &[JValue::Object(&uri_text)])?.l()?;
        let action = env.new_string("android.settings.MANAGE_APP_ALL_FILES_ACCESS_PERMISSION")?;
        let intent =
            env.new_object("android/content/Intent", "(Ljava/lang/String;Landroid/net/Uri;)V", &[JValue::Object(&action), JValue::Object(&uri)])?;
        env.call_method(activity, "startActivity", "(Landroid/content/Intent;)V", &[JValue::Object(&intent)])?;
        Ok(())
    });
    if opened.is_none() {
        // Some devices lack the per-app screen: fall back to the list of all apps.
        let _ = with_activity("opening the All files access list", |env, activity| {
            let action = env.new_string("android.settings.MANAGE_ALL_FILES_ACCESS_PERMISSION")?;
            let intent = env.new_object("android/content/Intent", "(Ljava/lang/String;)V", &[JValue::Object(&action)])?;
            env.call_method(activity, "startActivity", "(Landroid/content/Intent;)V", &[JValue::Object(&intent)])?;
            Ok(())
        });
    }
}
