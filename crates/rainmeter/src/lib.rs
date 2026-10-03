//! A deliberately limited, non-executing Rainmeter INI reader.
//! No Rainmeter binaries, plugins, scripts or commands are loaded; Time measures are parsed, not executed.

use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

const MAX_INI_BYTES: u64 = 1_048_576;
const MAX_IMAGE_BYTES: u64 = 16_777_216;
const MAX_SECTIONS: usize = 512;
const MAX_OPTIONS: usize = 4096;
const MAX_EXPANSIONS: usize = 16;

#[derive(Debug)]
pub enum Error {
    Io(std::io::Error),
    UnsafePath(String),
    InvalidIni(String),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(e) => write!(f, "I/O: {e}"),
            Self::UnsafePath(e) => write!(f, "unsafe path: {e}"),
            Self::InvalidIni(e) => write!(f, "invalid INI: {e}"),
        }
    }
}
impl std::error::Error for Error {}
impl From<std::io::Error> for Error {
    fn from(value: std::io::Error) -> Self {
        Self::Io(value)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diagnostic {
    pub section: String,
    pub code: &'static str,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Placement {
    pub x: f32,
    pub y: f32,
    pub width: Option<f32>,
    pub height: Option<f32>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Shape {
    Rectangle {
        x: f32,
        y: f32,
        width: f32,
        height: f32,
    },
    Ellipse {
        cx: f32,
        cy: f32,
        rx: f32,
        ry: f32,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub enum Meter {
    Text {
        id: String,
        placement: Placement,
        text: String,
        clock_format: Option<String>,
        font_face: Option<String>,
        font_size: f32,
        color: [u8; 4],
    },
    Image {
        id: String,
        placement: Placement,
        path: PathBuf,
    },
    Shape {
        id: String,
        placement: Placement,
        shape: Shape,
        fill: [u8; 4],
    },
}

#[derive(Debug, Default)]
pub struct Report {
    pub meters: Vec<Meter>,
    pub diagnostics: Vec<Diagnostic>,
}

#[derive(Debug)]
struct Section {
    name: String,
    options: HashMap<String, String>,
}

fn diagnostic(report: &mut Report, section: &str, code: &'static str, message: impl Into<String>) {
    report.diagnostics.push(Diagnostic {
        section: section.into(),
        code,
        message: message.into(),
    });
}

fn archive(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    [".zip", ".rmskin", ".7z", ".rar", ".tar", ".gz"]
        .iter()
        .any(|ext| lower.ends_with(ext))
}

fn parts(input: &str) -> Result<Vec<&str>, Error> {
    if input.is_empty() || input.starts_with(['/', '\\']) || input.contains(['\0', ':']) {
        return Err(Error::UnsafePath(input.into()));
    }
    let parts: Vec<_> = input.split(['/', '\\']).collect();
    if parts
        .iter()
        .any(|p| p.is_empty() || *p == "." || *p == ".." || archive(p))
    {
        return Err(Error::UnsafePath(input.into()));
    }
    Ok(parts)
}

fn local_path(root: &Path, input: &str, limit: u64) -> Result<PathBuf, Error> {
    let mut path = root.to_path_buf();
    for part in parts(input)? {
        path.push(part);
        let meta = fs::symlink_metadata(&path)?;
        if meta.file_type().is_symlink() {
            return Err(Error::UnsafePath(format!("symlink: {}", path.display())));
        }
    }
    let meta = fs::metadata(&path)?;
    if !meta.is_file() || meta.len() > limit {
        return Err(Error::UnsafePath(format!(
            "not a regular file within size limit: {}",
            path.display()
        )));
    }
    if !path.canonicalize()?.starts_with(root) {
        return Err(Error::UnsafePath(input.into()));
    }
    Ok(path)
}

fn parse_ini(bytes: &[u8]) -> Result<Vec<Section>, Error> {
    let text = if bytes.starts_with(&[0xff, 0xfe]) {
        if !(bytes.len() - 2).is_multiple_of(2) {
            return Err(Error::InvalidIni("odd UTF-16 length".into()));
        }
        let units: Vec<_> = bytes[2..]
            .as_chunks::<2>()
            .0
            .iter()
            .map(|pair| u16::from_le_bytes(*pair))
            .collect();
        String::from_utf16(&units).map_err(|_| Error::InvalidIni("invalid UTF-16".into()))?
    } else {
        let bytes = bytes.strip_prefix(&[0xef, 0xbb, 0xbf]).unwrap_or(bytes);
        std::str::from_utf8(bytes)
            .map_err(|_| Error::InvalidIni("invalid UTF-8".into()))?
            .to_owned()
    };
    let mut sections: Vec<Section> = Vec::new();
    let mut count = 0;
    for (line_no, line) in text.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() || line.starts_with(';') {
            continue;
        }
        if line.starts_with('[') {
            if !line.ends_with(']') || line.len() <= 2 {
                return Err(Error::InvalidIni(format!(
                    "bad section on line {}",
                    line_no + 1
                )));
            }
            let name = line[1..line.len() - 1].trim();
            if name.is_empty()
                || sections.iter().any(|s| s.name.eq_ignore_ascii_case(name))
                || sections.len() >= MAX_SECTIONS
            {
                return Err(Error::InvalidIni(format!(
                    "duplicate/invalid section on line {}",
                    line_no + 1
                )));
            }
            sections.push(Section {
                name: name.to_owned(),
                options: HashMap::new(),
            });
        } else {
            let (key, value) = line
                .split_once('=')
                .ok_or_else(|| Error::InvalidIni(format!("bad option on line {}", line_no + 1)))?;
            let key = key.trim().to_ascii_lowercase();
            let section = sections
                .last_mut()
                .ok_or_else(|| Error::InvalidIni("option before section".into()))?;
            count += 1;
            if key.is_empty()
                || count > MAX_OPTIONS
                || section
                    .options
                    .insert(key, value.trim().to_owned())
                    .is_some()
            {
                return Err(Error::InvalidIni(format!(
                    "duplicate/invalid option on line {}",
                    line_no + 1
                )));
            }
        }
    }
    Ok(sections)
}

fn expand(
    value: &str,
    vars: &HashMap<String, String>,
    depth: usize,
    active: &mut HashSet<String>,
) -> Result<String, String> {
    if depth > MAX_EXPANSIONS {
        return Err("variable depth exceeded".into());
    }
    let mut result = String::new();
    let mut remainder = value;
    while let Some(start) = remainder.find('#') {
        result.push_str(&remainder[..start]);
        remainder = &remainder[start + 1..];
        if remainder.starts_with('@') && remainder.get(1..2) == Some("#") {
            // The caller maps #@# to the local @Resources directory.
            result.push_str("@Resources/");
            remainder = &remainder[2..];
            continue;
        }
        let Some(end) = remainder.find('#') else {
            return Err("unclosed variable".into());
        };
        let key = remainder[..end].to_ascii_lowercase();
        if key.is_empty() || !active.insert(key.clone()) {
            return Err("empty or cyclic variable".into());
        }
        let resolved = vars
            .get(&key)
            .ok_or_else(|| format!("unknown variable: {key}"))?;
        result.push_str(&expand(resolved, vars, depth + 1, active)?);
        active.remove(&key);
        remainder = &remainder[end + 1..];
        if result.len() > 16_384 {
            return Err("expanded value too long".into());
        }
    }
    result.push_str(remainder);
    if result.len() > 16_384 {
        return Err("expanded value too long".into());
    }
    Ok(result)
}

fn number(raw: Option<&String>, default: f32) -> Result<f32, String> {
    let n = match raw {
        Some(v) => v
            .parse::<f32>()
            .map_err(|_| format!("invalid number: {v}"))?,
        None => default,
    };
    if !n.is_finite() || n.abs() > 100_000.0 {
        return Err("number outside supported range".into());
    }
    Ok(n)
}
fn color(raw: &str) -> Result<[u8; 4], String> {
    let pieces: Vec<_> = raw.split(',').map(str::trim).collect();
    if !(pieces.len() == 3 || pieces.len() == 4) {
        return Err("expected RGB or RGBA color".into());
    }
    let mut rgba = [0, 0, 0, 255];
    for (i, piece) in pieces.iter().enumerate() {
        rgba[i] = piece
            .parse::<u8>()
            .map_err(|_| "invalid color component".to_string())?;
    }
    Ok(rgba)
}
fn shape(raw: &str) -> Result<(Shape, [u8; 4]), String> {
    let mut segments = raw.split('|').map(str::trim);
    let geometry = segments.next().unwrap_or("");
    let (kind, args) = geometry
        .split_once(char::is_whitespace)
        .ok_or("missing shape arguments")?;
    let values: Vec<_> = args
        .split(',')
        .map(|n| number(Some(&n.trim().to_owned()), 0.0))
        .collect::<Result<_, _>>()?;
    if values.len() != 4 {
        return Err("only four-argument rectangle/ellipse supported".into());
    }
    let geometry = match kind.to_ascii_lowercase().as_str() {
        "rectangle" if values[2] >= 0.0 && values[3] >= 0.0 => Shape::Rectangle {
            x: values[0],
            y: values[1],
            width: values[2],
            height: values[3],
        },
        "ellipse" if values[2] >= 0.0 && values[3] >= 0.0 => Shape::Ellipse {
            cx: values[0],
            cy: values[1],
            rx: values[2],
            ry: values[3],
        },
        _ => return Err("unsupported shape geometry".into()),
    };
    let mut fill = [0, 0, 0, 255];
    for segment in segments {
        let (option, arg) = segment
            .split_once(char::is_whitespace)
            .ok_or("unsupported shape option")?;
        if !option.eq_ignore_ascii_case("Fill") {
            return Err("unsupported shape option".into());
        }
        let (kind, value) = arg
            .trim()
            .split_once(char::is_whitespace)
            .ok_or("unsupported fill")?;
        if !kind.eq_ignore_ascii_case("Color") {
            return Err("unsupported fill".into());
        }
        fill = color(value.trim())?;
    }
    Ok((geometry, fill))
}

fn clock_format(value: &str) -> Result<(), String> {
    if value.is_empty() || value.len() > 64 || value.chars().any(char::is_control) {
        return Err("invalid Time format length or control character".into());
    }
    if value.contains(['[', ']', '#']) {
        return Err("dynamic Time format reference".into());
    }
    let mut chars = value.chars();
    while let Some(c) = chars.next() {
        if c == '%'
            && !matches!(
                chars.next(),
                Some('H' | 'M' | 'S' | 'I' | 'p' | 'd' | 'm' | 'Y' | 'A' | 'a' | 'B' | 'b' | '%')
            )
        {
            return Err("unsupported Time format directive".into());
        }
    }
    Ok(())
}

fn clock_template(text: &str) -> bool {
    text.match_indices("%1").next().is_some_and(|(index, _)| {
        text[index + 2..]
            .chars()
            .next()
            .is_none_or(|c| !c.is_ascii_digit())
    }) && text.matches("%1").count() == 1
        && !text.contains(['[', ']', '#'])
        && !text
            .as_bytes()
            .windows(2)
            .any(|pair| pair[0] == b'%' && pair[1].is_ascii_digit() && pair[1] != b'1')
}

fn dangerous(key: &str, value: &str) -> bool {
    key.starts_with("@include")
        || key.contains("action")
        || key.contains("command")
        || key == "plugin"
        || key == "scriptfile"
        || value.contains("[!")
        || value.starts_with('!')
        || value.to_ascii_lowercase().contains("https://")
        || value.to_ascii_lowercase().contains("http://")
}

/// Reads one INI under `root`; returns only supported static meters and diagnostics for omitted content.
/// The caller must not treat `Meter::Image::path` as an authorization to open it after the tree changes.
/// If a hostile process can mutate the tree concurrently, copy verified resources into a trusted store first.
/// No archive extraction or installation occurs here.
pub fn inspect_skin(root: impl AsRef<Path>, ini: impl AsRef<str>) -> Result<Report, Error> {
    let root = root.as_ref();
    if fs::symlink_metadata(root)?.file_type().is_symlink() {
        return Err(Error::UnsafePath("symlink root".into()));
    }
    let root = root.canonicalize()?;
    if !root.is_dir() {
        return Err(Error::UnsafePath("root is not a directory".into()));
    }
    let ini_path = local_path(&root, ini.as_ref(), MAX_INI_BYTES)?;
    if !ini_path
        .extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("ini"))
    {
        return Err(Error::UnsafePath("expected .ini file".into()));
    }
    let sections = parse_ini(&fs::read(ini_path)?)?;
    let vars: HashMap<_, _> = sections
        .iter()
        .filter(|s| s.name.eq_ignore_ascii_case("variables"))
        .flat_map(|s| s.options.iter().map(|(k, v)| (k.clone(), v.clone())))
        .collect();
    let mut report = Report::default();
    let mut time_measures = HashMap::new();
    for section in &sections {
        let Some(measure) = section.options.get("measure") else {
            continue;
        };
        if !measure.eq_ignore_ascii_case("time") {
            diagnostic(
                &mut report,
                &section.name,
                "unsupported_measure",
                format!("measure not executed: {measure}"),
            );
            if measure.eq_ignore_ascii_case("plugin") || section.options.contains_key("plugin") {
                diagnostic(
                    &mut report,
                    &section.name,
                    "blocked_plugin",
                    "plugin/DLL not loaded",
                );
            }
            if measure.eq_ignore_ascii_case("script") || section.options.contains_key("scriptfile")
            {
                diagnostic(
                    &mut report,
                    &section.name,
                    "blocked_script",
                    "Lua script not executed",
                );
            }
            continue;
        }
        let mut rejected = false;
        for (key, value) in &section.options {
            if !["measure", "format"].contains(&key.as_str()) || dangerous(key, value) {
                diagnostic(
                    &mut report,
                    &section.name,
                    if dangerous(key, value) {
                        "unsafe_option"
                    } else {
                        "unsupported_option"
                    },
                    format!("blocked option: {key}"),
                );
                rejected = true;
            }
        }
        if rejected {
            continue;
        }
        let parsed = section
            .options
            .get("format")
            .ok_or_else(|| "missing Time format".to_string())
            .and_then(|format| expand(format, &vars, 0, &mut HashSet::new()))
            .and_then(|format| {
                clock_format(&format)?;
                Ok(format)
            });
        match parsed {
            Ok(format) => {
                time_measures.insert(section.name.to_ascii_lowercase(), format);
            }
            Err(error) => diagnostic(&mut report, &section.name, "invalid_measure", error),
        }
    }
    for section in sections {
        let name = section.name.as_str();
        if name.eq_ignore_ascii_case("variables") || name.eq_ignore_ascii_case("rainmeter") {
            for (key, value) in &section.options {
                if dangerous(key, value) {
                    diagnostic(
                        &mut report,
                        name,
                        "unsafe_option",
                        format!("blocked option: {key}"),
                    );
                }
            }
            continue;
        }
        if section.options.contains_key("measure") {
            continue;
        }
        let Some(kind) = section.options.get("meter") else {
            diagnostic(
                &mut report,
                name,
                "unsupported_section",
                "section is not a static meter",
            );
            continue;
        };
        let allowed: &[&str] = match kind.to_ascii_lowercase().as_str() {
            "string" | "text" => &[
                "meter",
                "x",
                "y",
                "w",
                "h",
                "text",
                "measurename",
                "fontface",
                "fontsize",
                "fontcolor",
            ],
            "image" => &["meter", "x", "y", "w", "h", "imagename"],
            "shape" => &["meter", "x", "y", "w", "h", "shape"],
            _ => {
                diagnostic(
                    &mut report,
                    name,
                    "unsupported_meter",
                    format!("unsupported meter: {kind}"),
                );
                continue;
            }
        };
        let mut rejected = false;
        for (key, value) in &section.options {
            if !allowed.contains(&key.as_str()) || dangerous(key, value) {
                diagnostic(
                    &mut report,
                    name,
                    if dangerous(key, value) {
                        "unsafe_option"
                    } else {
                        "unsupported_option"
                    },
                    format!("blocked option: {key}"),
                );
                rejected = true;
            }
        }
        if rejected {
            continue;
        }
        let resolve = |key: &str| -> Result<Option<String>, String> {
            section
                .options
                .get(key)
                .map(|v| expand(v, &vars, 0, &mut HashSet::new()))
                .transpose()
        };
        let parsed = (|| -> Result<Meter, String> {
            let x = resolve("x")?;
            let y = resolve("y")?;
            let w = resolve("w")?;
            let h = resolve("h")?;
            let width = w.as_ref().map(|v| number(Some(v), 0.0)).transpose()?;
            let height = h.as_ref().map(|v| number(Some(v), 0.0)).transpose()?;
            if width.is_some_and(|v| v < 0.0) || height.is_some_and(|v| v < 0.0) {
                return Err("negative dimensions".into());
            }
            let placement = Placement {
                x: number(x.as_ref(), 0.0)?,
                y: number(y.as_ref(), 0.0)?,
                width,
                height,
            };
            let id = section.name.clone();
            match kind.to_ascii_lowercase().as_str() {
                "string" | "text" => {
                    let text = resolve("text")?.ok_or("missing Text")?;
                    let clock_format = if let Some(reference) = resolve("measurename")? {
                        if reference.is_empty()
                            || reference.contains([',', '[', ']', '#'])
                            || reference.trim() != reference
                        {
                            return Err("invalid MeasureName reference".into());
                        }
                        let format = time_measures
                            .get(&reference.to_ascii_lowercase())
                            .ok_or("MeasureName does not reference a supported Time measure")?;
                        if !clock_template(&text) {
                            return Err(
                                "Time meter Text must contain exactly one literal %1 placeholder"
                                    .into(),
                            );
                        }
                        Some(format.clone())
                    } else {
                        if text.contains(['[', ']']) {
                            return Err("unresolved dynamic Text reference".into());
                        }
                        None
                    };
                    let font_size = number(resolve("fontsize")?.as_ref(), 10.0)?;
                    if font_size <= 0.0 {
                        return Err("invalid font size".into());
                    }
                    let font_face = resolve("fontface")?;
                    let color =
                        color(&resolve("fontcolor")?.unwrap_or_else(|| "255,255,255,255".into()))?;
                    Ok(Meter::Text {
                        id,
                        placement,
                        text,
                        clock_format,
                        font_face,
                        font_size,
                        color,
                    })
                }
                "image" => {
                    let image = resolve("imagename")?.ok_or("missing ImageName")?;
                    let path =
                        local_path(&root, &image, MAX_IMAGE_BYTES).map_err(|e| e.to_string())?;
                    let ext = path
                        .extension()
                        .and_then(|s| s.to_str())
                        .unwrap_or("")
                        .to_ascii_lowercase();
                    if !["png", "jpg", "jpeg", "bmp"].contains(&ext.as_str()) {
                        return Err("unsupported image format".into());
                    }
                    Ok(Meter::Image {
                        id,
                        placement,
                        path,
                    })
                }
                "shape" => {
                    let (shape, fill) = shape(&resolve("shape")?.ok_or("missing Shape")?)?;
                    Ok(Meter::Shape {
                        id,
                        placement,
                        shape,
                        fill,
                    })
                }
                _ => unreachable!(),
            }
        })();
        match parsed {
            Ok(meter) => report.meters.push(meter),
            Err(e) => diagnostic(&mut report, name, "invalid_meter", e),
        }
    }
    Ok(report)
}
