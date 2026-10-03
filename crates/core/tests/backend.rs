use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use http_body_util::BodyExt;
use serde_json::{Value, json};
use std::{fs, path::Path};
use tempfile::TempDir;
use tower::ServiceExt;
use wall_in_core::{
    import::{checked_resource, import_path, validate_scene},
    models::Kind,
    service::{Service, router},
    steam::{library_paths, scan_roots},
    store::Store,
};

fn image(dir: &Path) -> std::path::PathBuf {
    let path = dir.join("wallpaper.png");
    fs::write(&path, b"0123456789").unwrap();
    path
}
#[test]
fn import_deterministic_and_classifies_formats() {
    let dir = TempDir::new().unwrap();
    let path = image(dir.path());
    let first = import_path(&path).unwrap();
    let second = import_path(&path).unwrap();
    assert_eq!(first.wallpaper.id, second.wallpaper.id);
    assert_eq!(first.wallpaper.kind, Kind::Image);
    assert!(first.wallpaper.supported);
    assert!(first.wallpaper.preview_url.is_some());
    for (extension, kind, supported) in [
        ("mp4", Kind::Video, true),
        ("html", Kind::Web, false),
        ("pkg", Kind::Scene, false),
        ("exe", Kind::Application, false),
        ("unknown", Kind::Application, false),
    ] {
        let path = dir.path().join(format!("file.{extension}"));
        fs::write(&path, b"content").unwrap();
        let entry = import_path(&path).unwrap();
        assert_eq!(entry.wallpaper.kind, kind);
        assert_eq!(entry.wallpaper.supported, supported);
        if !supported {
            assert!(entry.media.is_none());
        }
    }
}
#[test]
fn native_scene_is_strict_and_project_scene_stays_unsupported() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("cube.wallin-scene.json");
    let valid = json!({"version":1,"camera":{"fov":60,"distance":5},
        "cube":{"size":1.5,"rpm":12,"colors":["#ff0000","#00ff00","#0000ff",
            "#ffff00","#ff00ff","#00ffff"]},"background":"#101020"});
    fs::write(&path, valid.to_string()).unwrap();
    let entry = import_path(&path).unwrap();
    assert_eq!(entry.wallpaper.kind, Kind::Scene);
    assert!(entry.wallpaper.supported);
    assert_eq!(checked_resource(&entry, false).unwrap(), path);
    for invalid in [
        json!({"version":2,"camera":valid["camera"],"cube":valid["cube"],"background":"#101020"}),
        json!({"version":1,"camera":{"fov":120,"distance":5},"cube":valid["cube"],"background":"#101020"}),
        json!({"version":1,"camera":valid["camera"],"cube":valid["cube"],"background":"#101020","shader":"evil"}),
        json!({"version":1,"camera":valid["camera"],"cube":{"size":1,"rpm":12,"colors":["#ffffff"]},"background":"#101020"}),
    ] {
        fs::write(&path, invalid.to_string()).unwrap();
        assert!(validate_scene(&path).is_err());
        assert!(import_path(&path).is_err());
    }
    fs::write(&path, valid.to_string()).unwrap();
    fs::write(
        dir.path().join("project.json"),
        json!({"type":"scene","file":"cube.wallin-scene.json"}).to_string(),
    )
    .unwrap();
    assert!(!import_path(dir.path()).unwrap().wallpaper.supported);
    fs::write(dir.path().join("legacy.pkg"), valid.to_string()).unwrap();
    assert!(
        !import_path(&dir.path().join("legacy.pkg"))
            .unwrap()
            .wallpaper
            .supported
    );
    fs::write(&path, " ".repeat(4097)).unwrap();
    assert!(validate_scene(&path).is_err());
}

#[test]
fn project_import_rejects_traversal_and_symlinks() {
    let dir = TempDir::new().unwrap();
    let outside = image(dir.path());
    let project = dir.path().join("project");
    fs::create_dir(&project).unwrap();
    let manifest = project.join("project.json");
    for file in [
        "../wallpaper.png",
        "..\\wallpaper.png",
        outside.to_str().unwrap(),
        "C:\\wallpaper.png",
    ] {
        fs::write(&manifest, json!({"type":"image","file":file}).to_string()).unwrap();
        assert!(import_path(&project).is_err(), "accepted {file}");
    }
    std::os::unix::fs::symlink(&outside, project.join("escape.png")).unwrap();
    fs::write(
        &manifest,
        json!({"type":"image","file":"escape.png"}).to_string(),
    )
    .unwrap();
    assert!(import_path(&project).is_err());
    image(&project);
    fs::write(
        &manifest,
        json!({"type":"image","file":"wallpaper.png","preview":"../wallpaper.png"}).to_string(),
    )
    .unwrap();
    assert!(import_path(&project).is_err());
    fs::write(
        &manifest,
        json!({"title":"Project", "type":"image","file":"wallpaper.png","tags":["nature"]})
            .to_string(),
    )
    .unwrap();
    let entry = import_path(&project).unwrap();
    assert_eq!(entry.wallpaper.title, "Project");
    assert_eq!(entry.wallpaper.tags, ["nature"]);
    fs::remove_file(project.join("wallpaper.png")).unwrap();
    std::os::unix::fs::symlink(&outside, project.join("wallpaper.png")).unwrap();
    assert!(checked_resource(&entry, false).is_err());
}
#[test]
fn unknown_project_type_is_unsupported_and_never_executed() {
    let dir = TempDir::new().unwrap();
    fs::write(
        dir.path().join("project.json"),
        json!({"title":"Future type", "type":"magic"}).to_string(),
    )
    .unwrap();
    let entry = import_path(dir.path()).unwrap();
    assert!(!entry.wallpaper.supported);
    assert!(entry.wallpaper.description.contains("magic"));
}
#[test]
fn persistence_is_atomic_and_exclusively_locked() {
    let dir = TempDir::new().unwrap();
    let path = image(dir.path());
    let (store, mut library) = Store::open(dir.path().join("data")).unwrap();
    assert!(Store::open(dir.path().join("data")).is_err());
    library.entries.push(import_path(&path).unwrap());
    library.settings.volume = 77;
    store.save(&library).unwrap();
    drop(store);
    let (_store, loaded) = Store::open(dir.path().join("data")).unwrap();
    assert_eq!(loaded.settings.volume, 77);
    assert_eq!(
        loaded.entries[0].wallpaper.id,
        library.entries[0].wallpaper.id
    );
    assert!(
        !dir.path()
            .join("data")
            .join(format!(".library-{}.tmp", std::process::id()))
            .exists()
    );
}
#[test]
fn steam_discovery_handles_modern_and_legacy_vdf() {
    let paths = library_paths(
        "// \"path\" \"/ignored\"\n\"libraryfolders\" { \"0\" { \"path\" \"/games/Steam Library\" } \"1\" \"/legacy\" }",
    );
    assert_eq!(
        paths,
        vec![
            std::path::PathBuf::from("/games/Steam Library"),
            std::path::PathBuf::from("/legacy")
        ]
    );
    let dir = TempDir::new().unwrap();
    let steam = dir.path().join("Steam");
    let other = dir.path().join("Other Library");
    fs::create_dir_all(steam.join("steamapps")).unwrap();
    let project = other.join("steamapps/workshop/content/431960/123");
    fs::create_dir_all(&project).unwrap();
    image(&project);
    fs::write(
        project.join("project.json"),
        json!({"type":"image","file":"wallpaper.png"}).to_string(),
    )
    .unwrap();
    fs::write(
        steam.join("steamapps/libraryfolders.vdf"),
        format!(
            "\"libraryfolders\" {{ \"0\" {{ \"path\" \"{}\" }} }}",
            other.display()
        ),
    )
    .unwrap();
    let result = scan_roots(&[steam]).unwrap();
    assert_eq!(result.entries.len(), 1);
    assert_eq!(result.paths.len(), 1);
}

async fn fixture() -> (
    TempDir,
    axum::Router,
    std::sync::Arc<tokio::sync::Mutex<Service>>,
) {
    let dir = TempDir::new().unwrap();
    let helper = dir.path().join("renderer.py");
    fs::write(&helper, r#"import sys,json,os,time
from pathlib import Path
root=Path(__file__).parent
if '--diagnostics' in sys.argv:
 available=not (root/'unavailable').exists()
 print(json.dumps({'desktop':'test','session':'test','backend':'test','available':available,'message':'Test helper' if available else 'Test backend unavailable','monitors':[{'id':'test-0','name':'Test','width':1920,'height':1080}]}));sys.exit(0)
with open(root/'pids','a') as f:f.write(str(os.getpid())+'\n')
if (root/'fail').exists():
 print('Test renderer failed before ready',file=sys.stderr,flush=True);sys.exit(2)
while (root/'wait').exists() and not (root/'ready').exists():time.sleep(0.01)
print(json.dumps({'event':'ready'}),flush=True)
for line in sys.stdin:
 command=json.loads(line)
 with open(root/'controls','a') as f:f.write(command['action']+'\n')
 if command['action']=='stop':break
 if command['action']=='configure':
  assert not (command.keys()-{'action','monitor','fit','volume','muted','pause_on_battery','pause_on_fullscreen'})
"#).unwrap();
    fs::write(
        dir.path().join("widgets.py"),
        r#"import json,sys,os
from pathlib import Path
root=Path(__file__).parent
print(json.dumps({'event':'ready'}),flush=True)
for line in sys.stdin:
 command=json.loads(line)
 if command['action']=='stop':break
 assert command['action']=='set'
 with open(root/'widget-calls','a') as log:log.write(json.dumps(command)+'\n')
 if (root/'widget-fail').exists():
  print(json.dumps({'event':'error','message':'Widget bridge refused update'}),flush=True)
 else:
  print(json.dumps({'event':'applied'}),flush=True)
 if (root/'widget-crash').exists():sys.exit(2)
"#,
    )
    .unwrap();
    let service = Service::open(dir.path().join("data"), helper)
        .await
        .unwrap();
    let app = router(service.clone(), dir.path().join("dist"), 3977);
    (dir, app, service)
}
async fn call(app: &axum::Router, method: &str, path: &str, body: Value) -> (StatusCode, Value) {
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method(method)
                .uri(path)
                .header("host", "127.0.0.1:3977")
                .header("content-type", "application/json")
                .body(Body::from(body.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    (status, serde_json::from_slice(&bytes).unwrap())
}
#[tokio::test]
async fn native_scene_apply_and_restore_revalidate_source() {
    let (dir, app, service) = fixture().await;
    let path = dir.path().join("cube.wallin-scene.json");
    let scene = json!({"version":1,"camera":{"fov":60,"distance":5},
        "cube":{"size":1.5,"rpm":12,"colors":["#ff0000","#00ff00","#0000ff",
            "#ffff00","#ff00ff","#00ffff"]},"background":"#101020"});
    fs::write(&path, scene.to_string()).unwrap();
    let (status, imported) = call(&app, "POST", "/api/import", json!({"path":path})).await;
    assert_eq!(status, StatusCode::OK, "{imported}");
    let id = imported["wallpapers"][0]["id"].as_str().unwrap().to_owned();
    assert_eq!(imported["wallpapers"][0]["supported"], true);
    let (status, playback) = call(
        &app,
        "POST",
        "/api/playback",
        json!({"action":"apply","wallpaper_id":id}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{playback}");
    fs::write(
        &path,
        json!({"version":1,"camera":scene["camera"],
        "cube":scene["cube"],"background":"#101020","asset":"file:///etc/passwd"})
        .to_string(),
    )
    .unwrap();
    let (status, _) = call(
        &app,
        "POST",
        "/api/playback",
        json!({"action":"apply","wallpaper_id":id}),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(
        fs::read_to_string(dir.path().join("pids"))
            .unwrap()
            .lines()
            .count(),
        1
    );
    service.lock().await.shutdown().await;
    drop(app);
    drop(service);
    let restored = Service::open(dir.path().join("data"), dir.path().join("renderer.py"))
        .await
        .unwrap();
    let state = restored.lock().await.snapshot();
    assert_eq!(state.playback.status, "stopped");
    assert!(
        state
            .playback
            .error
            .unwrap()
            .contains("Cannot restore wallpaper")
    );
    assert_eq!(
        fs::read_to_string(dir.path().join("pids"))
            .unwrap()
            .lines()
            .count(),
        1
    );
    restored.lock().await.shutdown().await;
}

#[tokio::test]
async fn native_widgets_import_position_restore_and_remove() {
    let (dir, app, service) = fixture().await;
    let root = dir.path().join("skins");
    fs::create_dir(&root).unwrap();
    fs::write(root.join("clock.ini"), "[Text]\nMeter=String\nText=Hello\nX=10\nY=20\nW=150\nH=40\nFontColor=10,20,30,255\nFontSize=18\n[Shape]\nMeter=Shape\nShape=Rectangle 0,0,10,10\n").unwrap();
    let (status, state) = call(
        &app,
        "POST",
        "/api/widgets/import",
        json!({"root":root,"ini":"clock.ini"}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{state}");
    let id = state["widgets"][0]["id"].as_str().unwrap().to_owned();
    assert_eq!(state["widgets"][0]["enabled"], false);
    assert_eq!(
        state["widgets"][0]["diagnostics"][0]["code"],
        "unsupported_native_meter"
    );
    assert!(!dir.path().join("widget-calls").exists());
    let (_, reimported) = call(
        &app,
        "POST",
        "/api/widgets/import",
        json!({"root":root,"ini":"clock.ini"}),
    )
    .await;
    assert_eq!(reimported["widgets"].as_array().unwrap().len(), 1);
    assert_eq!(reimported["widgets"][0]["id"], id);
    let (status, state) = call(
        &app,
        "PATCH",
        &format!("/api/widgets/{id}"),
        json!({"enabled":true,"monitor":"test-0","x":30,"y":40}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{state}");
    let commands = fs::read_to_string(dir.path().join("widget-calls")).unwrap();
    let command: Value = serde_json::from_str(commands.lines().last().unwrap()).unwrap();
    assert_eq!(command["widgets"][0]["x"], 40);
    assert_eq!(command["widgets"][0]["y"], 60);
    assert_eq!(command["widgets"][0]["text"], "Hello");
    assert_eq!(command["widgets"][0]["color"], "#0a141eff");
    assert_eq!(command["widgets"][0]["font_size"], 18);
    assert_eq!(command["widgets"][0]["id"], format!("{id}_0"));
    service.lock().await.shutdown().await;
    drop(app);
    drop(service);
    let restored = Service::open(dir.path().join("data"), dir.path().join("renderer.py"))
        .await
        .unwrap();
    assert!(restored.lock().await.snapshot().widget_error.is_none());
    let app = router(restored.clone(), dir.path().join("dist"), 3977);
    let (status, listed) = call(&app, "GET", "/api/widgets", json!(null)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(listed[0]["id"], id);
    let (status, state) = call(&app, "DELETE", &format!("/api/widgets/{id}"), json!(null)).await;
    assert_eq!(status, StatusCode::OK, "{state}");
    assert!(state["widgets"].as_array().unwrap().is_empty());
    restored.lock().await.shutdown().await;
}

#[tokio::test]
async fn native_clock_import_persists_template_and_documents_format() {
    let (dir, app, service) = fixture().await;
    let root = dir.path().join("skins");
    fs::create_dir(&root).unwrap();
    fs::write(root.join("clock.ini"), "[MeasureTime]\nMeasure=Time\nFormat=%H:%M:%S\n[MeterClock]\nMeter=String\nMeasureName=MeasureTime\nText=Now %1\n[Static]\nMeter=String\nText=Hello\n").unwrap();
    let (status, state) = call(
        &app,
        "POST",
        "/api/widgets/import",
        json!({"root":root,"ini":"clock.ini"}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{state}");
    let widget = &state["widgets"][0];
    assert_eq!(widget["enabled"], false);
    assert_eq!(widget["meters"][0]["text"], "Now %1");
    assert_eq!(widget["meters"][0]["clock_format"], "%H:%M:%S");
    assert!(widget["meters"][1].get("clock_format").is_none());
    assert!(!dir.path().join("widget-calls").exists());
    let saved = saved_library(&dir);
    assert_eq!(
        saved.widgets[0].meters[0].clock_format.as_deref(),
        Some("%H:%M:%S")
    );
    let mut old_meter = serde_json::to_value(&saved.widgets[0].meters[0]).unwrap();
    old_meter.as_object_mut().unwrap().remove("clock_format");
    let restored: wall_in_core::widgets::TextMeter = serde_json::from_value(old_meter).unwrap();
    assert_eq!(restored.clock_format, None);
    let id = widget["id"].as_str().unwrap();
    let (status, _) = call(
        &app,
        "PATCH",
        &format!("/api/widgets/{id}"),
        json!({"enabled":true,"monitor":"test-0"}),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let commands = fs::read_to_string(dir.path().join("widget-calls")).unwrap();
    let command: Value = serde_json::from_str(commands.lines().last().unwrap()).unwrap();
    assert_eq!(command["widgets"][0]["text"], "Now %1");
    assert_eq!(command["widgets"][0]["clock_format"], "%H:%M:%S");
    assert!(command["widgets"][1].get("clock_format").is_none());
    service.lock().await.shutdown().await;
}

#[tokio::test]
async fn widget_failures_do_not_claim_success_or_persist_failed_edits() {
    let (dir, app, _service) = fixture().await;
    let root = dir.path().join("skins");
    fs::create_dir(&root).unwrap();
    fs::write(
        root.join("empty.ini"),
        "[Image]\nMeter=Image\nImageName=no.png\n",
    )
    .unwrap();
    let (status, _) = call(
        &app,
        "POST",
        "/api/widgets/import",
        json!({"root":root,"ini":"empty.ini"}),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    fs::write(root.join("text.ini"), "[Text]\nMeter=Text\nText=Hello\n").unwrap();
    std::os::unix::fs::symlink(root.join("text.ini"), root.join("link.ini")).unwrap();
    for ini in ["../text.ini", "link.ini"] {
        let (status, _) = call(
            &app,
            "POST",
            "/api/widgets/import",
            json!({"root":root,"ini":ini}),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
    }
    let (_, state) = call(
        &app,
        "POST",
        "/api/widgets/import",
        json!({"root":root,"ini":"text.ini"}),
    )
    .await;
    let id = state["widgets"][0]["id"].as_str().unwrap();
    for body in [
        json!({"enabled":true,"monitor":"missing"}),
        json!({"enabled":true,"x":1910}),
    ] {
        let (status, _) = call(&app, "PATCH", &format!("/api/widgets/{id}"), body).await;
        assert!(!status.is_success());
    }
    fs::write(dir.path().join("widget-fail"), "").unwrap();
    let (status, response) = call(
        &app,
        "PATCH",
        &format!("/api/widgets/{id}"),
        json!({"enabled":true}),
    )
    .await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE, "{response}");
    assert!(!saved_library(&dir).widgets[0].enabled);
    let (_, state) = call(&app, "GET", "/api/state", json!(null)).await;
    assert!(state["widget_error"].as_str().unwrap().contains("refused"));
    fs::remove_file(dir.path().join("widget-fail")).unwrap();
    let (status, _) = call(
        &app,
        "PATCH",
        &format!("/api/widgets/{id}"),
        json!({"enabled":true}),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    fs::write(dir.path().join("widget-crash"), "").unwrap();
    let (status, _) = call(&app, "PATCH", &format!("/api/widgets/{id}"), json!({"x":1})).await;
    assert!(status == StatusCode::OK || status == StatusCode::SERVICE_UNAVAILABLE);
    tokio::time::sleep(std::time::Duration::from_millis(30)).await;
    let (_, state) = call(&app, "GET", "/api/state", json!(null)).await;
    assert!(state["widget_error"].as_str().unwrap().contains("exited"));
}

#[tokio::test]
async fn schedule_for_next_start_preserves_source_and_restores_when_integration_appears() {
    let (dir, app, service) = fixture().await;
    let (_, imported) = call(
        &app,
        "POST",
        "/api/import",
        json!({"path": image(dir.path())}),
    )
    .await;
    let id = imported["wallpapers"][0]["id"].as_str().unwrap();
    fs::write(dir.path().join("unavailable"), b"").unwrap();
    let (_, state) = call(&app, "GET", "/api/state?refresh_platform=true", json!(null)).await;
    assert_eq!(state["platform"]["available"], false);
    let (status, scheduled) = call(
        &app,
        "POST",
        "/api/playback",
        json!({"action":"schedule","wallpaper_id":id}),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(scheduled["playback"]["status"], "pending");
    assert_eq!(scheduled["playback"]["wallpaper_id"], id);
    assert_eq!(
        saved_library(&dir).desired_wallpaper_id.as_deref(),
        Some(id)
    );
    drop(app);
    drop(service);
    fs::remove_file(dir.path().join("unavailable")).unwrap();
    let restored = Service::open(dir.path().join("data"), dir.path().join("renderer.py"))
        .await
        .unwrap();
    assert_eq!(restored.lock().await.snapshot().playback.status, "playing");
    restored.lock().await.shutdown().await;
    assert!(dir.path().join("wallpaper.png").exists());
}

#[tokio::test]
async fn explicit_refresh_updates_platform_without_restarting_or_losing_state() {
    let (dir, app, _service) = fixture().await;
    let (_, imported) = call(
        &app,
        "POST",
        "/api/import",
        json!({"path":image(dir.path())}),
    )
    .await;
    fs::write(dir.path().join("unavailable"), b"").unwrap();
    let (_, cached) = call(&app, "GET", "/api/state", json!(null)).await;
    assert_eq!(cached["platform"]["available"], true);
    let (status, refreshed) =
        call(&app, "GET", "/api/state?refresh_platform=true", json!(null)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(refreshed["platform"]["available"], false);
    assert_eq!(refreshed["wallpapers"], imported["wallpapers"]);
    fs::remove_file(dir.path().join("unavailable")).unwrap();
    let (_, restored) = call(&app, "GET", "/api/state?refresh_platform=true", json!(null)).await;
    assert_eq!(restored["platform"]["available"], true);
}

#[tokio::test]
async fn api_errors_are_json_and_security_rejects_foreign_hosts() {
    let (_dir, app, _service) = fixture().await;
    for (method, path, input, expected) in [
        (
            "POST",
            "/api/import",
            json!({}),
            StatusCode::UNPROCESSABLE_ENTITY,
        ),
        (
            "POST",
            "/api/import",
            json!({"path":"/nonexistent-wall-in"}),
            StatusCode::BAD_REQUEST,
        ),
        (
            "POST",
            "/api/playback",
            json!({"action":"dance"}),
            StatusCode::BAD_REQUEST,
        ),
        (
            "POST",
            "/api/playback",
            json!({"action":"pause"}),
            StatusCode::CONFLICT,
        ),
        (
            "PATCH",
            "/api/settings",
            json!({"volume":101}),
            StatusCode::BAD_REQUEST,
        ),
        (
            "PATCH",
            "/api/settings",
            json!({"unknown":true}),
            StatusCode::BAD_REQUEST,
        ),
        (
            "PATCH",
            "/api/settings",
            json!({"playlist_ids":["missing"]}),
            StatusCode::BAD_REQUEST,
        ),
        (
            "DELETE",
            "/api/wallpapers/missing",
            json!({}),
            StatusCode::NOT_FOUND,
        ),
        ("GET", "/api/nothing", json!({}), StatusCode::NOT_FOUND),
    ] {
        let (status, body) = call(&app, method, path, input).await;
        assert_eq!(status, expected, "{path}: {body}");
        assert!(body["error"].is_string());
    }
    for (host, origin, expected) in [
        ("evil.example:3977", None, 403),
        ("127.0.0.1:3977", Some("https://evil.example"), 403),
        ("127.0.0.1:3977", Some("null"), 403),
        ("127.0.0.1:3977", Some("http://127.0.0.1:3977"), 200),
        ("localhost:3977", None, 200),
        ("127.0.0.1:3978", None, 403),
    ] {
        let mut request = Request::builder().uri("/api/state").header("host", host);
        if let Some(origin) = origin {
            request = request.header("origin", origin);
        }
        let response = app
            .clone()
            .oneshot(request.body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(response.status().as_u16(), expected, "{host} {origin:?}");
        assert!(
            response
                .headers()
                .get("access-control-allow-origin")
                .is_none()
        );
    }
}
#[tokio::test]
async fn api_import_range_update_playback_delete_lifecycle() {
    let (dir, app, service) = fixture().await;
    let path = image(dir.path());
    let (status, state) = call(&app, "POST", "/api/import", json!({"path":path})).await;
    assert_eq!(status, StatusCode::OK);
    let id = state["wallpapers"][0]["id"].as_str().unwrap();
    let (_, state) = call(&app, "POST", "/api/import", json!({"path":path})).await;
    assert_eq!(state["wallpapers"].as_array().unwrap().len(), 1);
    let (_, state) = call(
        &app,
        "PATCH",
        &format!("/api/wallpapers/{id}"),
        json!({"favorite":true,"tags":["favorite"],"title":"Edited"}),
    )
    .await;
    assert_eq!(state["wallpapers"][0]["title"], "Edited");
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!("/api/media/{id}"))
                .header("host", "127.0.0.1:3977")
                .header("range", "bytes=2-5")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::PARTIAL_CONTENT);
    assert_eq!(response.headers()["cache-control"], "no-cache");
    assert_eq!(
        response.into_body().collect().await.unwrap().to_bytes(),
        "2345"
    );
    for action in ["apply", "pause", "resume", "apply", "stop"] {
        let (status, state) = call(
            &app,
            "POST",
            "/api/playback",
            json!({"action":action,"wallpaper_id":id}),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{state}");
        assert_eq!(
            state["playback"]["status"],
            match action {
                "pause" => "paused",
                "stop" => "stopped",
                _ => "playing",
            }
        );
        if action == "apply" {
            let (status, _) = call(
                &app,
                "PATCH",
                "/api/settings",
                json!({"volume":22,"fit":"fill"}),
            )
            .await;
            assert_eq!(status, StatusCode::OK);
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
            assert_eq!(service.lock().await.snapshot().playback.status, "playing");
        }
    }
    let (_, state) = call(
        &app,
        "PATCH",
        "/api/settings",
        json!({"volume":12,"muted":false,"fit":"contain"}),
    )
    .await;
    assert_eq!(state["settings"]["volume"], 12);
    let (status, _) = call(&app, "DELETE", &format!("/api/wallpapers/{id}"), json!({})).await;
    assert_eq!(status, StatusCode::OK);
    assert!(path.exists());
    service.lock().await.shutdown().await;
}
#[tokio::test]
async fn media_revalidates_source_removal_and_restoration() {
    let (dir, app, _service) = fixture().await;
    let path = image(dir.path());
    let (_, imported) = call(&app, "POST", "/api/import", json!({"path":path})).await;
    let url = imported["wallpapers"][0]["media_url"].as_str().unwrap();
    for (present, contents) in [(true, "original"), (false, ""), (true, "restored")] {
        if present {
            fs::write(&path, contents).unwrap();
        } else {
            fs::remove_file(&path).unwrap();
        }
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri(url)
                    .header("host", "127.0.0.1:3977")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(
            response.status(),
            if present {
                StatusCode::OK
            } else {
                StatusCode::NOT_FOUND
            }
        );
        assert_eq!(
            response.headers()["cache-control"],
            if present { "no-cache" } else { "no-store" }
        );
        if present {
            assert_eq!(
                response.into_body().collect().await.unwrap().to_bytes(),
                contents
            );
        }
    }
}

#[tokio::test]
async fn helper_failure_never_reports_playing() {
    let (dir, app, service) = fixture().await;
    fs::write(dir.path().join("renderer.py"), r#"import sys,json
if '--diagnostics' in sys.argv:
 print(json.dumps({'desktop':'test','session':'test','backend':'test','available':True,'message':'test','monitors':[{'id':'test-0','name':'Test','width':1920,'height':1080}]}));sys.exit(0)
print('renderer failed',file=sys.stderr);sys.exit(2)
"#).unwrap();
    let (_, state) = call(
        &app,
        "POST",
        "/api/import",
        json!({"path":image(dir.path())}),
    )
    .await;
    let id = &state["wallpapers"][0]["id"];
    let (status, _) = call(
        &app,
        "POST",
        "/api/playback",
        json!({"action":"apply","wallpaper_id":id}),
    )
    .await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
    let state = service.lock().await.snapshot();
    assert_eq!(state.playback.status, "stopped");
    assert!(state.playback.error.unwrap().contains("renderer failed"));
}

#[tokio::test]
async fn unexpected_exit_is_detected_and_duplicate_renderers_are_stopped() {
    let (dir, app, service) = fixture().await;
    let helper = dir.path().join("renderer.py");
    fs::write(&helper, r#"import sys,json,os,time
if '--diagnostics' in sys.argv:
 print(json.dumps({'desktop':'test','session':'test','backend':'test','available':True,'message':'test','monitors':[{'id':'test-0','name':'Test','width':1920,'height':1080}]}));sys.exit(0)
with open(__file__+'.pids','a') as f:f.write(str(os.getpid())+'\n')
print(json.dumps({'event':'ready'}),flush=True)
for line in sys.stdin:
 action=json.loads(line)['action']
 if action=='stop':break
 if action=='pause':sys.exit(3)
"#).unwrap();
    let (_, state) = call(
        &app,
        "POST",
        "/api/import",
        json!({"path":image(dir.path())}),
    )
    .await;
    let id = &state["wallpapers"][0]["id"];
    for _ in 0..2 {
        let (status, _) = call(
            &app,
            "POST",
            "/api/playback",
            json!({"action":"apply","wallpaper_id":id}),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
    }
    let pids = fs::read_to_string(dir.path().join("renderer.py.pids")).unwrap();
    let pids: Vec<_> = pids.lines().collect();
    assert_eq!(pids.len(), 2);
    assert!(
        !Path::new(&format!("/proc/{}", pids[0])).exists(),
        "previous renderer still alive"
    );
    let monitor = wall_in_core::service::supervise(&service);
    call(&app, "POST", "/api/playback", json!({"action":"pause"})).await;
    tokio::time::sleep(std::time::Duration::from_millis(400)).await;
    let playback = service.lock().await.renderer.playback.clone();
    assert_eq!(playback.status, "stopped");
    assert!(playback.error.unwrap().contains("unexpectedly"));
    monitor.abort();
}

#[tokio::test]
async fn body_limits_and_malformed_json_return_errors() {
    let (_dir, app, _service) = fixture().await;
    for (body, expected) in [
        ("{".to_string(), StatusCode::BAD_REQUEST),
        (
            json!({"path":"x".repeat(70000)}).to_string(),
            StatusCode::PAYLOAD_TOO_LARGE,
        ),
    ] {
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/import")
                    .header("host", "localhost:3977")
                    .header("content-type", "application/json")
                    .body(Body::from(body))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), expected);
        let body: Value =
            serde_json::from_slice(&response.into_body().collect().await.unwrap().to_bytes())
                .unwrap();
        assert!(body["error"].is_string());
    }
}

#[tokio::test]
async fn concurrent_updates_are_serialized_and_frontend_is_served() {
    let (dir, app, service) = fixture().await;
    fs::create_dir(dir.path().join("dist")).unwrap();
    fs::write(
        dir.path().join("dist/index.html"),
        "<!doctype html><title>Wall-in</title>",
    )
    .unwrap();
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/")
                .header("host", "localhost:3977")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let (first, second) = tokio::join!(
        call(&app, "PATCH", "/api/settings", json!({"volume":63})),
        call(&app, "PATCH", "/api/settings", json!({"fit":"fill"}))
    );
    assert_eq!(first.0, StatusCode::OK);
    assert_eq!(second.0, StatusCode::OK);
    let state = service.lock().await.snapshot();
    assert_eq!(state.settings.volume, 63);
    assert_eq!(state.settings.fit, wall_in_core::models::Fit::Fill);
    drop(app);
    drop(service);
    let restored = Service::open(dir.path().join("data"), dir.path().join("renderer.py"))
        .await
        .unwrap();
    let state = restored.lock().await.snapshot();
    assert_eq!(state.settings.volume, 63);
    assert_eq!(state.settings.fit, wall_in_core::models::Fit::Fill);
}

fn saved_library(dir: &TempDir) -> wall_in_core::models::Library {
    serde_json::from_slice(&fs::read(dir.path().join("data/library.json")).unwrap()).unwrap()
}

async fn apply_fixture(dir: &TempDir, app: &axum::Router) -> String {
    let (status, state) = call(
        app,
        "POST",
        "/api/import",
        json!({"path":image(dir.path())}),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let id = state["wallpapers"][0]["id"].as_str().unwrap().to_string();
    let (status, state) = call(
        app,
        "POST",
        "/api/playback",
        json!({"action":"apply","wallpaper_id":id}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{state}");
    assert_eq!(state["playback"]["status"], "playing");
    id
}

fn helper_pids(dir: &TempDir) -> Vec<String> {
    fs::read_to_string(dir.path().join("pids"))
        .unwrap_or_default()
        .lines()
        .map(str::to_owned)
        .collect()
}

#[tokio::test]
async fn old_library_without_assignment_fields_stays_stopped() {
    let (dir, app, service) = fixture().await;
    let mut legacy = serde_json::to_value(&service.lock().await.library).unwrap();
    legacy
        .as_object_mut()
        .unwrap()
        .remove("desired_wallpaper_id");
    legacy.as_object_mut().unwrap().remove("desired_paused");
    drop(app);
    drop(service);
    fs::write(dir.path().join("data/library.json"), legacy.to_string()).unwrap();
    let service = Service::open(dir.path().join("data"), dir.path().join("renderer.py"))
        .await
        .unwrap();
    let mut service = service.lock().await;
    assert!(service.library.desired_wallpaper_id.is_none());
    assert!(!service.library.desired_paused);
    assert_eq!(service.snapshot().playback.status, "stopped");
    assert!(service.snapshot().playback.error.is_none());
    assert!(helper_pids(&dir).is_empty());
}

#[tokio::test]
async fn shutdown_preserves_assignment_and_restart_restores_after_ready() {
    let (dir, app, service) = fixture().await;
    let id = apply_fixture(&dir, &app).await;
    assert_eq!(
        saved_library(&dir).desired_wallpaper_id.as_deref(),
        Some(id.as_str())
    );
    service.lock().await.shutdown().await;
    assert_eq!(service.lock().await.snapshot().playback.status, "pending");
    assert_eq!(
        saved_library(&dir).desired_wallpaper_id.as_deref(),
        Some(id.as_str())
    );
    let original_pid = helper_pids(&dir)[0].clone();
    assert!(!Path::new(&format!("/proc/{original_pid}")).exists());
    drop(app);
    drop(service);

    fs::write(dir.path().join("wait"), "").unwrap();
    let data = dir.path().join("data");
    let helper = dir.path().join("renderer.py");
    let opening = tokio::spawn(async move { Service::open(data, helper).await });
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        while helper_pids(&dir).len() < 2 {
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    assert!(
        !opening.is_finished(),
        "startup returned before helper readiness"
    );
    assert!(
        Service::open(dir.path().join("data"), dir.path().join("renderer.py"))
            .await
            .is_err()
    );
    assert_eq!(
        helper_pids(&dir).len(),
        2,
        "second service spawned a duplicate renderer"
    );
    fs::write(dir.path().join("ready"), "").unwrap();
    let restored = opening.await.unwrap().unwrap();
    let mut restored = restored.lock().await;
    let state = restored.snapshot();
    assert_eq!(state.playback.status, "playing");
    assert_eq!(state.playback.wallpaper_id.as_deref(), Some(id.as_str()));
    assert!(state.playback.error.is_none());
    assert_eq!(helper_pids(&dir).len(), 2);
    restored.shutdown().await;
    assert_eq!(
        saved_library(&dir).desired_wallpaper_id.as_deref(),
        Some(id.as_str())
    );
}

#[tokio::test]
async fn paused_assignment_restores_and_resume_is_durable() {
    let (dir, app, service) = fixture().await;
    let id = apply_fixture(&dir, &app).await;
    let (status, _) = call(&app, "POST", "/api/playback", json!({"action":"pause"})).await;
    assert_eq!(status, StatusCode::OK);
    assert!(saved_library(&dir).desired_paused);
    service.lock().await.shutdown().await;
    drop(app);
    drop(service);
    let restored = Service::open(dir.path().join("data"), dir.path().join("renderer.py"))
        .await
        .unwrap();
    assert_eq!(restored.lock().await.snapshot().playback.status, "paused");
    assert_eq!(
        restored
            .lock()
            .await
            .snapshot()
            .playback
            .wallpaper_id
            .as_deref(),
        Some(id.as_str())
    );
    let app = router(restored.clone(), dir.path().join("dist"), 3977);
    let (status, _) = call(&app, "POST", "/api/playback", json!({"action":"resume"})).await;
    assert_eq!(status, StatusCode::OK);
    assert!(!saved_library(&dir).desired_paused);
    restored.lock().await.shutdown().await;
    drop(app);
    drop(restored);
    let restored = Service::open(dir.path().join("data"), dir.path().join("renderer.py"))
        .await
        .unwrap();
    assert_eq!(restored.lock().await.snapshot().playback.status, "playing");
    restored.lock().await.shutdown().await;
    let controls = fs::read_to_string(dir.path().join("controls")).unwrap();
    assert_eq!(controls.lines().filter(|line| *line == "pause").count(), 2);
    assert_eq!(controls.lines().filter(|line| *line == "resume").count(), 1);
}

#[tokio::test]
async fn explicit_stop_clears_durable_assignment() {
    let (dir, app, service) = fixture().await;
    apply_fixture(&dir, &app).await;
    call(&app, "POST", "/api/playback", json!({"action":"pause"})).await;
    let (status, state) = call(&app, "POST", "/api/playback", json!({"action":"stop"})).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(state["playback"]["status"], "stopped");
    assert!(state["playback"]["wallpaper_id"].is_null());
    let saved = saved_library(&dir);
    assert!(saved.desired_wallpaper_id.is_none());
    assert!(!saved.desired_paused);
    service.lock().await.shutdown().await;
    drop(app);
    drop(service);
    let restored = Service::open(dir.path().join("data"), dir.path().join("renderer.py"))
        .await
        .unwrap();
    assert_eq!(restored.lock().await.snapshot().playback.status, "stopped");
    assert_eq!(helper_pids(&dir).len(), 1);
}

#[tokio::test]
async fn restore_failures_remain_visible_and_keep_the_saved_assignment() {
    for failure in ["missing", "unavailable", "fail", "unknown"] {
        let (dir, app, service) = fixture().await;
        let id = apply_fixture(&dir, &app).await;
        service.lock().await.shutdown().await;
        drop(app);
        drop(service);
        match failure {
            "missing" => fs::remove_file(dir.path().join("wallpaper.png")).unwrap(),
            "unknown" => {
                let mut library = saved_library(&dir);
                library.entries.clear();
                fs::write(
                    dir.path().join("data/library.json"),
                    serde_json::to_vec(&library).unwrap(),
                )
                .unwrap();
            }
            flag => fs::write(dir.path().join(flag), "").unwrap(),
        }
        let restored = Service::open(dir.path().join("data"), dir.path().join("renderer.py"))
            .await
            .unwrap();
        let state = restored.lock().await.snapshot();
        assert_eq!(state.playback.status, "stopped", "{failure}");
        assert_eq!(state.playback.wallpaper_id.as_deref(), Some(id.as_str()));
        let error = state.playback.error.unwrap();
        assert!(error.starts_with("Cannot restore wallpaper:"));
        assert!(
            error.contains(match failure {
                "missing" => "no longer available",
                "unavailable" => "Test backend unavailable",
                "fail" => "Test renderer failed before ready",
                _ => "Unknown wallpaper",
            }),
            "{error}"
        );
        assert_eq!(
            saved_library(&dir).desired_wallpaper_id.as_deref(),
            Some(id.as_str())
        );
        assert_eq!(
            helper_pids(&dir).len(),
            if failure == "fail" { 2 } else { 1 }
        );
        restored.lock().await.shutdown().await;
        assert_eq!(
            saved_library(&dir).desired_wallpaper_id.as_deref(),
            Some(id.as_str())
        );
    }
}

#[tokio::test]
async fn deleting_the_saved_wallpaper_clears_assignment_even_after_failed_restore() {
    for unavailable in [false, true] {
        let (dir, app, service) = fixture().await;
        let id = apply_fixture(&dir, &app).await;
        call(&app, "POST", "/api/playback", json!({"action":"pause"})).await;
        service.lock().await.shutdown().await;
        drop(app);
        drop(service);
        if unavailable {
            fs::write(dir.path().join("unavailable"), "").unwrap();
        }
        let restored = Service::open(dir.path().join("data"), dir.path().join("renderer.py"))
            .await
            .unwrap();
        let app = router(restored.clone(), dir.path().join("dist"), 3977);
        let (status, state) =
            call(&app, "DELETE", &format!("/api/wallpapers/{id}"), json!({})).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(state["playback"]["status"], "stopped");
        assert!(state["playback"]["error"].is_null());
        let saved = saved_library(&dir);
        assert!(saved.desired_wallpaper_id.is_none());
        assert!(!saved.desired_paused);
        assert!(dir.path().join("wallpaper.png").exists());
        let count = helper_pids(&dir).len();
        restored.lock().await.shutdown().await;
        drop(app);
        drop(restored);
        let restored = Service::open(dir.path().join("data"), dir.path().join("renderer.py"))
            .await
            .unwrap();
        assert_eq!(restored.lock().await.snapshot().playback.status, "stopped");
        assert_eq!(helper_pids(&dir).len(), count);
    }
}

#[tokio::test]
async fn legacy_autostart_setting_never_installs_on_open_or_unrelated_patch() {
    let (dir, app, service) = fixture().await;
    let mut library = service.lock().await.library.clone();
    library.settings.autostart = true;
    drop(app);
    drop(service);
    fs::write(
        dir.path().join("data/library.json"),
        serde_json::to_vec(&library).unwrap(),
    )
    .unwrap();
    let restored = Service::open(dir.path().join("data"), dir.path().join("renderer.py"))
        .await
        .unwrap();
    let app = router(restored.clone(), dir.path().join("dist"), 3977);
    let (status, state) = call(&app, "PATCH", "/api/settings", json!({"volume":42})).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(state["settings"]["autostart"], true);
}

#[tokio::test]
async fn playlist_rotates_persists_and_respects_pause_stop_and_restart() {
    let (dir, app, service) = fixture().await;
    let first = apply_fixture(&dir, &app).await;
    let second_path = dir.path().join("second.png");
    fs::write(&second_path, b"second").unwrap();
    let (_, state) = call(&app, "POST", "/api/import", json!({"path":second_path})).await;
    let second = state["wallpapers"][1]["id"].as_str().unwrap().to_string();
    let (status, _) = call(
        &app,
        "PATCH",
        "/api/settings",
        json!({"playlist_ids":[first, second],"playlist_interval":1}),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let monitor = wall_in_core::service::supervise(&service);
    tokio::time::sleep(std::time::Duration::from_millis(1350)).await;
    assert_eq!(
        service
            .lock()
            .await
            .snapshot()
            .playback
            .wallpaper_id
            .as_deref(),
        Some(second.as_str())
    );
    assert_eq!(
        saved_library(&dir).desired_wallpaper_id.as_deref(),
        Some(second.as_str())
    );
    call(&app, "POST", "/api/playback", json!({"action":"pause"})).await;
    tokio::time::sleep(std::time::Duration::from_millis(1250)).await;
    assert_eq!(helper_pids(&dir).len(), 2);
    call(&app, "POST", "/api/playback", json!({"action":"resume"})).await;
    tokio::time::sleep(std::time::Duration::from_millis(1350)).await;
    assert_eq!(
        service
            .lock()
            .await
            .snapshot()
            .playback
            .wallpaper_id
            .as_deref(),
        Some(first.as_str())
    );
    call(&app, "POST", "/api/playback", json!({"action":"stop"})).await;
    tokio::time::sleep(std::time::Duration::from_millis(1250)).await;
    assert_eq!(helper_pids(&dir).len(), 3);
    monitor.abort();
    drop(app);
    drop(service);
    let restored = Service::open(dir.path().join("data"), dir.path().join("renderer.py"))
        .await
        .unwrap();
    assert_eq!(restored.lock().await.snapshot().playback.status, "stopped");
    assert_eq!(helper_pids(&dir).len(), 3);
}

#[tokio::test]
async fn single_item_playlist_does_not_restart_renderer_and_active_restart_rotates() {
    let (dir, app, service) = fixture().await;
    let first = apply_fixture(&dir, &app).await;
    let second_path = dir.path().join("second.png");
    fs::write(&second_path, b"second").unwrap();
    let (_, state) = call(&app, "POST", "/api/import", json!({"path":second_path})).await;
    let second = state["wallpapers"][1]["id"].as_str().unwrap().to_string();
    call(
        &app,
        "PATCH",
        "/api/settings",
        json!({"playlist_ids":[first],"playlist_interval":1}),
    )
    .await;
    let monitor = wall_in_core::service::supervise(&service);
    tokio::time::sleep(std::time::Duration::from_millis(1250)).await;
    assert_eq!(helper_pids(&dir).len(), 1);
    monitor.abort();
    service.lock().await.shutdown().await;
    drop(app);
    drop(service);
    let restored = Service::open(dir.path().join("data"), dir.path().join("renderer.py"))
        .await
        .unwrap();
    assert_eq!(restored.lock().await.snapshot().playback.status, "playing");
    let app = router(restored.clone(), dir.path().join("dist"), 3977);
    let (status, _) = call(
        &app,
        "PATCH",
        "/api/settings",
        json!({"playlist_ids":[saved_library(&dir).desired_wallpaper_id.unwrap(), second.clone()]}),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let monitor = wall_in_core::service::supervise(&restored);
    tokio::time::sleep(std::time::Duration::from_millis(1350)).await;
    assert_eq!(
        restored
            .lock()
            .await
            .snapshot()
            .playback
            .wallpaper_id
            .as_deref(),
        Some(second.as_str())
    );
    assert_eq!(helper_pids(&dir).len(), 3);
    monitor.abort();
    restored.lock().await.shutdown().await;
}

#[tokio::test]
async fn shuffle_visits_other_entries_and_invalid_entries_are_rejected() {
    let (dir, app, service) = fixture().await;
    let first = apply_fixture(&dir, &app).await;
    let mut ids = vec![first.clone()];
    for name in ["second.png", "third.png"] {
        let path = dir.path().join(name);
        fs::write(&path, name).unwrap();
        let (_, state) = call(&app, "POST", "/api/import", json!({"path":path})).await;
        ids.push(
            state["wallpapers"]
                .as_array()
                .unwrap()
                .iter()
                .find(|w| w["title"] == name.trim_end_matches(".png"))
                .unwrap()["id"]
                .as_str()
                .unwrap()
                .to_string(),
        );
    }
    let (status, _) = call(
        &app,
        "PATCH",
        "/api/settings",
        json!({"playlist_ids":[first.clone(), first],"playlist_interval":1}),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let (status, _) = call(
        &app,
        "PATCH",
        "/api/settings",
        json!({"playlist_ids":ids,"playlist_interval":1,"shuffle":true}),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let monitor = wall_in_core::service::supervise(&service);
    tokio::time::sleep(std::time::Duration::from_millis(1350)).await;
    let one = service
        .lock()
        .await
        .snapshot()
        .playback
        .wallpaper_id
        .unwrap();
    tokio::time::sleep(std::time::Duration::from_millis(1350)).await;
    let two = service
        .lock()
        .await
        .snapshot()
        .playback
        .wallpaper_id
        .unwrap();
    assert_ne!(one, two);
    assert!(ids.contains(&one) && ids.contains(&two));
    assert_eq!(helper_pids(&dir).len(), 3);
    monitor.abort();
    service.lock().await.shutdown().await;
}

#[tokio::test]
async fn unsuccessful_apply_does_not_replace_saved_assignment() {
    let (dir, app, service) = fixture().await;
    let original = apply_fixture(&dir, &app).await;
    call(&app, "POST", "/api/playback", json!({"action":"pause"})).await;
    let other = dir.path().join("other.png");
    fs::write(&other, "second image").unwrap();
    let (_, state) = call(&app, "POST", "/api/import", json!({"path":other})).await;
    let second = state["wallpapers"][1]["id"].as_str().unwrap();
    fs::write(dir.path().join("fail"), "").unwrap();
    let (status, _) = call(
        &app,
        "POST",
        "/api/playback",
        json!({"action":"apply","wallpaper_id":second}),
    )
    .await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
    let saved = saved_library(&dir);
    assert_eq!(
        saved.desired_wallpaper_id.as_deref(),
        Some(original.as_str())
    );
    assert!(saved.desired_paused);
    service.lock().await.shutdown().await;
    drop(app);
    drop(service);
    fs::remove_file(dir.path().join("fail")).unwrap();
    let restored = Service::open(dir.path().join("data"), dir.path().join("renderer.py"))
        .await
        .unwrap();
    let state = restored.lock().await.snapshot();
    assert_eq!(
        state.playback.wallpaper_id.as_deref(),
        Some(original.as_str())
    );
    assert_eq!(state.playback.status, "paused");
    restored.lock().await.shutdown().await;
}

#[tokio::test]
async fn failed_assignment_save_stops_unsaved_renderer_and_retains_previous_desire() {
    let (dir, app, service) = fixture().await;
    let original = apply_fixture(&dir, &app).await;
    let other = dir.path().join("other.png");
    fs::write(&other, "second image").unwrap();
    let (_, state) = call(&app, "POST", "/api/import", json!({"path":other})).await;
    let second = state["wallpapers"][1]["id"].as_str().unwrap();
    fs::create_dir(
        dir.path()
            .join("data")
            .join(format!(".library-{}.tmp", std::process::id())),
    )
    .unwrap();
    let (status, _) = call(
        &app,
        "POST",
        "/api/playback",
        json!({"action":"apply","wallpaper_id":second}),
    )
    .await;
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
    let state = service.lock().await.snapshot();
    assert_eq!(state.playback.status, "stopped");
    assert!(
        state
            .playback
            .error
            .unwrap()
            .contains("Cannot save wallpaper assignment")
    );
    assert_eq!(
        saved_library(&dir).desired_wallpaper_id.as_deref(),
        Some(original.as_str())
    );
    assert_eq!(
        service.lock().await.library.desired_wallpaper_id.as_deref(),
        Some(original.as_str())
    );
    for pid in helper_pids(&dir) {
        assert!(!Path::new(&format!("/proc/{pid}")).exists());
    }
}
