use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    Video,
    Image,
    Web,
    Scene,
    Application,
}
impl Kind {
    pub fn supported(self) -> bool {
        matches!(self, Self::Video | Self::Image | Self::Scene)
    }
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Video => "video",
            Self::Image => "image",
            Self::Web => "web",
            Self::Scene => "scene",
            Self::Application => "application",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Wallpaper {
    pub id: String,
    pub title: String,
    pub kind: Kind,
    pub path: String,
    pub preview_url: Option<String>,
    pub media_url: Option<String>,
    pub favorite: bool,
    pub tags: Vec<String>,
    pub supported: bool,
    pub description: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Entry {
    pub wallpaper: Wallpaper,
    pub root: PathBuf,
    pub media: Option<PathBuf>,
    pub preview: Option<PathBuf>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Fit {
    Cover,
    Contain,
    Fill,
}
impl Fit {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Cover => "cover",
            Self::Contain => "contain",
            Self::Fill => "fill",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Settings {
    pub volume: u8,
    pub muted: bool,
    pub fit: Fit,
    pub monitor: String,
    pub pause_on_battery: bool,
    pub pause_on_fullscreen: bool,
    pub autostart: bool,
    pub playlist_ids: Vec<String>,
    pub playlist_interval: u64,
    pub shuffle: bool,
}
impl Default for Settings {
    fn default() -> Self {
        Self {
            volume: 50,
            muted: true,
            fit: Fit::Cover,
            monitor: "all".into(),
            pause_on_battery: true,
            pause_on_fullscreen: true,
            autostart: false,
            playlist_ids: vec![],
            playlist_interval: 300,
            shuffle: false,
        }
    }
}
impl Settings {
    pub fn validate(&self) -> Result<(), crate::Error> {
        if self.volume > 100 {
            return Err(crate::Error::bad("volume must be between 0 and 100"));
        }
        if self.monitor.is_empty()
            || self.monitor.len() > 256
            || self.monitor.chars().any(char::is_control)
        {
            return Err(crate::Error::bad("invalid monitor ID"));
        }
        if !(1..=604800).contains(&self.playlist_interval) {
            return Err(crate::Error::bad(
                "playlist_interval must be between 1 and 604800 seconds",
            ));
        }
        if self.playlist_ids.len() > 10000 {
            return Err(crate::Error::bad("playlist is too large"));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Library {
    pub entries: Vec<Entry>,
    pub settings: Settings,
    pub steam_paths: Vec<String>,
    #[serde(default)]
    pub desired_wallpaper_id: Option<String>,
    #[serde(default)]
    pub desired_paused: bool,
    #[serde(default)]
    pub widgets: Vec<crate::widgets::Widget>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Playback {
    pub wallpaper_id: Option<String>,
    pub status: String,
    pub error: Option<String>,
}
impl Default for Playback {
    fn default() -> Self {
        Self {
            wallpaper_id: None,
            status: "stopped".into(),
            error: None,
        }
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Monitor {
    pub id: String,
    pub name: String,
    pub width: u32,
    pub height: u32,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Platform {
    pub desktop: String,
    pub session: String,
    pub backend: String,
    pub available: bool,
    pub message: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Diagnostics {
    #[serde(flatten)]
    pub platform: Platform,
    pub monitors: Vec<Monitor>,
}
#[derive(Debug, Clone, Serialize)]
pub struct State {
    pub wallpapers: Vec<Wallpaper>,
    pub settings: Settings,
    pub playback: Playback,
    pub platform: Platform,
    pub monitors: Vec<Monitor>,
    pub steam_paths: Vec<String>,
    pub widgets: Vec<crate::widgets::Widget>,
    pub widget_error: Option<String>,
}
