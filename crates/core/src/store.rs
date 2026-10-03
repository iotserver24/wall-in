use crate::{Error, import::read_limited, models::Library};
use fs2::FileExt;
use std::{
    fs::{self, File, OpenOptions},
    io::Write,
    os::unix::fs::{DirBuilderExt, OpenOptionsExt},
    path::{Path, PathBuf},
};

pub struct Store {
    dir: PathBuf,
    _lock: File,
}
impl Store {
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    pub fn open(dir: PathBuf) -> Result<(Self, Library), Error> {
        fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(&dir)?;
        let lock = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .mode(0o600)
            .open(dir.join("library.lock"))?;
        lock.try_lock_exclusive().map_err(|_| {
            Error::bad("Another Wall-in service is already using this data directory")
        })?;
        let path = dir.join("library.json");
        let library: Library = if path.exists() {
            serde_json::from_slice(&read_limited(&path, 32 * 1024 * 1024)?)
                .map_err(|e| Error::bad(format!("Cannot read saved library: {e}")))?
        } else {
            Library::default()
        };
        library.settings.validate()?;
        Ok((Self { dir, _lock: lock }, library))
    }
    pub fn save(&self, library: &Library) -> Result<(), Error> {
        let bytes = serde_json::to_vec_pretty(library)?;
        if bytes.len() > 32 * 1024 * 1024 {
            return Err(Error::bad("Library exceeds the 32 MiB persistence limit"));
        }
        let temp = self
            .dir
            .join(format!(".library-{}.tmp", std::process::id()));
        let result = (|| -> Result<(), Error> {
            let mut file = OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(0o600)
                .open(&temp)?;
            file.write_all(&bytes)?;
            file.sync_all()?;
            fs::rename(&temp, self.dir.join("library.json"))?;
            File::open(&self.dir)?.sync_all()?;
            Ok(())
        })();
        if result.is_err() {
            let _ = fs::remove_file(&temp);
        }
        result
    }
}
impl Drop for Store {
    fn drop(&mut self) {
        let _ = FileExt::unlock(&self._lock);
    }
}
pub fn default_data_dir() -> Result<PathBuf, Error> {
    if let Some(path) = std::env::var_os("WALL_IN_DATA_DIR") {
        return absolute(PathBuf::from(path));
    }
    if let Some(path) = std::env::var_os("XDG_DATA_HOME") {
        let path = PathBuf::from(path);
        if path.is_absolute() {
            return Ok(path.join("wall-in"));
        }
    }
    let home = std::env::var_os("HOME")
        .ok_or_else(|| Error::bad("HOME or WALL_IN_DATA_DIR must be set"))?;
    Ok(Path::new(&home).join(".local/share/wall-in"))
}
fn absolute(path: PathBuf) -> Result<PathBuf, Error> {
    if path.as_os_str().is_empty() {
        return Err(Error::bad("Data directory cannot be empty"));
    }
    Ok(if path.is_absolute() {
        path
    } else {
        std::env::current_dir()?.join(path)
    })
}
