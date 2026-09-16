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

pub fn write_private_atomic(path: &Path, contents: &[u8]) -> Result<(), String> {
    let dir = path
        .parent()
        .ok_or_else(|| format!("private file path {} has no parent", path.display()))?;
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("state.toml");
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
            Err(error) => return Err(format!("create temporary private file: {error}")),
        }
    }
    let Some((temp, mut file)) = created else {
        return Err("could not create a unique temporary private file".into());
    };
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
