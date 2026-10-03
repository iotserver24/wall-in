use crate::{
    Error,
    models::{Library, Monitor},
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    path::{Path, PathBuf},
    process::Stdio,
    time::Duration,
};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    process::{Child, ChildStdin, ChildStdout, Command},
    time::timeout,
};
use wall_in_rainmeter::{Meter, inspect_skin};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WidgetDiagnostic {
    pub section: String,
    pub code: String,
    pub message: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TextMeter {
    pub id: String,
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
    pub text: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub clock_format: Option<String>,
    pub color: String,
    pub font_size: i32,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Widget {
    pub id: String,
    pub root: PathBuf,
    pub ini: String,
    pub enabled: bool,
    pub monitor: String,
    pub x: i32,
    pub y: i32,
    pub meters: Vec<TextMeter>,
    pub diagnostics: Vec<WidgetDiagnostic>,
}

fn integer(value: f32) -> Result<i32, Error> {
    if !value.is_finite() || value < -100_000.0 || value > 100_000.0 {
        return Err(Error::bad("Widget geometry is outside supported bounds"));
    }
    Ok(value.round() as i32)
}
pub fn import(root: &Path, ini: &str, monitor: String) -> Result<Widget, Error> {
    let report =
        inspect_skin(root, ini).map_err(|e| Error::bad(format!("Cannot inspect skin: {e}")))?;
    let root = root.canonicalize().map_err(|e| Error::bad(e.to_string()))?;
    let mut hash = Sha256::new();
    hash.update(root.as_os_str().as_encoded_bytes());
    hash.update([0]);
    hash.update(ini.as_bytes());
    let id = format!("skin_{}", &format!("{:x}", hash.finalize())[..32]);
    let mut diagnostics: Vec<_> = report
        .diagnostics
        .into_iter()
        .map(|d| WidgetDiagnostic {
            section: d.section,
            code: d.code.into(),
            message: d.message,
        })
        .collect();
    let mut meters = Vec::new();
    for (index, meter) in report.meters.into_iter().enumerate() {
        match meter {
            Meter::Text {
                id: section,
                placement,
                text,
                clock_format,
                font_size,
                color,
                ..
            } => {
                if text.len() > 2048
                    || text.chars().any(|c| c.is_control() && c != '\n')
                    || !(8.0..=96.0).contains(&font_size)
                {
                    diagnostics.push(WidgetDiagnostic {
                        section,
                        code: "unsupported_text".into(),
                        message: "Text or font size exceeds native widget limits".into(),
                    });
                    continue;
                }
                let width = integer(placement.width.unwrap_or(200.0))?;
                let height = integer(placement.height.unwrap_or(40.0))?;
                if width < 1 || height < 1 {
                    diagnostics.push(WidgetDiagnostic {
                        section,
                        code: "unsupported_geometry".into(),
                        message: "Text meter has zero dimensions".into(),
                    });
                    continue;
                }
                meters.push(TextMeter {
                    id: format!("{id}_{index}"),
                    x: integer(placement.x)?,
                    y: integer(placement.y)?,
                    width,
                    height,
                    text,
                    clock_format,
                    color: format!(
                        "#{:02x}{:02x}{:02x}{:02x}",
                        color[0], color[1], color[2], color[3]
                    ),
                    font_size: integer(font_size)?,
                });
            }
            Meter::Image { id: section, .. } | Meter::Shape { id: section, .. } => diagnostics
                .push(WidgetDiagnostic {
                    section,
                    code: "unsupported_native_meter".into(),
                    message: "Image and Shape meters are not rendered by the native widget bridge"
                        .into(),
                }),
        }
    }
    if meters.is_empty() {
        return Err(Error::bad(format!(
            "Skin contains no native-compatible Text meters; diagnostics: {}",
            diagnostics
                .iter()
                .map(|d| format!("{}: {}", d.section, d.message))
                .collect::<Vec<_>>()
                .join("; ")
        )));
    }
    if meters.len() > 32 {
        return Err(Error::bad("Skin exceeds the 32 native widgets limit"));
    }
    Ok(Widget {
        id,
        root,
        ini: ini.into(),
        enabled: false,
        monitor,
        x: 0,
        y: 0,
        meters,
        diagnostics,
    })
}

#[derive(Serialize)]
struct NativeWidget<'a> {
    id: &'a str,
    monitor: &'a str,
    x: i32,
    y: i32,
    width: i32,
    height: i32,
    text: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    clock_format: Option<&'a str>,
    color: &'a str,
    font_size: i32,
}
pub fn document(library: &Library, monitors: &[Monitor]) -> Result<serde_json::Value, Error> {
    let mut result = Vec::new();
    for widget in library.widgets.iter().filter(|w| w.enabled) {
        let monitor = monitors
            .iter()
            .find(|m| m.id == widget.monitor)
            .ok_or_else(|| {
                Error::unavailable(format!(
                    "Widget {} monitor {} is not connected",
                    widget.id, widget.monitor
                ))
            })?;
        for meter in &widget.meters {
            let x = widget
                .x
                .checked_add(meter.x)
                .ok_or_else(|| Error::bad("Widget position overflow"))?;
            let y = widget
                .y
                .checked_add(meter.y)
                .ok_or_else(|| Error::bad("Widget position overflow"))?;
            if x < 0
                || y < 0
                || meter.width < 1
                || meter.height < 1
                || i64::from(x) + i64::from(meter.width) > i64::from(monitor.width)
                || i64::from(y) + i64::from(meter.height) > i64::from(monitor.height)
                || meter.width > 4096
                || meter.height > 4096
            {
                return Err(Error::bad(format!(
                    "Widget {} is outside monitor bounds",
                    widget.id
                )));
            }
            result.push(NativeWidget {
                id: &meter.id,
                monitor: &widget.monitor,
                x,
                y,
                width: meter.width,
                height: meter.height,
                text: &meter.text,
                clock_format: meter.clock_format.as_deref(),
                color: &meter.color,
                font_size: meter.font_size,
            });
        }
    }
    if result.len() > 32 {
        return Err(Error::bad("At most 32 native Text meters may be enabled"));
    }
    Ok(serde_json::json!({"widgets": result}))
}

#[derive(Default)]
pub struct Bridge {
    child: Option<Child>,
    stdin: Option<ChildStdin>,
    stdout: Option<BufReader<ChildStdout>>,
    pub error: Option<String>,
}
impl Bridge {
    pub fn refresh(&mut self) {
        if let Some(child) = self.child.as_mut() {
            match child.try_wait() {
                Ok(Some(status)) => {
                    self.error = Some(format!(
                        "Widget helper exited ({status}); widgets are not displayed"
                    ));
                    self.child = None;
                    self.stdin = None;
                    self.stdout = None;
                }
                Err(e) => {
                    self.error = Some(format!("Cannot inspect widget helper: {e}"));
                    self.child = None;
                    self.stdin = None;
                    self.stdout = None;
                }
                Ok(None) => {}
            }
        }
    }
    async fn line(&mut self, expected: &str) -> Result<(), Error> {
        let mut line = String::new();
        let read = timeout(
            Duration::from_secs(8),
            self.stdout.as_mut().unwrap().read_line(&mut line),
        )
        .await
        .map_err(|_| Error::unavailable("Widget helper response timed out"))?
        .map_err(|e| Error::unavailable(format!("Widget helper read failed: {e}")))?;
        if read == 0 || line.len() > 65536 {
            return Err(Error::unavailable(
                "Widget helper closed or sent oversized response",
            ));
        }
        let response: serde_json::Value = serde_json::from_str(&line)
            .map_err(|_| Error::unavailable("Invalid widget helper response"))?;
        if response["event"] == expected {
            return Ok(());
        }
        Err(Error::unavailable(
            response["message"]
                .as_str()
                .unwrap_or("Unexpected widget helper response"),
        ))
    }
    async fn start(&mut self, helper: &Path) -> Result<(), Error> {
        let python = std::env::var_os("WALL_IN_PYTHON").unwrap_or_else(|| {
            if Path::new("/usr/bin/python3").is_file() {
                "/usr/bin/python3".into()
            } else {
                "python3".into()
            }
        });
        let mut child = Command::new(python)
            .arg(helper)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .kill_on_drop(true)
            .spawn()
            .map_err(|e| Error::unavailable(format!("Cannot start widget helper: {e}")))?;
        self.stdin = child.stdin.take();
        self.stdout = child.stdout.take().map(BufReader::new);
        self.child = Some(child);
        if let Err(e) = self.line("ready").await {
            self.stop().await;
            return Err(e);
        }
        Ok(())
    }
    pub async fn apply(
        &mut self,
        helper: &Path,
        document: &serde_json::Value,
    ) -> Result<(), Error> {
        self.refresh();
        if self.child.is_none() && document["widgets"].as_array().is_some_and(Vec::is_empty) {
            self.error = None;
            return Ok(());
        }
        self.error = None;
        let result = self.apply_inner(helper, document).await;
        if let Err(e) = &result {
            self.error = Some(e.message.clone());
            self.stop().await;
        } else {
            self.error = None;
        }
        result
    }
    async fn apply_inner(
        &mut self,
        helper: &Path,
        document: &serde_json::Value,
    ) -> Result<(), Error> {
        if self.child.is_none() {
            self.start(helper).await?;
        }
        let command = serde_json::json!({"action":"set","widgets":document["widgets"]});
        let mut bytes = serde_json::to_vec(&command)?;
        bytes.push(b'\n');
        timeout(Duration::from_secs(8), async {
            let stdin = self.stdin.as_mut().unwrap();
            stdin.write_all(&bytes).await?;
            stdin.flush().await
        })
        .await
        .map_err(|_| Error::unavailable("Widget helper write timed out"))?
        .map_err(|e| Error::unavailable(format!("Widget helper write failed: {e}")))?;
        self.line("applied").await?;
        self.refresh();
        if let Some(message) = &self.error {
            return Err(Error::unavailable(message.clone()));
        }
        Ok(())
    }
    pub async fn stop(&mut self) {
        if let Some(mut stdin) = self.stdin.take() {
            let _ = timeout(
                Duration::from_secs(1),
                stdin.write_all(b"{\"action\":\"stop\"}\n"),
            )
            .await;
        }
        if let Some(mut child) = self.child.take()
            && timeout(Duration::from_secs(2), child.wait()).await.is_err()
        {
            let _ = child.kill().await;
        }
        self.stdout = None;
    }
}
