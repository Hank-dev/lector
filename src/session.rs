use std::collections::{HashMap, HashSet};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use serde::{Deserialize, Serialize};

use crate::view::{LayoutMode, Zoom};

const MAX_FILES: usize = 200;
static NEXT_TEMP_FILE: AtomicUsize = AtomicUsize::new(0);

#[derive(Clone, Default, Serialize, Deserialize)]
pub struct Session {
    pub files: HashMap<String, FileState>,
    #[serde(default)]
    pub recent: Vec<String>,
    #[serde(default)]
    history_generation: u64,
    pub sidebar: String,
    pub invert: bool,
    #[serde(default)]
    pub pdf_mode: String,
    pub layout: String,
    #[serde(skip)]
    dirty_files: HashSet<String>,
    #[serde(skip)]
    history_cleared: bool,
    #[serde(skip)]
    loaded_history_generation: u64,
    #[serde(skip)]
    suppressed_history_path: Option<String>,
    #[serde(skip)]
    loaded_sidebar: String,
    #[serde(skip)]
    loaded_invert: bool,
    #[serde(skip)]
    loaded_pdf_mode: String,
    #[serde(skip)]
    loaded_layout: String,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct FileState {
    pub page: i32,
    pub zoom: String,
    pub custom: f64,
    #[serde(default)]
    pub rotation: i32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HistoryEntry {
    pub path: PathBuf,
    pub basename: String,
    pub page: i32,
    pub exists: bool,
}

impl Default for FileState {
    fn default() -> Self {
        Self {
            page: 0,
            zoom: "fit-width".into(),
            custom: 1.0,
            rotation: 0,
        }
    }
}

impl Session {
    pub fn load() -> Self {
        Self::load_from_paths(&session_path(), &legacy_session_path())
    }

    fn load_from_paths(path: &Path, legacy_path: &Path) -> Self {
        if path.exists() {
            return Self::load_from_path(path);
        }
        Self::load_from_path(legacy_path)
    }

    pub fn load_from_path(path: &Path) -> Self {
        let Ok(text) = fs::read_to_string(path) else {
            return Self::default();
        };
        let mut session: Self = toml::from_str(&text).unwrap_or_default();
        session.normalize();
        session.mark_clean();
        session
    }

    pub fn save(&mut self) -> Result<(), String> {
        self.save_to(&session_path())
    }

    pub fn save_to(&mut self, path: &Path) -> Result<(), String> {
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir)
                .map_err(|e| format!("create session directory {}: {e}", dir.display()))?;
        }
        let lock_path = path.with_extension("toml.lock");
        let lock = open_lock_file(&lock_path)?;
        lock.lock()
            .map_err(|e| format!("lock session {}: {e}", lock_path.display()))?;

        let mut merged = Self::load_from_path(path);
        merged.merge_dirty(self);
        let text = toml::to_string_pretty(&merged)
            .map_err(|e| format!("serialize session {}: {e}", path.display()))?;
        write_private_atomic(path, text.as_bytes())?;
        *self = merged;
        self.mark_clean();
        Ok(())
    }

    fn merge_dirty(&mut self, dirty: &Session) {
        if dirty.history_cleared && dirty.history_generation >= self.history_generation {
            self.files.clear();
            self.recent.clear();
            self.history_generation = dirty.history_generation;
        }
        // Replay surviving updates in reading order. Local retention eviction
        // is not an explicit deletion of another window's newer state.
        if dirty.loaded_history_generation >= self.history_generation {
            for key in dirty
                .recent
                .iter()
                .filter(|key| dirty.dirty_files.contains(*key))
            {
                if let Some(state) = dirty.files.get(key) {
                    self.files.insert(key.clone(), state.clone());
                    self.recent.retain(|recent| recent != key);
                    self.recent.push(key.clone());
                }
            }
        }
        self.suppressed_history_path = dirty.suppressed_history_path.clone();
        if dirty.sidebar != dirty.loaded_sidebar {
            self.sidebar = dirty.sidebar.clone();
        }
        if dirty.invert != dirty.loaded_invert {
            self.invert = dirty.invert;
        }
        if dirty.pdf_mode != dirty.loaded_pdf_mode {
            self.pdf_mode = dirty.pdf_mode.clone();
        }
        if dirty.layout != dirty.loaded_layout {
            self.layout = dirty.layout.clone();
        }
        self.normalize();
    }

    fn mark_clean(&mut self) {
        self.dirty_files.clear();
        self.history_cleared = false;
        self.loaded_history_generation = self.history_generation;
        self.loaded_sidebar = self.sidebar.clone();
        self.loaded_invert = self.invert;
        self.loaded_pdf_mode = self.pdf_mode.clone();
        self.loaded_layout = self.layout.clone();
    }

    fn normalize(&mut self) {
        for state in self.files.values_mut() {
            state.rotation = state.rotation.rem_euclid(360) / 90 * 90;
        }
        let mut seen = HashSet::new();
        let mut recent = self
            .recent
            .iter()
            .rev()
            .filter(|key| self.files.contains_key(*key) && seen.insert((*key).clone()))
            .cloned()
            .collect::<Vec<_>>();
        recent.reverse();
        for key in self.files.keys() {
            if seen.insert(key.clone()) {
                recent.push(key.clone());
            }
        }
        self.recent = recent;
        while self.recent.len() > MAX_FILES {
            let old = self.recent.remove(0);
            self.files.remove(&old);
        }
    }

    #[cfg(test)]
    pub fn remember(&mut self, path: &Path, page: i32, zoom: Zoom) {
        self.remember_with_rotation(path, page, zoom, 0);
    }

    pub fn remember_with_rotation(&mut self, path: &Path, page: i32, zoom: Zoom, rotation: i32) {
        let key = path.to_string_lossy().into_owned();
        if self.suppressed_history_path.as_deref() == Some(key.as_str()) {
            return;
        }
        self.suppressed_history_path = None;
        self.files.insert(
            key.clone(),
            FileState {
                page,
                zoom: zoom_key(zoom),
                custom: match zoom {
                    Zoom::Custom(s) => s,
                    _ => 1.0,
                },
                rotation: rotation.rem_euclid(360) / 90 * 90,
            },
        );
        self.dirty_files.insert(key.clone());
        self.recent.retain(|k| k != &key);
        self.recent.push(key);
        self.normalize();
    }

    pub fn restore(&self, path: &Path) -> Option<FileState> {
        self.files.get(path.to_string_lossy().as_ref()).cloned()
    }

    pub fn history(&self, limit: usize) -> Vec<HistoryEntry> {
        self.recent
            .iter()
            .rev()
            .filter_map(|key| {
                let state = self.files.get(key)?;
                let path = PathBuf::from(key);
                let basename = path
                    .file_name()
                    .map(|name| name.to_string_lossy().into_owned())
                    .filter(|name| !name.is_empty())
                    .unwrap_or_else(|| key.clone());
                Some(HistoryEntry {
                    exists: path.is_file(),
                    path,
                    basename,
                    page: state.page.saturating_add(1),
                })
            })
            .take(limit)
            .collect()
    }

    pub fn clear_history(&mut self) {
        self.files.clear();
        self.recent.clear();
        self.dirty_files.clear();
        self.history_generation = self
            .history_generation
            .max(self.loaded_history_generation)
            .saturating_add(1);
        self.history_cleared = true;
    }

    pub fn clear_history_for_open_document(&mut self, open_path: Option<&Path>) {
        self.clear_history();
        self.suppressed_history_path = open_path.map(|path| path.to_string_lossy().into_owned());
    }
}

pub fn parse_zoom(s: &str, custom: f64) -> Zoom {
    match s {
        "fit-page" => Zoom::FitPage,
        "fit-height" => Zoom::FitHeight,
        "actual" => Zoom::Actual,
        "custom" => Zoom::Custom(custom.max(0.1)),
        _ => Zoom::FitWidth,
    }
}

pub fn parse_layout(s: &str) -> LayoutMode {
    match s {
        "single" => LayoutMode::Single,
        "dual" => LayoutMode::Dual,
        _ => LayoutMode::Continuous,
    }
}

fn zoom_key(z: Zoom) -> String {
    match z {
        Zoom::FitWidth => "fit-width".into(),
        Zoom::FitPage => "fit-page".into(),
        Zoom::FitHeight => "fit-height".into(),
        Zoom::Actual => "actual".into(),
        Zoom::Custom(_) => "custom".into(),
    }
}

fn session_path() -> PathBuf {
    app_session_path("lector")
}

fn legacy_session_path() -> PathBuf {
    app_session_path("omadoc")
}

fn app_session_path(app_name: &str) -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".into());
    PathBuf::from(home)
        .join(".config")
        .join(app_name)
        .join("session.toml")
}

fn open_lock_file(path: &Path) -> Result<fs::File, String> {
    let mut options = OpenOptions::new();
    options.read(true).write(true).create(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    options
        .open(path)
        .map_err(|e| format!("open session lock {}: {e}", path.display()))
}

fn write_private_atomic(path: &Path, contents: &[u8]) -> Result<(), String> {
    let dir = path
        .parent()
        .ok_or_else(|| format!("session path {} has no parent", path.display()))?;
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("session.toml");
    let mut created = None;
    for _ in 0..128 {
        let id = NEXT_TEMP_FILE.fetch_add(1, Ordering::Relaxed);
        let temp = dir.join(format!(".{name}.{}-{id}.tmp", std::process::id()));
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        match options.open(&temp) {
            Ok(file) => {
                created = Some((temp, file));
                break;
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(format!("create temporary session file: {e}")),
        }
    }
    let Some((temp, mut file)) = created else {
        return Err("could not create a unique temporary session file".into());
    };
    let result = (|| {
        file.write_all(contents)
            .map_err(|e| format!("write temporary session {}: {e}", temp.display()))?;
        file.sync_all()
            .map_err(|e| format!("sync temporary session {}: {e}", temp.display()))?;
        drop(file);
        fs::rename(&temp, path).map_err(|e| format!("replace session {}: {e}", path.display()))
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temp);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    static NEXT_TEST_DIR: AtomicUsize = AtomicUsize::new(0);

    fn test_path(name: &str) -> PathBuf {
        let id = NEXT_TEST_DIR.fetch_add(1, Ordering::Relaxed);
        let dir =
            std::env::temp_dir().join(format!("lector-session-{name}-{}-{id}", std::process::id()));
        fs::create_dir_all(&dir).expect("create test session directory");
        dir.join("session.toml")
    }

    fn remembered(page: i32) -> Session {
        let mut session = Session::default();
        session.remember(Path::new("/documents/a.pdf"), page, Zoom::Actual);
        session
    }

    #[test]
    fn stale_sessions_merge_different_dirty_files() {
        let path = test_path("merge");
        let mut first = Session::load_from_path(&path);
        let mut second = Session::load_from_path(&path);
        first.remember(Path::new("/documents/first.pdf"), 3, Zoom::Actual);
        second.remember(Path::new("/documents/second.pdf"), 7, Zoom::FitPage);

        first.save_to(&path).expect("save first session");
        second.save_to(&path).expect("merge second session");

        let merged = Session::load_from_path(&path);
        assert_eq!(
            merged
                .restore(Path::new("/documents/first.pdf"))
                .unwrap()
                .page,
            3
        );
        assert_eq!(
            merged
                .restore(Path::new("/documents/second.pdf"))
                .unwrap()
                .page,
            7
        );
        fs::remove_dir_all(path.parent().unwrap()).expect("remove test session directory");
    }

    #[test]
    fn stale_non_dirty_state_does_not_replace_a_newer_file_state() {
        let path = test_path("same-file");
        let mut initial = remembered(2);
        initial.save_to(&path).expect("save initial state");

        let mut stale = Session::load_from_path(&path);
        let mut newer = Session::load_from_path(&path);
        newer.remember(Path::new("/documents/a.pdf"), 9, Zoom::Custom(1.5));
        newer.save_to(&path).expect("save newer state");
        stale.remember(Path::new("/documents/b.pdf"), 5, Zoom::FitHeight);
        stale.save_to(&path).expect("merge stale state");

        let merged = Session::load_from_path(&path);
        assert_eq!(
            merged.restore(Path::new("/documents/a.pdf")).unwrap().page,
            9
        );
        assert_eq!(
            merged.restore(Path::new("/documents/b.pdf")).unwrap().page,
            5
        );
        fs::remove_dir_all(path.parent().unwrap()).expect("remove test session directory");
    }

    #[test]
    fn concurrent_writers_keep_each_dirty_file() {
        let path = test_path("concurrent");
        let barrier = std::sync::Arc::new(std::sync::Barrier::new(2));
        std::thread::scope(|scope| {
            for (name, page) in [("left.pdf", 11), ("right.pdf", 12)] {
                let path = path.clone();
                let barrier = barrier.clone();
                scope.spawn(move || {
                    let mut session = Session::load_from_path(&path);
                    session.remember(Path::new(name), page, Zoom::Actual);
                    barrier.wait();
                    session.save_to(&path).expect("concurrent session save");
                });
            }
        });

        let merged = Session::load_from_path(&path);
        assert_eq!(merged.restore(Path::new("left.pdf")).unwrap().page, 11);
        assert_eq!(merged.restore(Path::new("right.pdf")).unwrap().page, 12);
        fs::remove_dir_all(path.parent().unwrap()).expect("remove test session directory");
    }

    #[test]
    fn renamed_session_loads_legacy_state_until_lector_state_exists() {
        let new_path = test_path("lector-state");
        let legacy_path = test_path("legacy-state");
        let mut legacy = remembered(3);
        legacy.save_to(&legacy_path).unwrap();
        assert_eq!(
            Session::load_from_paths(&new_path, &legacy_path)
                .restore(Path::new("/documents/a.pdf"))
                .unwrap()
                .page,
            3
        );
        let mut current = remembered(7);
        current.save_to(&new_path).unwrap();
        assert_eq!(
            Session::load_from_paths(&new_path, &legacy_path)
                .restore(Path::new("/documents/a.pdf"))
                .unwrap()
                .page,
            7
        );
        fs::remove_dir_all(new_path.parent().unwrap()).unwrap();
        fs::remove_dir_all(legacy_path.parent().unwrap()).unwrap();
    }

    #[test]
    fn renamed_session_paths_use_lector_and_keep_omadoc_as_legacy() {
        assert!(session_path().ends_with(".config/lector/session.toml"));
        assert!(legacy_session_path().ends_with(".config/omadoc/session.toml"));
    }

    #[test]
    fn merged_session_preserves_reading_order_and_evicts_oldest() {
        let path = test_path("reading-order");
        let mut session = Session::default();
        for index in 0..MAX_FILES {
            session.remember(
                Path::new(&format!("/documents/{index}.pdf")),
                0,
                Zoom::Actual,
            );
        }
        let expected = session.recent.clone();
        session.save_to(&path).unwrap();
        assert_eq!(session.recent, expected);
        session.remember(Path::new("/documents/newest.pdf"), 0, Zoom::Actual);
        session.save_to(&path).unwrap();
        assert!(session.restore(Path::new("/documents/0.pdf")).is_none());
        assert!(session
            .restore(Path::new("/documents/newest.pdf"))
            .is_some());
        assert_eq!(session.files.len(), MAX_FILES);
        fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }

    #[test]
    fn loaded_rotation_is_normalized_before_restore() {
        let path = test_path("loaded-rotation");
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(
            &path,
            "sidebar = \"\"\ninvert = false\npdf_mode = \"\"\nlayout = \"\"\nrecent = [\"/documents/a.pdf\"]\nhistory_generation = 0\n\n[files.\"/documents/a.pdf\"]\npage = 0\nzoom = \"actual\"\ncustom = 1.0\nrotation = 450\n",
        )
        .unwrap();

        let session = Session::load_from_path(&path);
        assert_eq!(
            session
                .restore(Path::new("/documents/a.pdf"))
                .unwrap()
                .rotation,
            90
        );

        fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }

    #[test]
    fn per_document_rotation_is_normalized_and_restored() {
        let mut session = Session::default();
        let path = Path::new("/documents/rotated.pdf");

        session.remember_with_rotation(path, 4, Zoom::FitPage, 450);

        let restored = session.restore(path).expect("saved file state");
        assert_eq!(restored.page, 4);
        assert_eq!(restored.rotation, 90);
    }

    #[test]
    fn pdf_mode_is_persisted_and_legacy_invert_remains_readable() {
        let path = test_path("pdf-mode");
        let mut session = Session {
            pdf_mode: "omarchy".into(),
            ..Session::default()
        };
        session.save_to(&path).unwrap();
        assert_eq!(Session::load_from_path(&path).pdf_mode, "omarchy");

        let legacy_path = test_path("legacy-invert");
        fs::write(
            &legacy_path,
            "files = {}\nrecent = []\nsidebar = \"thumbs\"\ninvert = true\nlayout = \"continuous\"\n",
        )
        .unwrap();
        let legacy = Session::load_from_path(&legacy_path);
        assert_eq!(legacy.pdf_mode, "");
        assert!(legacy.invert);

        fs::remove_dir_all(path.parent().unwrap()).unwrap();
        fs::remove_dir_all(legacy_path.parent().unwrap()).unwrap();
    }

    #[test]
    fn stale_preference_updates_merge_only_changed_fields() {
        let path = test_path("preferences");
        let mut first = Session::load_from_path(&path);
        let mut second = Session::load_from_path(&path);
        first.invert = true;
        first.save_to(&path).unwrap();
        second.layout = "dual".into();
        second.save_to(&path).unwrap();
        assert!(Session::load_from_path(&path).invert);
        fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }

    #[test]
    fn history_projection_is_newest_first_capped_and_marks_missing_files() {
        let root = test_path("history-projection")
            .parent()
            .unwrap()
            .to_path_buf();
        let mut session = Session::default();
        for index in 0..25 {
            let path = root.join(format!("book-{index}.pdf"));
            fs::write(&path, b"pdf fixture placeholder").unwrap();
            session.remember(&path, index, Zoom::Actual);
        }
        let missing = root.join("book-23.pdf");
        fs::remove_file(&missing).unwrap();

        let history = session.history(20);
        assert_eq!(history.len(), 20);
        assert_eq!(history[0].basename, "book-24.pdf");
        assert_eq!(history[0].page, 25);
        assert_eq!(history[1].path, missing);
        assert!(!history[1].exists);
        assert_eq!(history[19].basename, "book-5.pdf");
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn clearing_history_persists_without_stale_writer_resurrection() {
        let path = test_path("clear-history");
        let mut initial = Session::default();
        initial.remember(Path::new("/documents/old-a.pdf"), 1, Zoom::Actual);
        initial.remember(Path::new("/documents/old-b.pdf"), 2, Zoom::Actual);
        initial.save_to(&path).unwrap();

        let mut clearer = Session::load_from_path(&path);
        let mut stale = Session::load_from_path(&path);
        stale.remember(Path::new("/documents/new.pdf"), 3, Zoom::Actual);
        clearer.clear_history();
        clearer.save_to(&path).unwrap();
        stale.save_to(&path).unwrap();

        let merged = Session::load_from_path(&path);
        assert!(merged.recent.is_empty());
        assert!(merged.restore(Path::new("/documents/old-a.pdf")).is_none());
        assert!(merged.restore(Path::new("/documents/old-b.pdf")).is_none());
        fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }

    #[test]
    fn clear_epoch_blocks_same_path_updates_from_a_stale_session() {
        let path = test_path("clear-history-same-path");
        let document = Path::new("/documents/already-open.pdf");
        let mut initial = Session::default();
        initial.remember(document, 1, Zoom::Actual);
        initial.save_to(&path).unwrap();

        let mut stale = Session::load_from_path(&path);
        let mut clearer = Session::load_from_path(&path);
        let stale_generation = stale.loaded_history_generation;
        stale.remember(document, 8, Zoom::Custom(1.5));
        clearer.clear_history();
        clearer.save_to(&path).unwrap();
        stale.save_to(&path).unwrap();

        let merged = Session::load_from_path(&path);
        assert!(merged.restore(document).is_none());
        assert!(merged.history_generation > stale_generation);
        fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }

    #[test]
    fn session_loaded_after_clear_can_add_new_history() {
        let path = test_path("post-clear-history");
        let mut initial = remembered(2);
        initial.save_to(&path).unwrap();
        initial.clear_history();
        initial.save_to(&path).unwrap();

        let mut post_clear = Session::load_from_path(&path);
        post_clear.remember(Path::new("/documents/new.pdf"), 4, Zoom::FitPage);
        post_clear.save_to(&path).unwrap();

        let merged = Session::load_from_path(&path);
        assert_eq!(merged.recent, vec!["/documents/new.pdf"]);
        assert_eq!(merged.history_generation, post_clear.history_generation);
        fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }

    #[test]
    fn clearing_while_open_suppresses_reload_and_close_until_a_different_document_opens() {
        let path = test_path("clear-open-document");
        let open = Path::new("/documents/open.pdf");
        let different = Path::new("/documents/different.pdf");
        let mut session = Session::default();
        session.remember(open, 2, Zoom::Actual);
        session.clear_history_for_open_document(Some(open));
        session.save_to(&path).unwrap();

        session.remember(open, 3, Zoom::FitPage); // reload
        session.remember(open, 4, Zoom::Custom(1.2)); // close persistence
        session.save_to(&path).unwrap();
        assert!(Session::load_from_path(&path).restore(open).is_none());

        session.remember(different, 5, Zoom::Actual);
        session.remember(open, 6, Zoom::Actual);
        session.save_to(&path).unwrap();
        let reloaded = Session::load_from_path(&path);
        assert!(reloaded.restore(different).is_some());
        assert!(reloaded.restore(open).is_some());
        fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }

    #[test]
    fn legacy_session_without_history_generation_defaults_to_zero() {
        let legacy = r#"
files = {}
recent = []
sidebar = "thumbs"
invert = false
layout = "continuous"
"#;
        let session: Session = toml::from_str(legacy).unwrap();
        assert_eq!(session.history_generation, 0);
    }

    #[cfg(unix)]
    #[test]
    fn saved_session_is_private() {
        use std::os::unix::fs::PermissionsExt;

        let path = test_path("permissions");
        let mut session = remembered(4);
        session.save_to(&path).expect("save private session");
        assert_eq!(
            fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
        fs::remove_dir_all(path.parent().unwrap()).expect("remove test session directory");
    }
}
