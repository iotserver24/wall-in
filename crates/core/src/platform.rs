use crate::{
    Error,
    models::{Diagnostics, Kind, Platform, Playback, Settings},
};
use std::{
    path::{Path, PathBuf},
    process::Stdio,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};
use tokio::{
    io::{AsyncRead, AsyncReadExt, AsyncWriteExt},
    process::{Child, Command},
    task::JoinHandle,
    time::timeout,
};

pub fn workspace_root() -> PathBuf {
    std::env::var_os("WALL_IN_ROOT")
        .map(PathBuf::from)
        .unwrap_or_else(|| Path::new(env!("CARGO_MANIFEST_DIR")).join("../.."))
        .canonicalize()
        .unwrap_or_else(|_| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../.."))
}
fn unavailable(message: String) -> Diagnostics {
    Diagnostics {
        platform: Platform {
            desktop: std::env::var("XDG_CURRENT_DESKTOP").unwrap_or_default(),
            session: std::env::var("XDG_SESSION_TYPE").unwrap_or_default(),
            backend: "unavailable".into(),
            available: false,
            message,
        },
        monitors: vec![],
    }
}
fn renderer_python() -> std::ffi::OsString {
    std::env::var_os("WALL_IN_PYTHON").unwrap_or_else(|| {
        if Path::new("/usr/bin/python3").is_file() {
            "/usr/bin/python3".into()
        } else {
            "python3".into()
        }
    })
}

pub async fn diagnostics(helper: &Path) -> Diagnostics {
    let result = timeout(
        Duration::from_secs(8),
        Command::new(renderer_python())
            .arg(helper)
            .arg("--diagnostics")
            .kill_on_drop(true)
            .output(),
    )
    .await;
    match result {
        Ok(Ok(output)) if output.status.success() => serde_json::from_slice(&output.stdout)
            .unwrap_or_else(|e| unavailable(format!("Invalid renderer diagnostics: {e}"))),
        Ok(Ok(output)) => unavailable(format!(
            "Renderer diagnostics failed: {}",
            String::from_utf8_lossy(&output.stderr)
                .chars()
                .take(1024)
                .collect::<String>()
        )),
        Ok(Err(error)) => unavailable(format!("Cannot start renderer diagnostics: {error}")),
        Err(_) => unavailable("Renderer diagnostics timed out".into()),
    }
}

fn renderer_command(helper: &Path, path: &Path, kind: Kind, settings: &Settings) -> Command {
    let mut command = Command::new(renderer_python());
    command
        .arg(helper)
        .arg("--file")
        .arg(path)
        .arg("--kind")
        .arg(kind.as_str())
        .arg("--monitor")
        .arg(&settings.monitor)
        .arg("--fit")
        .arg(settings.fit.as_str())
        .arg("--volume")
        .arg(settings.volume.to_string());
    if settings.muted {
        command.arg("--muted");
    }
    if !settings.pause_on_battery {
        command.arg("--no-pause-on-battery");
    }
    if !settings.pause_on_fullscreen {
        command.arg("--no-pause-on-fullscreen");
    }
    command
}

fn configuration(settings: &Settings) -> serde_json::Value {
    serde_json::json!({
        "monitor": settings.monitor,
        "fit": settings.fit,
        "volume": settings.volume,
        "muted": settings.muted,
        "pause_on_battery": settings.pause_on_battery,
        "pause_on_fullscreen": settings.pause_on_fullscreen,
    })
}

pub struct Renderer {
    child: Option<Child>,
    readers: Vec<JoinHandle<()>>,
    output: Arc<Mutex<Vec<u8>>>,
    ready: Arc<AtomicBool>,
    pub playback: Playback,
}
impl Default for Renderer {
    fn default() -> Self {
        Self {
            child: None,
            readers: vec![],
            output: Arc::new(Mutex::new(Vec::new())),
            playback: Playback::default(),
            ready: Arc::new(AtomicBool::new(false)),
        }
    }
}
impl Renderer {
    fn capture(&mut self, mut reader: impl AsyncRead + Send + Unpin + 'static, protocol: bool) {
        let output = self.output.clone();
        let ready = self.ready.clone();
        self.readers.push(tokio::spawn(async move {
            let mut line = Vec::new();
            let mut bytes = [0u8; 1024];
            loop {
                match reader.read(&mut bytes).await {
                    Ok(0) | Err(_) => break,
                    Ok(n) => {
                        if protocol {
                            for byte in &bytes[..n] {
                                if *byte == b'\n' {
                                    if serde_json::from_slice::<serde_json::Value>(&line)
                                        .is_ok_and(|v| v["event"] == "ready")
                                    {
                                        ready.store(true, Ordering::Release);
                                    }
                                    line.clear();
                                } else if line.len() < 65536 {
                                    line.push(*byte);
                                }
                            }
                        }
                        let mut buffer = output.lock().unwrap_or_else(|e| e.into_inner());
                        buffer.extend_from_slice(&bytes[..n]);
                        if buffer.len() > 8192 {
                            let excess = buffer.len() - 8192;
                            buffer.drain(..excess);
                        }
                    }
                }
            }
        }));
    }
    pub fn refresh(&mut self) {
        let exited = match self.child.as_mut() {
            Some(child) => match child.try_wait() {
                Ok(Some(status)) => Some(format!("Renderer exited unexpectedly ({status})")),
                Err(e) => Some(format!("Cannot inspect renderer: {e}")),
                Ok(None) => None,
            },
            None => None,
        };
        if let Some(mut message) = exited {
            self.child.take();
            let output = self.output.lock().unwrap_or_else(|e| e.into_inner());
            if !output.is_empty() {
                message.push_str(": ");
                message.push_str(String::from_utf8_lossy(&output).trim());
            }
            self.playback.status = "stopped".into();
            self.playback.error = Some(message);
            for reader in self.readers.drain(..) {
                reader.abort();
            }
        }
    }
    pub async fn apply(
        &mut self,
        helper: &Path,
        path: &Path,
        kind: Kind,
        id: &str,
        settings: &Settings,
    ) -> Result<(), Error> {
        if kind == Kind::Scene {
            crate::import::validate_scene(path)?;
        }
        self.stop().await;
        let mut command = renderer_command(helper, path, kind, settings);
        command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        let mut child = command
            .spawn()
            .map_err(|e| Error::unavailable(format!("Cannot start renderer: {e}")))?;
        self.output
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clear();
        self.ready.store(false, Ordering::Release);
        if let Some(stdout) = child.stdout.take() {
            self.capture(stdout, true);
        }
        if let Some(stderr) = child.stderr.take() {
            self.capture(stderr, false);
        }
        self.child = Some(child);
        self.playback.wallpaper_id = Some(id.into());
        self.playback.error = None;
        let deadline = tokio::time::Instant::now() + Duration::from_secs(20);
        loop {
            tokio::time::sleep(Duration::from_millis(50)).await;
            self.refresh();
            if let Some(error) = &self.playback.error {
                return Err(Error::unavailable(error.clone()));
            }
            if self.ready.load(Ordering::Acquire) {
                break;
            }
            if tokio::time::Instant::now() >= deadline {
                self.stop().await;
                self.playback.wallpaper_id = Some(id.into());
                self.playback.error =
                    Some("Renderer did not confirm background attachment within 20 seconds".into());
                return Err(Error::unavailable(self.playback.error.clone().unwrap()));
            }
        }
        self.playback.status = "playing".into();
        Ok(())
    }
    pub async fn control(
        &mut self,
        action: &str,
        settings: Option<&Settings>,
    ) -> Result<(), Error> {
        self.refresh();
        let child = self
            .child
            .as_mut()
            .ok_or_else(|| Error::conflict("No active renderer"))?;
        let mut value = settings
            .map(configuration)
            .unwrap_or_else(|| serde_json::json!({}));
        value["action"] = action.into();
        let mut bytes = serde_json::to_vec(&value)?;
        bytes.push(b'\n');
        let stdin = child
            .stdin
            .as_mut()
            .ok_or_else(|| Error::unavailable("Renderer control channel is closed"))?;
        match timeout(Duration::from_secs(2), stdin.write_all(&bytes)).await {
            Ok(Ok(())) => {}
            _ => {
                self.stop().await;
                self.playback.error = Some("Renderer control channel failed".into());
                return Err(Error::unavailable("Renderer control channel failed"));
            }
        }
        if action == "pause" {
            self.playback.status = "paused".into();
        }
        if action == "resume" {
            self.playback.status = "playing".into();
        }
        Ok(())
    }
    pub async fn stop(&mut self) {
        if let Some(mut child) = self.child.take() {
            if let Some(mut stdin) = child.stdin.take() {
                let _ = timeout(
                    Duration::from_millis(500),
                    stdin.write_all(b"{\"action\":\"stop\"}\n"),
                )
                .await;
            }
            if timeout(Duration::from_secs(3), child.wait()).await.is_err() {
                let _ = child.kill().await;
                let _ = child.wait().await;
            }
        }
        for reader in self.readers.drain(..) {
            reader.abort();
        }
        self.playback = Playback::default();
    }
}
impl Drop for Renderer {
    fn drop(&mut self) {
        for reader in &self.readers {
            reader.abort();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn launch_flags_match_pause_policies() {
        let settings = Settings::default();
        let args = |settings: &Settings| {
            renderer_command(
                Path::new("renderer.py"),
                Path::new("media.mp4"),
                Kind::Video,
                settings,
            )
            .as_std()
            .get_args()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect::<Vec<_>>()
        };
        assert!(
            !args(&settings)
                .iter()
                .any(|arg| arg.starts_with("--no-pause"))
        );
        let mut disabled = settings;
        disabled.pause_on_battery = false;
        disabled.pause_on_fullscreen = false;
        let flags = args(&disabled);
        assert!(flags.contains(&"--no-pause-on-battery".to_string()));
        assert!(flags.contains(&"--no-pause-on-fullscreen".to_string()));
    }

    #[test]
    fn configure_sends_both_pause_policies_as_booleans() {
        let settings = Settings {
            pause_on_battery: false,
            ..Settings::default()
        };
        let value = configuration(&settings);
        assert_eq!(value["pause_on_battery"], false);
        assert_eq!(value["pause_on_fullscreen"], true);
        assert_eq!(value["muted"], settings.muted);
        assert_eq!(value["monitor"], settings.monitor);
    }
}
