use std::collections::{HashMap, HashSet};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use serde::{Deserialize, Serialize};

use crate::view::{LayoutMode, Zoom};

const MAX_FILES: usize = 200;
static NEXT_TEMP_FILE: AtomicUsize = AtomicUsize::new(0);

#[derive(Default, Serialize, Deserialize)]
pub struct Session {
    pub files: HashMap<String, FileState>,
    #[serde(default)]
    pub recent: Vec<String>,
    pub sidebar: String,
    pub invert: bool,
    pub layout: String,
    #[serde(skip)]
    dirty_files: HashSet<String>,
    #[serde(skip)]
    loaded_sidebar: String,
    #[serde(skip)]
    loaded_invert: bool,
    #[serde(skip)]
    loaded_layout: String,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct FileState {
    pub page: i32,
    pub zoom: String,
    pub custom: f64,
}

impl Default for FileState {
    fn default() -> Self {
        Self {
            page: 0,
            zoom: "fit-width".into(),
            custom: 1.0,
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
        // Replay surviving updates in reading order. Local retention eviction
        // is not an explicit deletion of another window's newer state.
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
        if dirty.sidebar != dirty.loaded_sidebar {
            self.sidebar = dirty.sidebar.clone();
        }
        if dirty.invert != dirty.loaded_invert {
            self.invert = dirty.invert;
        }
        if dirty.layout != dirty.loaded_layout {
            self.layout = dirty.layout.clone();
        }
        self.normalize();
    }

    fn mark_clean(&mut self) {
        self.dirty_files.clear();
        self.loaded_sidebar = self.sidebar.clone();
        self.loaded_invert = self.invert;
        self.loaded_layout = self.layout.clone();
    }

    fn normalize(&mut self) {
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

    pub fn remember(&mut self, path: &Path, page: i32, zoom: Zoom) {
        let key = path.to_string_lossy().into_owned();
        self.files.insert(
            key.clone(),
            FileState {
                page,
                zoom: zoom_key(zoom),
                custom: match zoom {
                    Zoom::Custom(s) => s,
                    _ => 1.0,
                },
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
