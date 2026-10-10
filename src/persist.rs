use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

static NEXT_TEMP_FILE: AtomicUsize = AtomicUsize::new(0);

pub fn config_dir(xdg_config_home: Option<&Path>, home: &Path, app: &str) -> PathBuf {
    match xdg_config_home {
        Some(dir) if dir.is_absolute() => dir.join(app),
        _ => home.join(".config").join(app),
    }
}

pub fn user_config_dir(app: &str) -> PathBuf {
    config_dir(xdg_config_home().as_deref(), &home_dir(), app)
}

pub fn user_config_file(app: &str, name: &str) -> PathBuf {
    user_config_dir(app).join(name)
}

fn xdg_config_home() -> Option<PathBuf> {
    std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
}

fn home_dir() -> PathBuf {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
}

pub fn open_lock_file(path: &Path) -> Result<fs::File, String> {
    let mut options = OpenOptions::new();
    options.read(true).write(true).create(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    options
        .open(path)
        .map_err(|error| format!("open lock {}: {error}", path.display()))
}

/// Creates a new, uniquely named file next to `path` for an atomic replace.
fn create_temp_beside(path: &Path, mode: u32) -> Result<(PathBuf, fs::File), String> {
    let dir = path
        .parent()
        .ok_or_else(|| format!("file path {} has no parent", path.display()))?;
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("state.toml");
    for _ in 0..128 {
        let id = NEXT_TEMP_FILE.fetch_add(1, Ordering::Relaxed);
        let temp = dir.join(format!(".{name}.{}-{id}.tmp", std::process::id()));
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(mode);
        }
        #[cfg(not(unix))]
        let _ = mode;
        match options.open(&temp) {
            Ok(file) => return Ok((temp, file)),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(format!("create temporary file: {error}")),
        }
    }
    Err("could not create a unique temporary file".into())
}

pub fn write_private_atomic(path: &Path, contents: &[u8]) -> Result<(), String> {
    let (temp, mut file) = create_temp_beside(path, 0o600)?;
    let result = (|| {
        file.write_all(contents)
            .map_err(|error| format!("write temporary private file {}: {error}", temp.display()))?;
        file.sync_all()
            .map_err(|error| format!("sync temporary private file {}: {error}", temp.display()))?;
        drop(file);
        fs::rename(&temp, path)
            .map_err(|error| format!("replace private file {}: {error}", path.display()))
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temp);
    }
    result
}

/// Replaces `path` with a file that `write` fills at a temporary path next to
/// it, so a failed write never leaves `path` truncated. An existing file keeps
/// its permissions.
pub fn replace_file(
    path: &Path,
    write: impl FnOnce(&Path) -> Result<(), String>,
) -> Result<(), String> {
    let (temp, file) = create_temp_beside(path, 0o666)?;
    drop(file);
    let result = (|| {
        write(&temp)?;
        fs::File::open(&temp)
            .and_then(|file| file.sync_all())
            .map_err(|error| format!("sync temporary file {}: {error}", temp.display()))?;
        if let Ok(existing) = fs::metadata(path) {
            let _ = fs::set_permissions(&temp, existing.permissions());
        }
        fs::rename(&temp, path).map_err(|error| format!("replace {}: {error}", path.display()))
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temp);
    }
    result
}

/// True when both paths name one existing file, also through symlinks and
/// hard links.
pub fn same_file(a: &Path, b: &Path) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        match (fs::metadata(a), fs::metadata(b)) {
            (Ok(a), Ok(b)) => a.dev() == b.dev() && a.ino() == b.ino(),
            _ => false,
        }
    }
    #[cfg(not(unix))]
    {
        matches!((a.canonicalize(), b.canonicalize()), (Ok(a), Ok(b)) if a == b)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_dir_prefers_nonempty_xdg_config_home() {
        assert_eq!(
            config_dir(
                Some(Path::new("/xdg/config")),
                Path::new("/home/u"),
                "lector"
            ),
            PathBuf::from("/xdg/config/lector")
        );
    }

    #[test]
    fn config_dir_falls_back_to_home_dot_config() {
        assert_eq!(
            config_dir(None, Path::new("/home/u"), "lector"),
            PathBuf::from("/home/u/.config/lector")
        );
    }

    #[test]
    fn empty_xdg_config_home_uses_home_dot_config() {
        assert_eq!(
            config_dir(Some(Path::new("")), Path::new("/home/u"), "omadoc"),
            PathBuf::from("/home/u/.config/omadoc")
        );
    }

    #[test]
    fn replace_file_keeps_the_old_file_when_writing_fails() {
        let dir = std::env::temp_dir().join(format!("lector-replace-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("copy.pdf");
        fs::write(&path, b"old").unwrap();
        assert!(replace_file(&path, |temp| {
            fs::write(temp, b"partial").unwrap();
            Err("failed".into())
        })
        .is_err());
        assert_eq!(fs::read(&path).unwrap(), b"old");
        replace_file(&path, |temp| {
            fs::write(temp, b"new").map_err(|e| e.to_string())
        })
        .unwrap();
        assert_eq!(fs::read(&path).unwrap(), b"new");
        assert_eq!(
            fs::read_dir(&dir).unwrap().count(),
            1,
            "no temporary file left"
        );
        fs::remove_dir_all(dir).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn same_file_sees_through_links() {
        let dir = std::env::temp_dir().join(format!("lector-same-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("a.pdf");
        fs::write(&path, b"a").unwrap();
        let other = dir.join("b.pdf");
        fs::write(&other, b"a").unwrap();
        std::os::unix::fs::symlink(&path, dir.join("link.pdf")).unwrap();
        fs::hard_link(&path, dir.join("hard.pdf")).unwrap();
        assert!(same_file(&dir.join("link.pdf"), &path));
        assert!(same_file(&dir.join("hard.pdf"), &path));
        assert!(same_file(&dir.join(".").join("a.pdf"), &path));
        assert!(!same_file(&other, &path));
        assert!(!same_file(&dir.join("missing.pdf"), &path));
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn relative_xdg_config_home_is_ignored() {
        assert_eq!(
            config_dir(
                Some(Path::new("relative/config")),
                Path::new("/home/u"),
                "lector"
            ),
            PathBuf::from("/home/u/.config/lector")
        );
    }
}
