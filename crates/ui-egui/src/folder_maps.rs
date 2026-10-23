//! RevuCraft: folder mappings per device. A drawing linked on the PC as `M:\Jobs\Smith\101.dwg`
//! is `/storage/emulated/0/Sync/Jobs/Smith/101.dwg` on the tablet; with `M:\Jobs` mapped to
//! `/storage/emulated/0/Sync/Jobs` (in either order) the tablet finds it, instead of showing the
//! link as Missing. Mappings are this device's own (Preferences), never written into a PDF.

use std::sync::{PoisonError, RwLock};

/// Mappings kept, at most.
pub const MAX_MAPS: usize = 16;
/// Longest folder accepted (from Preferences or settings), in characters.
const MAX_CHARS: usize = 4096;

/// The mappings in use, for finding linked files anywhere in the app.
static MAPS: RwLock<Vec<(String, String)>> = RwLock::new(Vec::new());

/// The folders of `maps` worth keeping: both sides filled in, not too long, at most
/// [`MAX_MAPS`]. Rows still being typed (one side empty) are kept by Preferences, not used.
pub fn usable(maps: &[(String, String)]) -> Vec<(String, String)> {
    maps.iter()
        .map(|(a, b)| (a.trim().to_string(), b.trim().to_string()))
        .filter(|(a, b)| !components(a).is_empty() && !components(b).is_empty() && a.chars().count() <= MAX_CHARS && b.chars().count() <= MAX_CHARS)
        .take(MAX_MAPS)
        .collect()
}

/// Use `maps` from now on.
pub fn set(maps: &[(String, String)]) {
    *MAPS.write().unwrap_or_else(PoisonError::into_inner) = usable(maps);
}

/// The mappings in use.
pub fn current() -> Vec<(String, String)> {
    MAPS.read().unwrap_or_else(PoisonError::into_inner).clone()
}

/// A path's folders and file, whichever separator it was written with.
fn components(path: &str) -> Vec<&str> {
    path.split(['/', '\\']).filter(|c| !c.is_empty()).collect()
}

/// Where `path` is under each mapping that covers it: the rest of the path after one side's
/// folder, put under the other side's. Folder names match in any case (Windows doesn't mind
/// case, and a synced copy keeps the names).
pub fn mapped(path: &str, maps: &[(String, String)]) -> Vec<String> {
    let parts = components(path);
    let mut out = Vec::new();
    for (a, b) in maps {
        for (from, to) in [(a, b), (b, a)] {
            let root = components(from);
            if root.is_empty() || root.len() >= parts.len() {
                continue;
            }
            let covers = root.iter().zip(&parts).all(|(r, p)| r.eq_ignore_ascii_case(p));
            let Some(rest) = parts.get(root.len()..).filter(|_| covers) else { continue };
            // Written the way the target folder is: backslashes for a Windows folder.
            let sep = if to.contains('\\') && !to.contains('/') { "\\" } else { "/" };
            let base = to.trim_end_matches(['/', '\\']);
            let candidate = format!("{base}{sep}{}", rest.join(sep));
            if !out.contains(&candidate) {
                out.push(candidate);
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_path_moves_between_mapped_folders_either_way() {
        let maps = vec![("M:\\Jobs".to_string(), "/storage/emulated/0/Sync/Jobs/".to_string())];
        assert_eq!(mapped("M:\\Jobs\\Smith\\101.dwg", &maps), ["/storage/emulated/0/Sync/Jobs/Smith/101.dwg"]);
        assert_eq!(mapped("m:/jobs/Smith/101.dwg", &maps), ["/storage/emulated/0/Sync/Jobs/Smith/101.dwg"], "any case, any separator");
        assert_eq!(mapped("/storage/emulated/0/Sync/Jobs/Smith/101.dwg", &maps), ["M:\\Jobs\\Smith\\101.dwg"]);
        // Outside the folders, or the folder itself: nothing.
        assert!(mapped("M:\\Other\\101.dwg", &maps).is_empty());
        assert!(mapped("M:\\Jobs", &maps).is_empty());
        assert!(mapped("M:\\JobsOld\\101.dwg", &maps).is_empty(), "whole folder names only");
        assert!(mapped("", &maps).is_empty());
    }

    #[test]
    fn half_typed_and_oversized_rows_are_not_used() {
        let rows = vec![
            ("M:\\Jobs".to_string(), String::new()),
            ("  ".to_string(), "/x".to_string()),
            ("a".repeat(MAX_CHARS + 1), "/x".to_string()),
            (" C:\\Plans ".to_string(), " /home/steve/Plans ".to_string()),
        ];
        assert_eq!(usable(&rows), [("C:\\Plans".to_string(), "/home/steve/Plans".to_string())]);
        let many: Vec<(String, String)> = (0..40).map(|i| (format!("/a{i}"), format!("/b{i}"))).collect();
        assert_eq!(usable(&many).len(), MAX_MAPS);
    }
}
