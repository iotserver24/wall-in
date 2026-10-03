use crate::Error;
use std::{
    ffi::OsStr,
    fs::{self, OpenOptions},
    io::Write,
    os::unix::fs::OpenOptionsExt,
    path::{Path, PathBuf},
    process::Command,
};

const UNIT: &str = "wall-in.service";
const MARKER: &str = "# Managed by Wall-in; remove only through Wall-in settings.\n";

fn unit_path() -> Result<PathBuf, Error> {
    let config = match std::env::var_os("XDG_CONFIG_HOME") {
        Some(path) if Path::new(&path).is_absolute() => PathBuf::from(path),
        _ => PathBuf::from(std::env::var_os("HOME").ok_or_else(|| Error::bad("HOME is unset"))?)
            .join(".config"),
    };
    Ok(config.join("systemd/user").join(UNIT))
}

fn quote(value: &OsStr) -> Result<String, Error> {
    let value = value
        .to_str()
        .ok_or_else(|| Error::bad("Autostart paths must be UTF-8"))?;
    if value.chars().any(char::is_control) {
        return Err(Error::bad(
            "Autostart paths cannot contain control characters",
        ));
    }
    Ok(format!(
        "\"{}\"",
        value
            .replace('\\', "\\\\")
            .replace('"', "\\\"")
            .replace('%', "%%")
    ))
}

fn unit(binary: &Path, root: &Path, data_dir: &Path) -> Result<String, Error> {
    if !binary.is_absolute() || !root.is_absolute() || !data_dir.is_absolute() {
        return Err(Error::bad("Autostart requires absolute paths"));
    }
    Ok(format!(
        "{MARKER}[Unit]\nDescription=Wall-in wallpaper service\nAfter=graphical-session.target\nPartOf=graphical-session.target\n\n[Service]\nType=simple\nEnvironment={}\nExecStart={} serve --data-dir {}\nMemoryMax=16000000000\nMemorySwapMax=0\nRestart=on-failure\nRestartSec=5s\n\n[Install]\nWantedBy=graphical-session.target\n",
        quote(
            std::ffi::OsString::from(format!(
                "WALL_IN_ROOT={}",
                root.to_str()
                    .ok_or_else(|| Error::bad("Autostart paths must be UTF-8"))?
            ))
            .as_os_str()
        )?,
        quote(binary.as_os_str())?,
        quote(data_dir.as_os_str())?,
    ))
}

fn systemctl(binary: &Path, action: &str) -> Result<(), Error> {
    let mut command = Command::new(binary);
    command.arg("--user").arg(action);
    if action != "daemon-reload" {
        command.arg(UNIT);
    }
    let output = command
        .output()
        .map_err(|e| Error::unavailable(format!("Cannot run systemctl --user {action}: {e}")))?;
    if !output.status.success() {
        return Err(Error::unavailable(format!(
            "systemctl --user {action} failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        )));
    }
    Ok(())
}

fn managed(path: &Path) -> Result<bool, Error> {
    match fs::symlink_metadata(path) {
        Ok(meta) => {
            if !meta.file_type().is_file() {
                return Err(Error::conflict("Autostart unit path is not a regular file"));
            }
            let bytes = fs::read(path)?;
            if !bytes.starts_with(MARKER.as_bytes()) {
                return Err(Error::conflict("Autostart unit is not managed by Wall-in"));
            }
            Ok(true)
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(e) => Err(e.into()),
    }
}

fn change(enabled: bool, path: &Path, contents: &str, ctl: &Path) -> Result<(), Error> {
    let exists = managed(path)?;
    if enabled {
        if exists {
            return Err(Error::conflict(
                "Autostart unit already exists; refusing to overwrite it",
            ));
        }
        fs::create_dir_all(path.parent().unwrap())?;
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(path)?;
        if let Err(e) = file
            .write_all(contents.as_bytes())
            .and_then(|_| file.sync_all())
        {
            let _ = fs::remove_file(path);
            return Err(e.into());
        }
        if let Err(error) = systemctl(ctl, "daemon-reload").and_then(|_| systemctl(ctl, "enable")) {
            if let Err(rollback) = systemctl(ctl, "disable") {
                return Err(Error::unavailable(format!(
                    "{error}; cannot undo autostart: {rollback}"
                )));
            }
            fs::remove_file(path)?;
            systemctl(ctl, "daemon-reload")?;
            return Err(error);
        }
    } else if exists {
        let previous = fs::read(path)?;
        systemctl(ctl, "disable")?;
        if let Err(error) = fs::remove_file(path)
            .map_err(Error::from)
            .and_then(|_| systemctl(ctl, "daemon-reload"))
        {
            let rollback = (|| -> Result<(), Error> {
                if !path.exists() {
                    let mut file = OpenOptions::new()
                        .write(true)
                        .create_new(true)
                        .mode(0o600)
                        .open(path)?;
                    file.write_all(&previous)?;
                    file.sync_all()?;
                }
                systemctl(ctl, "daemon-reload")?;
                systemctl(ctl, "enable")
            })();
            if let Err(rollback) = rollback {
                return Err(Error::unavailable(format!(
                    "{error}; cannot restore autostart: {rollback}"
                )));
            }
            return Err(error);
        }
    }
    Ok(())
}

pub fn set(enabled: bool, root: &Path, data_dir: &Path) -> Result<(), Error> {
    let binary = std::env::current_exe()?.canonicalize()?;
    let path = unit_path()?;
    let contents = unit(&binary, root, data_dir)?;
    change(enabled, &path, &contents, Path::new("systemctl"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn unit_uses_explicit_paths_and_quotes_specifiers() {
        let text = unit(
            Path::new("/bin/my wall%in"),
            Path::new("/tmp/root%here"),
            Path::new("/tmp/data dir"),
        )
        .unwrap();
        assert!(text.contains("Environment=\"WALL_IN_ROOT=/tmp/root%%here\""));
        assert!(text.contains("ExecStart=\"/bin/my wall%%in\" serve --data-dir \"/tmp/data dir\""));
        assert!(
            unit(
                Path::new("relative"),
                Path::new("/root"),
                Path::new("/data")
            )
            .is_err()
        );
    }

    #[test]
    fn toggle_is_reversible_and_refuses_foreign_units_or_failed_commands() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("systemd/user/wall-in.service");
        let ctl = dir.path().join("systemctl");
        fs::write(
            &ctl,
            format!(
                "#!/bin/sh\ncase \"$2\" in enable) test ! -e '{}' || exit 1;; disable) test ! -e '{}' || exit 1;; esac\nexit 0\n",
                dir.path().join("fail").display(),
                dir.path().join("fail-disable").display()
            ),
        )
        .unwrap();
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&ctl, fs::Permissions::from_mode(0o700)).unwrap();
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, "[Unit]\nDescription=foreign\n").unwrap();
        assert_eq!(
            change(true, &path, "managed", &ctl).unwrap_err().status,
            axum::http::StatusCode::CONFLICT
        );
        assert_eq!(
            change(false, &path, "managed", &ctl).unwrap_err().status,
            axum::http::StatusCode::CONFLICT
        );
        assert_eq!(
            fs::read_to_string(&path).unwrap(),
            "[Unit]\nDescription=foreign\n"
        );
        fs::remove_file(&path).unwrap();
        fs::write(dir.path().join("fail"), "").unwrap();
        assert!(change(true, &path, MARKER, &ctl).is_err());
        assert!(!path.exists());
        fs::remove_file(dir.path().join("fail")).unwrap();
        change(true, &path, MARKER, &ctl).unwrap();
        assert!(path.exists());
        assert!(change(true, &path, MARKER, &ctl).is_err());
        fs::write(dir.path().join("fail-disable"), "").unwrap();
        assert!(change(false, &path, MARKER, &ctl).is_err());
        assert!(path.exists());
        fs::remove_file(dir.path().join("fail-disable")).unwrap();
        change(false, &path, MARKER, &ctl).unwrap();
        assert!(!path.exists());
        std::os::unix::fs::symlink(&ctl, &path).unwrap();
        assert_eq!(
            change(false, &path, MARKER, &ctl).unwrap_err().status,
            axum::http::StatusCode::CONFLICT
        );
        assert!(
            fs::symlink_metadata(&path)
                .unwrap()
                .file_type()
                .is_symlink()
        );
    }
}
