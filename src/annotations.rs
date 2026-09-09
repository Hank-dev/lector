use std::collections::{HashMap, HashSet};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct DocumentRect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

pub fn document_rect_to_display(
    rect: DocumentRect,
    page_width: f64,
    page_height: f64,
    rotation: i32,
) -> DocumentRect {
    match rotation.rem_euclid(360) {
        90 => DocumentRect {
            x: page_height - rect.y - rect.height,
            y: rect.x,
            width: rect.height,
            height: rect.width,
        },
        180 => DocumentRect {
            x: page_width - rect.x - rect.width,
            y: page_height - rect.y - rect.height,
            width: rect.width,
            height: rect.height,
        },
        270 => DocumentRect {
            x: rect.y,
            y: page_width - rect.x - rect.width,
            width: rect.height,
            height: rect.width,
        },
        _ => rect,
    }
}

pub fn display_rect_to_document(
    x1: f64,
    y1: f64,
    x2: f64,
    y2: f64,
    page_width: f64,
    page_height: f64,
    rotation: i32,
) -> Option<DocumentRect> {
    if ![x1, y1, x2, y2, page_width, page_height]
        .into_iter()
        .all(f64::is_finite)
        || page_width <= 0.0
        || page_height <= 0.0
    {
        return None;
    }
    let (display_width, display_height) = match rotation.rem_euclid(360) {
        90 | 270 => (page_height, page_width),
        _ => (page_width, page_height),
    };
    let left = x1.min(x2).clamp(0.0, display_width);
    let right = x1.max(x2).clamp(0.0, display_width);
    let top = y1.min(y2).clamp(0.0, display_height);
    let bottom = y1.max(y2).clamp(0.0, display_height);
    if right - left <= 0.5 || bottom - top <= 0.5 {
        return None;
    }
    let to_document = |x: f64, y: f64| match rotation.rem_euclid(360) {
        90 => (y, page_height - x),
        180 => (page_width - x, page_height - y),
        270 => (page_width - y, x),
        _ => (x, y),
    };
    let points = [
        to_document(left, top),
        to_document(right, top),
        to_document(left, bottom),
        to_document(right, bottom),
    ];
    let min_x = points
        .iter()
        .map(|point| point.0)
        .fold(f64::INFINITY, f64::min);
    let max_x = points
        .iter()
        .map(|point| point.0)
        .fold(f64::NEG_INFINITY, f64::max);
    let min_y = points
        .iter()
        .map(|point| point.1)
        .fold(f64::INFINITY, f64::min);
    let max_y = points
        .iter()
        .map(|point| point.1)
        .fold(f64::NEG_INFINITY, f64::max);
    Some(DocumentRect {
        x: min_x,
        y: min_y,
        width: max_x - min_x,
        height: max_y - min_y,
    })
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", tag = "kind")]
pub enum AnnotationKind {
    Highlight,
    Note { text: String },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Annotation {
    pub id: String,
    pub page: i32,
    pub rect: DocumentRect,
    #[serde(flatten)]
    pub kind: AnnotationKind,
}

#[derive(Clone, Default, Serialize, Deserialize)]
pub struct AnnotationStore {
    #[serde(default)]
    documents: HashMap<String, Vec<Annotation>>,
    #[serde(skip)]
    added: HashMap<String, Vec<Annotation>>,
    #[serde(skip)]
    deleted: HashMap<String, HashSet<String>>,
}

static NEXT_TEMP_FILE: AtomicUsize = AtomicUsize::new(0);
static NEXT_ANNOTATION_ID: AtomicUsize = AtomicUsize::new(0);

pub fn new_annotation(page: i32, rect: DocumentRect, kind: AnnotationKind) -> Annotation {
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let sequence = NEXT_ANNOTATION_ID.fetch_add(1, Ordering::Relaxed);
    Annotation {
        id: format!("{}-{timestamp}-{sequence}", std::process::id()),
        page,
        rect,
        kind,
    }
}

impl AnnotationStore {
    pub fn load() -> Result<Self, String> {
        Self::load_from_path(&annotation_path())
    }

    pub fn load_from_path(path: &Path) -> Result<Self, String> {
        let text = match fs::read_to_string(path) {
            Ok(text) => text,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(Self::default())
            }
            Err(error) => return Err(format!("read annotation store {}: {error}", path.display())),
        };
        let store: Self = toml::from_str(&text)
            .map_err(|error| format!("parse annotation store {}: {error}", path.display()))?;
        store.validate()?;
        Ok(store)
    }

    pub fn save(&mut self) -> Result<(), String> {
        self.save_to(&annotation_path())
    }

    pub fn save_to(&mut self, path: &Path) -> Result<(), String> {
        let dir = path
            .parent()
            .ok_or_else(|| format!("annotation path {} has no parent", path.display()))?;
        fs::create_dir_all(dir)
            .map_err(|error| format!("create annotation directory {}: {error}", dir.display()))?;
        let lock_path = path.with_extension("toml.lock");
        let lock = open_lock_file(&lock_path)?;
        lock.lock()
            .map_err(|error| format!("lock annotation store {}: {error}", lock_path.display()))?;

        // Reload only after acquiring the lock. A parse error aborts before
        // replacement, so corrupt user data remains available for recovery.
        let mut merged = Self::load_from_path(path)?;
        merged.merge_changes(self);
        merged.validate()?;
        let text = toml::to_string_pretty(&merged)
            .map_err(|error| format!("serialize annotation store {}: {error}", path.display()))?;
        write_private_atomic(path, text.as_bytes())?;
        *self = merged;
        Ok(())
    }

    pub fn add(&mut self, path: &Path, annotation: Annotation) {
        let key = path.to_string_lossy().into_owned();
        let id = annotation.id.clone();
        let annotations = self.documents.entry(key.clone()).or_default();
        annotations.retain(|existing| existing.id != id);
        annotations.push(annotation.clone());
        let added = self.added.entry(key.clone()).or_default();
        added.retain(|existing| existing.id != id);
        added.push(annotation);
        if let Some(deleted) = self.deleted.get_mut(&key) {
            deleted.remove(&id);
        }
    }

    pub fn remove(&mut self, path: &Path, id: &str) -> bool {
        let key = path.to_string_lossy().into_owned();
        let Some(annotations) = self.documents.get_mut(&key) else {
            return false;
        };
        let before = annotations.len();
        annotations.retain(|annotation| annotation.id != id);
        if annotations.len() == before {
            return false;
        }
        self.deleted
            .entry(key.clone())
            .or_default()
            .insert(id.into());
        if let Some(added) = self.added.get_mut(&key) {
            added.retain(|annotation| annotation.id != id);
        }
        true
    }

    pub fn for_document(&self, path: &Path) -> &[Annotation] {
        self.documents
            .get(path.to_string_lossy().as_ref())
            .map(Vec::as_slice)
            .unwrap_or(&[])
    }

    fn merge_changes(&mut self, changes: &Self) {
        for (document, ids) in &changes.deleted {
            if let Some(annotations) = self.documents.get_mut(document) {
                annotations.retain(|annotation| !ids.contains(&annotation.id));
            }
        }
        for (document, additions) in &changes.added {
            let annotations = self.documents.entry(document.clone()).or_default();
            for addition in additions {
                annotations.retain(|existing| existing.id != addition.id);
                annotations.push(addition.clone());
            }
        }
        self.documents
            .retain(|_, annotations| !annotations.is_empty());
        self.added.clear();
        self.deleted.clear();
    }

    fn validate(&self) -> Result<(), String> {
        let mut ids = HashSet::new();
        for (document, annotations) in &self.documents {
            if document.is_empty() {
                return Err("annotation store contains an empty document path".into());
            }
            for annotation in annotations {
                let rect = annotation.rect;
                if annotation.id.is_empty()
                    || annotation.page < 0
                    || !rect.x.is_finite()
                    || !rect.y.is_finite()
                    || !rect.width.is_finite()
                    || !rect.height.is_finite()
                    || rect.width <= 0.0
                    || rect.height <= 0.0
                {
                    return Err(format!("invalid annotation in {document}"));
                }
                if !ids.insert((document, &annotation.id)) {
                    return Err(format!(
                        "duplicate annotation id {} in {document}",
                        annotation.id
                    ));
                }
                if matches!(&annotation.kind, AnnotationKind::Note { text } if text.trim().is_empty())
                {
                    return Err(format!("empty annotation note in {document}"));
                }
            }
        }
        Ok(())
    }
}

fn annotation_path() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".into());
    PathBuf::from(home)
        .join(".config")
        .join("lector")
        .join("annotations.toml")
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
        .map_err(|error| format!("open annotation lock {}: {error}", path.display()))
}

fn write_private_atomic(path: &Path, contents: &[u8]) -> Result<(), String> {
    let dir = path
        .parent()
        .ok_or_else(|| format!("annotation path {} has no parent", path.display()))?;
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("annotations.toml");
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
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(format!("create temporary annotation file: {error}")),
        }
    }
    let Some((temp, mut file)) = created else {
        return Err("could not create a unique temporary annotation file".into());
    };
    let result = (|| {
        file.write_all(contents)
            .map_err(|error| format!("write temporary annotation {}: {error}", temp.display()))?;
        file.sync_all()
            .map_err(|error| format!("sync temporary annotation {}: {error}", temp.display()))?;
        drop(file);
        fs::rename(&temp, path)
            .map_err(|error| format!("replace annotation store {}: {error}", path.display()))
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temp);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::sync::atomic::{AtomicUsize, Ordering};

    static NEXT_TEST_DIR: AtomicUsize = AtomicUsize::new(0);

    fn test_path(name: &str) -> PathBuf {
        let id = NEXT_TEST_DIR.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!(
            "lector-annotations-{name}-{}-{id}",
            std::process::id()
        ));
        fs::create_dir_all(&dir).expect("create test annotation directory");
        dir.join("annotations.toml")
    }

    fn highlight(id: &str, page: i32) -> Annotation {
        Annotation {
            id: id.into(),
            page,
            rect: DocumentRect {
                x: 10.0,
                y: 20.0,
                width: 30.0,
                height: 12.0,
            },
            kind: AnnotationKind::Highlight,
        }
    }

    #[test]
    fn stale_writers_merge_annotations_for_the_same_document() {
        let store_path = test_path("merge");
        let document = Path::new("/documents/shared.pdf");
        let mut first = AnnotationStore::load_from_path(&store_path).unwrap();
        let mut second = AnnotationStore::load_from_path(&store_path).unwrap();
        first.add(document, highlight("first", 1));
        second.add(document, highlight("second", 2));

        first.save_to(&store_path).unwrap();
        second.save_to(&store_path).unwrap();

        let merged = AnnotationStore::load_from_path(&store_path).unwrap();
        let ids = merged
            .for_document(document)
            .iter()
            .map(|annotation| annotation.id.as_str())
            .collect::<Vec<_>>();
        assert_eq!(ids, vec!["first", "second"]);
        fs::remove_dir_all(store_path.parent().unwrap()).unwrap();
    }

    #[test]
    fn note_text_and_region_round_trip() {
        let store_path = test_path("note-round-trip");
        let document = Path::new("/documents/notes.pdf");
        let note = Annotation {
            id: "note-1".into(),
            page: 4,
            rect: DocumentRect {
                x: 11.0,
                y: 22.0,
                width: 33.0,
                height: 44.0,
            },
            kind: AnnotationKind::Note {
                text: "Revisit this assumption.".into(),
            },
        };
        let mut store = AnnotationStore::default();
        store.add(document, note.clone());
        store.save_to(&store_path).unwrap();
        assert_eq!(
            AnnotationStore::load_from_path(&store_path)
                .unwrap()
                .for_document(document),
            &[note]
        );
        fs::remove_dir_all(store_path.parent().unwrap()).unwrap();
    }

    #[test]
    fn annotation_bounds_round_trip_at_every_rotation() {
        let original = DocumentRect {
            x: 10.0,
            y: 20.0,
            width: 30.0,
            height: 40.0,
        };
        let expected = [
            (
                0,
                DocumentRect {
                    x: 10.0,
                    y: 20.0,
                    width: 30.0,
                    height: 40.0,
                },
            ),
            (
                90,
                DocumentRect {
                    x: 140.0,
                    y: 10.0,
                    width: 40.0,
                    height: 30.0,
                },
            ),
            (
                180,
                DocumentRect {
                    x: 60.0,
                    y: 140.0,
                    width: 30.0,
                    height: 40.0,
                },
            ),
            (
                270,
                DocumentRect {
                    x: 20.0,
                    y: 60.0,
                    width: 40.0,
                    height: 30.0,
                },
            ),
        ];
        for (rotation, displayed) in expected {
            assert_eq!(
                document_rect_to_display(original, 100.0, 200.0, rotation),
                displayed,
                "display bounds at {rotation}°"
            );
            assert_eq!(
                display_rect_to_document(
                    displayed.x,
                    displayed.y,
                    displayed.x + displayed.width,
                    displayed.y + displayed.height,
                    100.0,
                    200.0,
                    rotation,
                ),
                Some(original),
                "round trip at {rotation}°"
            );
        }
    }

    #[test]
    fn corrupt_annotation_store_is_reported_and_never_replaced() {
        let store_path = test_path("corrupt");
        let corrupt = "this is not = valid [toml";
        fs::write(&store_path, corrupt).unwrap();
        assert!(AnnotationStore::load_from_path(&store_path).is_err());

        let mut fresh = AnnotationStore::default();
        fresh.add(Path::new("/documents/a.pdf"), highlight("new", 0));
        assert!(fresh.save_to(&store_path).is_err());
        assert_eq!(fs::read_to_string(&store_path).unwrap(), corrupt);
        fs::remove_dir_all(store_path.parent().unwrap()).unwrap();
    }

    #[test]
    fn stale_deletion_does_not_discard_another_writers_addition() {
        let store_path = test_path("delete-merge");
        let document = Path::new("/documents/shared.pdf");
        let mut initial = AnnotationStore::default();
        initial.add(document, highlight("old", 0));
        initial.save_to(&store_path).unwrap();
        let mut deleting = AnnotationStore::load_from_path(&store_path).unwrap();
        let mut adding = AnnotationStore::load_from_path(&store_path).unwrap();
        assert!(deleting.remove(document, "old"));
        adding.add(document, highlight("new", 1));
        adding.save_to(&store_path).unwrap();
        deleting.save_to(&store_path).unwrap();

        let merged = AnnotationStore::load_from_path(&store_path).unwrap();
        assert_eq!(merged.for_document(document), &[highlight("new", 1)]);
        fs::remove_dir_all(store_path.parent().unwrap()).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn saved_annotation_store_and_lock_are_private() {
        use std::os::unix::fs::PermissionsExt;

        let store_path = test_path("permissions");
        let mut store = AnnotationStore::default();
        store.add(Path::new("/documents/a.pdf"), highlight("private", 0));
        store.save_to(&store_path).unwrap();
        for path in [&store_path, &store_path.with_extension("toml.lock")] {
            assert_eq!(
                fs::metadata(path).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }
        fs::remove_dir_all(store_path.parent().unwrap()).unwrap();
    }
}
