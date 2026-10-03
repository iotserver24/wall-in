use crate::{
    Error, autostart,
    import::{checked_resource, import_path, validate_scene, validate_tags},
    models::{Diagnostics, Entry, Kind, Library, Settings, State},
    platform::{self, Renderer},
    steam,
    store::Store,
    widgets::{self, Bridge},
};
use axum::{
    Json, Router,
    body::Body,
    extract::{
        DefaultBodyLimit, Path, Query, Request, State as AppState, rejection::JsonRejection,
    },
    http::{HeaderValue, StatusCode, header},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::{get, patch, post},
};
use serde::Deserialize;
use serde_json::Value;
use std::{
    path::PathBuf,
    sync::Arc,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use tokio::sync::Mutex;
use tower::ServiceExt;
use tower_http::services::{ServeDir, ServeFile};

pub type HttpRouter = Router;
pub async fn serve(
    listener: tokio::net::TcpListener,
    app: Router,
    shutdown: impl std::future::Future<Output = ()> + Send + 'static,
) -> std::io::Result<()> {
    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown)
        .await
}
type Shared = Arc<Mutex<Service>>;
pub struct Service {
    store: Store,
    pub library: Library,
    pub renderer: Renderer,
    pub diagnostics: Diagnostics,
    helper: PathBuf,
    widget_bridge: Bridge,
    rotation_deadline: Option<Instant>,
    shuffle_queue: Vec<String>,
}
impl Service {
    pub async fn open(data_dir: PathBuf, helper: PathBuf) -> Result<Shared, Error> {
        let (store, mut library) = Store::open(data_dir)?;
        let diagnostics = platform::diagnostics(&helper).await;
        if library.settings.monitor == "all"
            && let Some(monitor) = diagnostics.monitors.first()
        {
            library.settings.monitor = monitor.id.clone();
        }
        let mut service = Self {
            store,
            library,
            renderer: Renderer::default(),
            diagnostics,
            helper,
            widget_bridge: Bridge::default(),
            rotation_deadline: None,
            shuffle_queue: Vec::new(),
        };
        if let Some(id) = service.library.desired_wallpaper_id.clone()
            && let Err(error) = service
                .start_wallpaper(&id, service.library.desired_paused)
                .await
        {
            service.renderer.stop().await;
            service.renderer.playback.wallpaper_id = Some(id);
            service.renderer.playback.error = Some(format!("Cannot restore wallpaper: {error}"));
        }
        if service.library.widgets.iter().any(|widget| widget.enabled)
            && let Err(error) = service.apply_widgets(&service.library.clone()).await
        {
            service.widget_bridge.error = Some(format!("Cannot restore widgets: {error}"));
        }
        service.reset_rotation();
        Ok(Arc::new(Mutex::new(service)))
    }
    fn reset_rotation(&mut self) {
        self.shuffle_queue.clear();
        self.rotation_deadline = if self.renderer.playback.status == "playing"
            && self.library.desired_wallpaper_id.is_some()
            && !self.library.settings.playlist_ids.is_empty()
        {
            Some(Instant::now() + Duration::from_secs(self.library.settings.playlist_interval))
        } else {
            None
        };
    }
    fn next_wallpaper(&mut self) -> Option<String> {
        let ids = &self.library.settings.playlist_ids;
        let current = self.library.desired_wallpaper_id.as_deref();
        if ids.is_empty() {
            return None;
        }
        if !self.library.settings.shuffle {
            return Some(
                ids.iter()
                    .position(|id| Some(id.as_str()) == current)
                    .map(|index| &ids[(index + 1) % ids.len()])
                    .unwrap_or(&ids[0])
                    .clone(),
            );
        }
        if self.shuffle_queue.is_empty() {
            self.shuffle_queue = ids
                .iter()
                .filter(|id| ids.len() == 1 || Some(id.as_str()) != current)
                .cloned()
                .collect();
            let mut seed = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos() as u64;
            for i in (1..self.shuffle_queue.len()).rev() {
                seed ^= seed << 13;
                seed ^= seed >> 7;
                seed ^= seed << 17;
                self.shuffle_queue.swap(i, (seed as usize) % (i + 1));
            }
        }
        self.shuffle_queue.pop()
    }
    async fn rotate_if_due(&mut self) {
        self.renderer.refresh();
        if self.renderer.playback.status != "playing" {
            self.rotation_deadline = None;
            return;
        }
        if !self
            .rotation_deadline
            .is_some_and(|deadline| Instant::now() >= deadline)
        {
            return;
        }
        self.rotation_deadline = None;
        let Some(id) = self.next_wallpaper() else {
            return;
        };
        if self.library.desired_wallpaper_id.as_deref() == Some(id.as_str()) {
            self.rotation_deadline =
                Some(Instant::now() + Duration::from_secs(self.library.settings.playlist_interval));
            return;
        }
        if let Err(error) = self.start_wallpaper(&id, false).await {
            self.renderer.playback.error = Some(format!("Cannot rotate playlist: {error}"));
            if self.renderer.playback.status == "playing" {
                self.rotation_deadline = Some(
                    Instant::now() + Duration::from_secs(self.library.settings.playlist_interval),
                );
            }
            return;
        }
        if self.save_assignment(id, false).await.is_ok() {
            self.rotation_deadline =
                Some(Instant::now() + Duration::from_secs(self.library.settings.playlist_interval));
        }
    }
    pub fn snapshot(&mut self) -> State {
        self.renderer.refresh();
        self.widget_bridge.refresh();
        let mut playback = self.renderer.playback.clone();
        if playback.status == "stopped"
            && playback.error.is_none()
            && let Some(id) = &self.library.desired_wallpaper_id
        {
            playback.wallpaper_id = Some(id.clone());
            playback.status = "pending".into();
        }
        State {
            wallpapers: self
                .library
                .entries
                .iter()
                .map(|e| e.wallpaper.clone())
                .collect(),
            settings: self.library.settings.clone(),
            playback,
            platform: self.diagnostics.platform.clone(),
            monitors: self.diagnostics.monitors.clone(),
            steam_paths: self.library.steam_paths.clone(),
            widgets: self.library.widgets.clone(),
            widget_error: self.widget_bridge.error.clone(),
        }
    }
    fn renderer_settings(&self, settings: &Settings) -> Result<Settings, Error> {
        let mut settings = settings.clone();
        if settings.monitor == "all" || settings.monitor == "primary" {
            settings.monitor = self
                .diagnostics
                .monitors
                .first()
                .ok_or_else(|| Error::unavailable("No connected monitor is available"))?
                .id
                .clone();
        }
        if !self
            .diagnostics
            .monitors
            .iter()
            .any(|m| m.id == settings.monitor)
        {
            return Err(Error::bad("Selected monitor is not connected"));
        }
        Ok(settings)
    }
    async fn start_wallpaper(&mut self, id: &str, paused: bool) -> Result<(), Error> {
        let entry = self
            .library
            .entries
            .iter()
            .find(|e| e.wallpaper.id == id)
            .cloned()
            .ok_or_else(|| Error::not_found("Unknown wallpaper"))?;
        if !entry.wallpaper.supported || !entry.wallpaper.kind.supported() {
            return Err(Error {
                status: StatusCode::UNPROCESSABLE_ENTITY,
                message: "This wallpaper format is not supported by the local renderer".into(),
            });
        }
        let path = checked_resource(&entry, false)?;
        if entry.wallpaper.kind == Kind::Scene {
            validate_scene(&path)?;
        }
        if !self.diagnostics.platform.available {
            return Err(Error::unavailable(
                self.diagnostics.platform.message.clone(),
            ));
        }
        let settings = self.renderer_settings(&self.library.settings)?;
        self.renderer
            .apply(&self.helper, &path, entry.wallpaper.kind, id, &settings)
            .await?;
        if paused {
            self.renderer.control("pause", None).await?;
        }
        self.renderer.refresh();
        if let Some(error) = &self.renderer.playback.error {
            return Err(Error::unavailable(error.clone()));
        }
        Ok(())
    }
    async fn save_assignment(&mut self, id: String, paused: bool) -> Result<(), Error> {
        let mut library = self.library.clone();
        library.desired_wallpaper_id = Some(id.clone());
        library.desired_paused = paused;
        if let Err(error) = self.commit(library) {
            self.renderer.stop().await;
            self.renderer.playback.wallpaper_id = Some(id);
            self.renderer.playback.error =
                Some(format!("Cannot save wallpaper assignment: {error}"));
            return Err(error);
        }
        Ok(())
    }
    fn commit(&mut self, library: Library) -> Result<(), Error> {
        self.store.save(&library)?;
        self.library = library;
        Ok(())
    }
    async fn apply_widgets(&mut self, library: &Library) -> Result<(), Error> {
        let document = widgets::document(library, &self.diagnostics.monitors)?;
        self.widget_bridge
            .apply(&self.helper.with_file_name("widgets.py"), &document)
            .await
    }
    async fn update_widgets(&mut self, library: Library) -> Result<(), Error> {
        self.apply_widgets(&library).await?;
        if let Err(error) = self.commit(library) {
            let previous = self.library.clone();
            if let Err(rollback) = self.apply_widgets(&previous).await {
                return Err(Error::unavailable(format!(
                    "Cannot save widgets: {error}; cannot restore displayed widgets: {rollback}"
                )));
            }
            return Err(error);
        }
        Ok(())
    }
    pub async fn shutdown(&mut self) {
        self.widget_bridge.stop().await;
        self.renderer.stop().await;
    }
}
fn merge_entry(library: &mut Library, mut entry: Entry) -> Result<(), Error> {
    if let Some(existing) = library
        .entries
        .iter_mut()
        .find(|e| e.wallpaper.id == entry.wallpaper.id)
    {
        entry.wallpaper.favorite = existing.wallpaper.favorite;
        entry.wallpaper.tags = existing.wallpaper.tags.clone();
        entry.wallpaper.title = existing.wallpaper.title.clone();
        *existing = entry;
    } else {
        if library.entries.len() >= 10000 {
            return Err(Error::bad("Library is limited to 10000 wallpapers"));
        }
        library.entries.push(entry);
    }
    Ok(())
}
fn json<T>(input: Result<Json<T>, JsonRejection>) -> Result<T, Error> {
    input.map(|Json(value)| value).map_err(|rejection| Error {
        status: rejection.status(),
        message: rejection.body_text(),
    })
}

pub fn router(shared: Shared, frontend: PathBuf, port: u16) -> Router {
    Router::new()
        .route(
            "/api/health",
            get(|| async { Json(serde_json::json!({"status": "ok"})) }),
        )
        .route("/api", get(api_not_found))
        .route("/api/state", get(state))
        .route("/api/import", post(import))
        .route("/api/widgets", get(list_widgets))
        .route("/api/widgets/import", post(import_widget))
        .route(
            "/api/widgets/{id}",
            patch(update_widget).delete(delete_widget),
        )
        .route("/api/scan-steam", post(scan))
        .route(
            "/api/wallpapers/{id}",
            patch(update_wallpaper).delete(delete_wallpaper),
        )
        .route("/api/settings", patch(settings))
        .route("/api/playback", post(playback))
        .route("/api/media/{id}", get(media))
        .route(
            "/api/{*path}",
            get(api_not_found)
                .post(api_not_found)
                .patch(api_not_found)
                .delete(api_not_found),
        )
        .fallback_service(
            ServeDir::new(&frontend)
                .append_index_html_on_directories(true)
                .not_found_service(ServeFile::new(frontend.join("index.html"))),
        )
        .method_not_allowed_fallback(|| async {
            Error {
                status: StatusCode::METHOD_NOT_ALLOWED,
                message: "Method not allowed".into(),
            }
        })
        .layer(DefaultBodyLimit::max(64 * 1024))
        .layer(middleware::from_fn(move |request, next| {
            security(request, next, port)
        }))
        .with_state(shared)
}
async fn api_not_found() -> Error {
    Error::not_found("Unknown API endpoint")
}
fn local_authority(authority: &str, port: u16) -> bool {
    let Ok(authority) = authority.parse::<axum::http::uri::Authority>() else {
        return false;
    };
    matches!(
        authority.host(),
        "localhost" | "127.0.0.1" | "[::1]" | "::1"
    ) && authority.port_u16().unwrap_or(80) == port
        && !authority.as_str().contains('@')
}
async fn security(request: Request, next: Next, port: u16) -> Response {
    let hosts: Vec<_> = request.headers().get_all(header::HOST).iter().collect();
    if hosts.len() != 1 || !hosts[0].to_str().is_ok_and(|h| local_authority(h, port)) {
        return Error {
            status: StatusCode::FORBIDDEN,
            message: "Only loopback Host headers are accepted".into(),
        }
        .into_response();
    }
    for origin in request.headers().get_all(header::ORIGIN) {
        let allowed = origin
            .to_str()
            .ok()
            .and_then(|o| o.parse::<axum::http::Uri>().ok())
            .is_some_and(|uri| {
                uri.scheme_str() == Some("http")
                    && uri
                        .authority()
                        .is_some_and(|a| local_authority(a.as_str(), port))
                    && uri.path() == "/"
                    && uri.query().is_none()
            });
        if !allowed {
            return Error {
                status: StatusCode::FORBIDDEN,
                message: "Foreign browser Origin rejected".into(),
            }
            .into_response();
        }
    }
    if request
        .headers()
        .get("sec-fetch-site")
        .is_some_and(|v| v == "cross-site")
    {
        return Error {
            status: StatusCode::FORBIDDEN,
            message: "Cross-site requests are not allowed".into(),
        }
        .into_response();
    }
    let api_request = request.uri().path().starts_with("/api/") || request.uri().path() == "/api";
    let mut response = next.run(request).await;
    if api_request && !response.headers().contains_key(header::CACHE_CONTROL) {
        response
            .headers_mut()
            .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    }
    if api_request
        && (response.status().is_client_error() || response.status().is_server_error())
        && !response
            .headers()
            .get(header::CONTENT_TYPE)
            .is_some_and(|v| v == "application/json")
    {
        let (mut parts, body) = response.into_parts();
        let fallback = parts.status.canonical_reason().unwrap_or("Request failed");
        let message = axum::body::to_bytes(body, 8192)
            .await
            .ok()
            .and_then(|b| String::from_utf8(b.to_vec()).ok())
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| fallback.into());
        parts.headers.insert(
            header::CONTENT_TYPE,
            HeaderValue::from_static("application/json"),
        );
        parts.headers.remove(header::CONTENT_LENGTH);
        response = Response::from_parts(
            parts,
            Body::from(serde_json::json!({"error":message}).to_string()),
        );
    }
    response.headers_mut().insert(
        header::X_CONTENT_TYPE_OPTIONS,
        HeaderValue::from_static("nosniff"),
    );
    response
        .headers_mut()
        .insert("referrer-policy", HeaderValue::from_static("no-referrer"));
    response.headers_mut().insert(
        "cross-origin-resource-policy",
        HeaderValue::from_static("same-origin"),
    );
    response
        .headers_mut()
        .insert("x-frame-options", HeaderValue::from_static("DENY"));
    response
}
async fn state(
    AppState(shared): AppState<Shared>,
    Query(query): Query<std::collections::HashMap<String, String>>,
) -> Json<State> {
    if query
        .get("refresh_platform")
        .is_some_and(|value| value == "true")
    {
        let helper = shared.lock().await.helper.clone();
        let diagnostics = platform::diagnostics(&helper).await;
        let mut service = shared.lock().await;
        service.diagnostics = diagnostics;
        return Json(service.snapshot());
    }
    Json(shared.lock().await.snapshot())
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ImportRequest {
    path: String,
}
async fn import(
    AppState(shared): AppState<Shared>,
    input: Result<Json<ImportRequest>, JsonRejection>,
) -> Result<Json<State>, Error> {
    let input = json(input)?;
    if input.path.is_empty() {
        return Err(Error::bad("Import path cannot be empty"));
    }
    let entry = tokio::task::spawn_blocking(move || import_path(std::path::Path::new(&input.path)))
        .await
        .map_err(|e| Error::bad(e.to_string()))??;
    let mut service = shared.lock().await;
    let mut library = service.library.clone();
    merge_entry(&mut library, entry)?;
    service.commit(library)?;
    Ok(Json(service.snapshot()))
}
async fn list_widgets(AppState(shared): AppState<Shared>) -> Json<Vec<widgets::Widget>> {
    Json(shared.lock().await.library.widgets.clone())
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct WidgetImport {
    root: PathBuf,
    ini: String,
}
async fn import_widget(
    AppState(shared): AppState<Shared>,
    input: Result<Json<WidgetImport>, JsonRejection>,
) -> Result<Json<State>, Error> {
    let input = json(input)?;
    let monitor = shared
        .lock()
        .await
        .diagnostics
        .monitors
        .first()
        .map(|m| m.id.clone())
        .unwrap_or_default();
    let widget =
        tokio::task::spawn_blocking(move || widgets::import(&input.root, &input.ini, monitor))
            .await
            .map_err(|e| Error::bad(e.to_string()))??;
    let mut service = shared.lock().await;
    let mut library = service.library.clone();
    if let Some(existing) = library.widgets.iter_mut().find(|w| w.id == widget.id) {
        let mut updated = widget;
        updated.enabled = existing.enabled;
        updated.monitor = existing.monitor.clone();
        updated.x = existing.x;
        updated.y = existing.y;
        *existing = updated;
    } else {
        if library.widgets.len() >= 32 {
            return Err(Error::bad("Library is limited to 32 skins"));
        }
        library.widgets.push(widget);
    }
    service.update_widgets(library).await?;
    Ok(Json(service.snapshot()))
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct WidgetPatch {
    enabled: Option<bool>,
    monitor: Option<String>,
    x: Option<i32>,
    y: Option<i32>,
}
async fn update_widget(
    AppState(shared): AppState<Shared>,
    Path(id): Path<String>,
    input: Result<Json<WidgetPatch>, JsonRejection>,
) -> Result<Json<State>, Error> {
    let patch = json(input)?;
    if patch.enabled.is_none() && patch.monitor.is_none() && patch.x.is_none() && patch.y.is_none()
    {
        return Err(Error::bad("Widget patch is empty"));
    }
    let mut service = shared.lock().await;
    let mut library = service.library.clone();
    let widget = library
        .widgets
        .iter_mut()
        .find(|w| w.id == id)
        .ok_or_else(|| Error::not_found("Unknown widget"))?;
    if let Some(enabled) = patch.enabled {
        widget.enabled = enabled;
    }
    if let Some(monitor) = patch.monitor {
        if monitor.is_empty() || monitor.len() > 256 || monitor.chars().any(char::is_control) {
            return Err(Error::bad("Invalid widget monitor"));
        }
        widget.monitor = monitor;
    }
    if let Some(x) = patch.x {
        widget.x = x;
    }
    if let Some(y) = patch.y {
        widget.y = y;
    }
    service.update_widgets(library).await?;
    Ok(Json(service.snapshot()))
}
async fn delete_widget(
    AppState(shared): AppState<Shared>,
    Path(id): Path<String>,
) -> Result<Json<State>, Error> {
    let mut service = shared.lock().await;
    let mut library = service.library.clone();
    let old = library.widgets.len();
    library.widgets.retain(|w| w.id != id);
    if library.widgets.len() == old {
        return Err(Error::not_found("Unknown widget"));
    }
    service.update_widgets(library).await?;
    Ok(Json(service.snapshot()))
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Empty {}
async fn scan(
    AppState(shared): AppState<Shared>,
    input: Result<Json<Empty>, JsonRejection>,
) -> Result<Json<State>, Error> {
    json(input)?;
    let home =
        std::env::var_os("HOME").ok_or_else(|| Error::bad("HOME is unset; cannot locate Steam"))?;
    let scan = tokio::task::spawn_blocking(move || steam::scan(std::path::Path::new(&home)))
        .await
        .map_err(|e| Error::bad(e.to_string()))??;
    let mut service = shared.lock().await;
    let mut library = service.library.clone();
    library.steam_paths = scan.paths;
    for entry in scan.entries {
        merge_entry(&mut library, entry)?;
    }
    service.commit(library)?;
    Ok(Json(service.snapshot()))
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct WallpaperPatch {
    favorite: Option<bool>,
    tags: Option<Vec<String>>,
    title: Option<String>,
}
async fn update_wallpaper(
    AppState(shared): AppState<Shared>,
    Path(id): Path<String>,
    input: Result<Json<WallpaperPatch>, JsonRejection>,
) -> Result<Json<State>, Error> {
    let patch = json(input)?;
    let mut service = shared.lock().await;
    let mut library = service.library.clone();
    let wallpaper = &mut library
        .entries
        .iter_mut()
        .find(|e| e.wallpaper.id == id)
        .ok_or_else(|| Error::not_found("Unknown wallpaper"))?
        .wallpaper;
    if let Some(favorite) = patch.favorite {
        wallpaper.favorite = favorite;
    }
    if let Some(tags) = patch.tags {
        validate_tags(&tags)?;
        wallpaper.tags = tags;
    }
    if let Some(title) = patch.title {
        if title.trim().is_empty() || title.len() > 512 || title.chars().any(char::is_control) {
            return Err(Error::bad(
                "Title must contain 1–512 bytes without control characters",
            ));
        }
        wallpaper.title = title;
    }
    service.commit(library)?;
    Ok(Json(service.snapshot()))
}
async fn delete_wallpaper(
    AppState(shared): AppState<Shared>,
    Path(id): Path<String>,
) -> Result<Json<State>, Error> {
    let mut service = shared.lock().await;
    let mut library = service.library.clone();
    let before = library.entries.len();
    library.entries.retain(|e| e.wallpaper.id != id);
    if before == library.entries.len() {
        return Err(Error::not_found("Unknown wallpaper"));
    }
    library.settings.playlist_ids.retain(|i| i != &id);
    if library.desired_wallpaper_id.as_ref() == Some(&id) {
        library.desired_wallpaper_id = None;
        library.desired_paused = false;
    }
    service.commit(library)?;
    service.reset_rotation();
    if service.renderer.playback.wallpaper_id.as_ref() == Some(&id) {
        service.renderer.stop().await;
    }
    Ok(Json(service.snapshot()))
}
async fn settings(
    AppState(shared): AppState<Shared>,
    input: Result<Json<Value>, JsonRejection>,
) -> Result<Json<State>, Error> {
    let patch = json(input)?;
    let patch = patch
        .as_object()
        .ok_or_else(|| Error::bad("Settings patch must be an object"))?;
    let mut service = shared.lock().await;
    let mut value = serde_json::to_value(&service.library.settings)?;
    for (key, item) in patch {
        value[key] = item.clone();
    }
    let settings: Settings = serde_json::from_value(value)?;
    settings.validate()?;
    let mut seen = std::collections::HashSet::new();
    for id in &settings.playlist_ids {
        if !seen.insert(id)
            || !service.library.entries.iter().any(|e| {
                &e.wallpaper.id == id && e.wallpaper.supported && e.wallpaper.kind.supported()
            })
        {
            return Err(Error::bad(format!(
                "Playlist contains an unknown or unsupported wallpaper: {id}"
            )));
        }
    }
    service.renderer.refresh();
    if service.renderer.playback.status != "stopped" {
        let renderer_settings = service.renderer_settings(&settings)?;
        service
            .renderer
            .control("configure", Some(&renderer_settings))
            .await?;
    }
    let previous = service.library.settings.clone();
    if previous.autostart != settings.autostart {
        let root = service
            .helper
            .parent()
            .filter(|p| p.file_name().is_some_and(|name| name == "integrations"))
            .and_then(|p| p.parent())
            .ok_or_else(|| {
                Error::bad("Autostart requires the renderer under WALL_IN_ROOT/integrations")
            })?
            .to_path_buf();
        let data_dir = service.store.dir().to_path_buf();
        let enabled = settings.autostart;
        tokio::task::spawn_blocking(move || autostart::set(enabled, &root, &data_dir))
            .await
            .map_err(|e| Error::unavailable(e.to_string()))??;
    }
    let mut library = service.library.clone();
    library.settings = settings.clone();
    if let Err(error) = service.commit(library) {
        if previous.autostart != settings.autostart {
            let root = service
                .helper
                .parent()
                .unwrap()
                .parent()
                .unwrap()
                .to_path_buf();
            let data_dir = service.store.dir().to_path_buf();
            let rollback = tokio::task::spawn_blocking(move || {
                autostart::set(previous.autostart, &root, &data_dir)
            })
            .await
            .map_err(|e| Error::unavailable(e.to_string()))?;
            if let Err(rollback) = rollback {
                return Err(Error::unavailable(format!(
                    "Cannot save settings: {error}; cannot roll back autostart: {rollback}"
                )));
            }
        }
        return Err(error);
    }
    if previous.playlist_ids != settings.playlist_ids
        || previous.playlist_interval != settings.playlist_interval
        || previous.shuffle != settings.shuffle
    {
        service.reset_rotation();
    }
    Ok(Json(service.snapshot()))
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PlaybackRequest {
    action: String,
    wallpaper_id: Option<String>,
}
async fn playback(
    AppState(shared): AppState<Shared>,
    input: Result<Json<PlaybackRequest>, JsonRejection>,
) -> Result<Json<State>, Error> {
    let input = json(input)?;
    let mut service = shared.lock().await;
    match input.action.as_str() {
        "schedule" => {
            let id = input
                .wallpaper_id
                .ok_or_else(|| Error::bad("schedule requires wallpaper_id"))?;
            let entry = service
                .library
                .entries
                .iter()
                .find(|entry| entry.wallpaper.id == id)
                .ok_or_else(|| Error::not_found("Unknown wallpaper"))?;
            if !entry.wallpaper.supported || !entry.wallpaper.kind.supported() {
                return Err(Error::bad("Only supported wallpapers can be scheduled"));
            }
            checked_resource(entry, false)?;
            if service.renderer.playback.status != "stopped" {
                return Err(Error::conflict(
                    "Stop the active wallpaper before scheduling another",
                ));
            }
            let mut library = service.library.clone();
            library.desired_wallpaper_id = Some(id);
            library.desired_paused = false;
            service.commit(library)?;
            service.reset_rotation();
        }
        "apply" => {
            let id = input
                .wallpaper_id
                .ok_or_else(|| Error::bad("apply requires wallpaper_id"))?;
            service.diagnostics = platform::diagnostics(&service.helper).await;
            service.start_wallpaper(&id, false).await?;
            service.save_assignment(id, false).await?;
            service.reset_rotation();
        }
        "pause" | "resume" => {
            service.renderer.control(&input.action, None).await?;
            let id = service
                .renderer
                .playback
                .wallpaper_id
                .clone()
                .ok_or_else(|| Error::conflict("No active wallpaper assignment"))?;
            service.save_assignment(id, input.action == "pause").await?;
            service.reset_rotation();
        }
        "stop" => {
            let mut library = service.library.clone();
            library.desired_wallpaper_id = None;
            library.desired_paused = false;
            service.commit(library)?;
            service.renderer.stop().await;
            service.reset_rotation();
        }
        _ => {
            return Err(Error::bad(
                "Unknown playback action; use apply, pause, resume or stop",
            ));
        }
    }
    Ok(Json(service.snapshot()))
}
async fn media(
    AppState(shared): AppState<Shared>,
    Path(id): Path<String>,
    Query(query): Query<std::collections::HashMap<String, String>>,
    request: Request,
) -> Result<Response, Error> {
    let path = {
        let service = shared.lock().await;
        let entry = service
            .library
            .entries
            .iter()
            .find(|e| e.wallpaper.id == id)
            .ok_or_else(|| Error::not_found("Unknown wallpaper"))?;
        checked_resource(
            entry,
            query
                .get("preview")
                .is_some_and(|v| v == "1" || v == "true"),
        )?
    };
    let response = ServeFile::new(path)
        .oneshot(request)
        .await
        .map_err(|e| Error::bad(e.to_string()))?;
    if response.status() == StatusCode::NOT_FOUND {
        return Err(Error::not_found("Wallpaper media is missing"));
    }
    let mut response = response.map(Body::new);
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-cache"));
    Ok(response)
}

pub fn supervise(shared: &Shared) -> tokio::task::JoinHandle<()> {
    let weak = Arc::downgrade(shared);
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_millis(250));
        loop {
            interval.tick().await;
            let Some(shared) = weak.upgrade() else {
                break;
            };
            shared.lock().await.rotate_if_due().await;
        }
    })
}
