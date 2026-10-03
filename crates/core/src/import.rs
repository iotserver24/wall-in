use crate::{
    Error,
    models::{Entry, Kind, Wallpaper},
};
use serde::Deserialize;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::Read,
    path::{Component, Path, PathBuf},
};

pub const MAX_JSON: u64 = 2 * 1024 * 1024;
const MAX_SCENE: u64 = 4096;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Camera {
    fov: f64,
    distance: f64,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Cube {
    size: f64,
    rpm: f64,
    colors: [String; 6],
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct NativeScene {
    version: u32,
    camera: Camera,
    cube: Cube,
    background: String,
}

fn color(s: &str) -> bool {
    s.len() == 7 && s.starts_with('#') && s.as_bytes()[1..].iter().all(u8::is_ascii_hexdigit)
}

pub fn validate_scene(path: &Path) -> Result<(), Error> {
    if !path.to_string_lossy().ends_with(".wallin-scene.json") {
        return Err(Error::bad("Native scene requires .wallin-scene.json"));
    }
    let scene: NativeScene = serde_json::from_slice(&read_limited(path, MAX_SCENE)?)
        .map_err(|e| Error::bad(format!("Invalid native scene: {e}")))?;
    if scene.version != 1
        || !scene.camera.fov.is_finite()
        || !(30.0..=100.0).contains(&scene.camera.fov)
        || !scene.camera.distance.is_finite()
        || !(3.0..=20.0).contains(&scene.camera.distance)
        || !scene.cube.size.is_finite()
        || !(0.2..=2.0).contains(&scene.cube.size)
        || !scene.cube.rpm.is_finite()
        || !(1.0..=60.0).contains(&scene.cube.rpm)
        || scene.cube.size * 1.74 >= scene.camera.distance - 0.1
        || !color(&scene.background)
        || !scene.cube.colors.iter().all(|c| color(c))
    {
        return Err(Error::bad(
            "Native scene contains unsupported or out-of-range values",
        ));
    }
    Ok(())
}

pub fn read_limited(path: &Path, limit: u64) -> Result<Vec<u8>, Error> {
    let file = fs::File::open(path)
        .map_err(|e| Error::bad(format!("Cannot open {}: {e}", path.display())))?;
    if !file.metadata()?.is_file() {
        return Err(Error::bad("Expected a regular file"));
    }
    let mut bytes = Vec::new();
    file.take(limit + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > limit {
        return Err(Error::bad("File exceeds size limit"));
    }
    Ok(bytes)
}

pub fn confined(root: &Path, reference: &str) -> Result<PathBuf, Error> {
    let normalized = reference.replace('\\', "/");
    let path = Path::new(&normalized);
    if path.as_os_str().is_empty()
        || path.is_absolute()
        || normalized.contains(':')
        || path.components().any(|c| {
            matches!(
                c,
                Component::ParentDir | Component::RootDir | Component::Prefix(_)
            )
        })
    {
        return Err(Error::bad(
            "Project resource path must be relative and cannot traverse parents",
        ));
    }
    let resolved = root
        .join(path)
        .canonicalize()
        .map_err(|e| Error::bad(format!("Missing project resource {reference}: {e}")))?;
    if !resolved.starts_with(root) || !resolved.is_file() {
        return Err(Error::bad(
            "Project resource escapes its directory or is not a regular file",
        ));
    }
    Ok(resolved)
}

pub fn checked_resource(entry: &Entry, preview: bool) -> Result<PathBuf, Error> {
    let original = if preview {
        entry.preview.as_ref().or(entry.media.as_ref())
    } else {
        entry.media.as_ref().or(entry.preview.as_ref())
    }
    .ok_or_else(|| Error::not_found("No local media for this wallpaper"))?;
    let resolved = original
        .canonicalize()
        .map_err(|_| Error::not_found("Wallpaper source is no longer available"))?;
    if &resolved != original || !resolved.starts_with(&entry.root) || !resolved.is_file() {
        return Err(Error::bad(
            "Wallpaper resource changed or escaped its import directory; reimport it",
        ));
    }
    Ok(resolved)
}

fn kind_for(path: &Path) -> Option<Kind> {
    match path.extension()?.to_str()?.to_ascii_lowercase().as_str() {
        "mp4" | "webm" | "mkv" | "mov" | "avi" | "m4v" | "ogv" => Some(Kind::Video),
        "png" | "jpg" | "jpeg" | "webp" | "gif" | "bmp" | "avif" => Some(Kind::Image),
        "html" | "htm" => Some(Kind::Web),
        "pkg" => Some(Kind::Scene),
        _ => None,
    }
}
fn string(value: &Value, field: &str, limit: usize) -> Result<Option<String>, Error> {
    match value.get(field) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(s)) if s.len() <= limit => Ok(Some(s.clone())),
        _ => Err(Error::bad(format!("Invalid or oversized project {field}"))),
    }
}
pub fn import_path(path: &Path) -> Result<Entry, Error> {
    if path.as_os_str().len() > 4096 {
        return Err(Error::bad("Import path is too long"));
    }
    let mut source = path
        .canonicalize()
        .map_err(|e| Error::bad(format!("Cannot access import path: {e}")))?;
    if source.is_dir() {
        source = confined(&source, "project.json")?;
    }
    if !source.is_file() {
        return Err(Error::bad(
            "Import path must be a file or project directory",
        ));
    }
    let root = source
        .parent()
        .ok_or_else(|| Error::bad("Invalid import directory"))?
        .to_path_buf();
    let id = format!(
        "{:x}",
        Sha256::digest(source.as_os_str().as_encoded_bytes())
    )[..24]
        .to_string();
    let mut title = source
        .file_stem()
        .unwrap_or_default()
        .to_string_lossy()
        .into_owned();
    let mut description = String::new();
    let mut tags = Vec::new();
    let mut preview = None;
    let (kind, media) = if source.file_name().is_some_and(|n| n == "project.json") {
        let value: Value = serde_json::from_slice(&read_limited(&source, MAX_JSON)?)
            .map_err(|e| Error::bad(format!("Invalid project.json: {e}")))?;
        if !value.is_object() {
            return Err(Error::bad("project.json must be an object"));
        }
        if let Some(t) = string(&value, "title", 512)? {
            title = t;
        }
        if let Some(d) = string(&value, "description", 16384)? {
            description = d;
        }
        let declared = string(&value, "type", 100)?
            .unwrap_or_default()
            .to_ascii_lowercase();
        let resource = string(&value, "file", 4096)?
            .map(|p| confined(&root, &p))
            .transpose()?;
        let kind = match declared.as_str() {
            "video" => Kind::Video,
            "image" => Kind::Image,
            "web" => Kind::Web,
            "scene" => Kind::Scene,
            "application" => Kind::Application,
            "" => resource
                .as_deref()
                .and_then(kind_for)
                .unwrap_or(Kind::Application),
            other => {
                description = format!("Unsupported project type: {other}. {description}");
                Kind::Application
            }
        };
        if matches!(kind, Kind::Image | Kind::Video) && resource.is_none() {
            return Err(Error::bad(
                "Image/video project is missing its file resource",
            ));
        }
        if matches!(kind, Kind::Image | Kind::Video)
            && resource.as_deref().and_then(kind_for) != Some(kind)
        {
            return Err(Error::bad(
                "Project type does not match its media file extension",
            ));
        }
        preview = string(&value, "preview", 4096)?
            .filter(|p| !p.is_empty())
            .map(|p| confined(&root, &p))
            .transpose()?;
        if preview
            .as_deref()
            .is_some_and(|p| kind_for(p) != Some(Kind::Image))
        {
            preview = None;
        }
        if let Some(raw) = value.get("tags") {
            tags = serde_json::from_value(raw.clone())
                .map_err(|_| Error::bad("Project tags must be an array of strings"))?;
            validate_tags(&tags)?;
        }
        (
            kind,
            resource.filter(|_| matches!(kind, Kind::Image | Kind::Video)),
        )
    } else if source.to_string_lossy().ends_with(".wallin-scene.json") {
        validate_scene(&source)?;
        title = source
            .file_name()
            .unwrap()
            .to_string_lossy()
            .trim_end_matches(".wallin-scene.json")
            .to_owned();
        (Kind::Scene, Some(source.clone()))
    } else {
        let kind = kind_for(&source).unwrap_or(Kind::Application);
        if !matches!(kind, Kind::Image | Kind::Video) {
            description =
                "This wallpaper format is recognized but cannot be rendered locally.".into();
        }
        (
            kind,
            matches!(kind, Kind::Image | Kind::Video).then(|| source.clone()),
        )
    };
    if title.is_empty() || title.len() > 512 {
        return Err(Error::bad("Wallpaper title must contain 1–512 bytes"));
    }
    if kind == Kind::Image && preview.is_none() {
        preview = media.clone();
    }
    let wallpaper = Wallpaper {
        preview_url: preview
            .as_ref()
            .map(|_| format!("/api/media/{id}?preview=1")),
        media_url: media.as_ref().map(|_| format!("/api/media/{id}")),
        id,
        title,
        kind,
        path: source.to_string_lossy().into_owned(),
        favorite: false,
        tags,
        supported: media.is_some(),
        description,
    };
    Ok(Entry {
        wallpaper,
        root,
        media,
        preview,
    })
}
pub fn validate_tags(tags: &[String]) -> Result<(), Error> {
    if tags.len() > 64
        || tags
            .iter()
            .any(|t| t.is_empty() || t.len() > 128 || t.chars().any(char::is_control))
    {
        return Err(Error::bad("Use at most 64 tags of 1–128 bytes each"));
    }
    Ok(())
}
