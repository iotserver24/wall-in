use crate::{
    Error,
    import::{import_path, read_limited},
    models::Entry,
};
use std::{
    collections::BTreeSet,
    path::{Path, PathBuf},
};

// Only quoted strings are relevant to Steam library paths; comments and braces are ignored.
pub fn library_paths(text: &str) -> Vec<PathBuf> {
    let mut tokens = Vec::new();
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '/' && chars.peek() == Some(&'/') {
            for c in chars.by_ref() {
                if c == '\n' {
                    break;
                }
            }
        } else if c == '"' {
            let mut token = String::new();
            while let Some(c) = chars.next() {
                if c == '"' {
                    break;
                }
                if c == '\\' {
                    if let Some(next) = chars.next() {
                        token.push(next);
                    }
                } else {
                    token.push(c);
                }
            }
            tokens.push(token);
        } else if c == '{' || c == '}' {
            tokens.push(c.to_string());
        }
    }
    tokens
        .windows(2)
        .filter_map(|pair| {
            let key = &pair[0];
            let path = PathBuf::from(&pair[1]);
            ((key.eq_ignore_ascii_case("path") || key.chars().all(|c| c.is_ascii_digit()))
                && path.is_absolute())
            .then_some(path)
        })
        .take(256)
        .collect()
}

pub struct Scan {
    pub paths: Vec<String>,
    pub entries: Vec<Entry>,
}
pub fn scan(home: &Path) -> Result<Scan, Error> {
    let roots = [
        home.join(".steam/steam"),
        home.join(".steam/root"),
        home.join(".local/share/Steam"),
        home.join(".var/app/com.valvesoftware.Steam/.local/share/Steam"),
    ];
    scan_roots(&roots)
}
pub fn scan_roots(roots: &[PathBuf]) -> Result<Scan, Error> {
    let mut libraries = BTreeSet::new();
    for root in roots {
        if let Ok(root) = root.canonicalize() {
            for relative in ["steamapps/libraryfolders.vdf", "config/libraryfolders.vdf"] {
                let file = root.join(relative);
                if file.is_file() {
                    let bytes = read_limited(&file, 4 * 1024 * 1024)?;
                    for path in library_paths(&String::from_utf8_lossy(&bytes)) {
                        if let Ok(path) = path.canonicalize()
                            && path.is_dir()
                        {
                            libraries.insert(path);
                        }
                    }
                }
            }
            libraries.insert(root);
        }
    }
    let mut entries = Vec::new();
    let mut paths = Vec::new();
    let mut seen = BTreeSet::new();
    for library in libraries {
        let workshop = library.join("steamapps/workshop/content/431960");
        if !workshop.is_dir() {
            continue;
        }
        let workshop = workshop.canonicalize()?;
        paths.push(workshop.to_string_lossy().into_owned());
        for item in std::fs::read_dir(&workshop)?.take(10000) {
            let item = item?;
            if !item.file_type()?.is_dir() {
                continue;
            }
            let project = item.path().join("project.json");
            if !project.is_file() {
                continue;
            }
            if !project.canonicalize()?.starts_with(&workshop) {
                continue;
            }
            match import_path(&project) {
                Ok(entry) if seen.insert(entry.wallpaper.id.clone()) => entries.push(entry),
                Ok(_) => {}
                Err(error) => eprintln!("Skipping Steam project {}: {error}", project.display()),
            }
        }
    }
    Ok(Scan { paths, entries })
}
