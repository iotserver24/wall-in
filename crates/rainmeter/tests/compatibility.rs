use std::fs;
use std::path::Path;
use wall_in_rainmeter::{Error, Meter, Shape, inspect_skin};

fn fixture(root: &Path, ini: &str) {
    fs::create_dir_all(root.join("@Resources/Images")).unwrap();
    fs::write(
        root.join("@Resources/Images/icon.png"),
        b"original test image placeholder",
    )
    .unwrap();
    fs::write(root.join("Skin.ini"), ini).unwrap();
}

#[test]
fn static_skin_and_variables() {
    let dir = tempfile::tempdir().unwrap();
    fixture(
        dir.path(),
        "; original fixture, not an upstream skin\n[Variables]\nTitle=Hello\nLabel=#Title# world\n[Rainmeter]\nUpdate=1000\n[Title]\nMeter=String\nText=#Label#\nX=12\nY=8\nFontColor=1,2,3,200\n[Photo]\nMeter=Image\nImageName=#@#Images\\icon.png\nW=40\nH=20\n[Badge]\nMeter=Shape\nShape=Rectangle 0,0,30,20 | Fill Color 5,6,7,128\n",
    );
    let report = inspect_skin(dir.path(), "Skin.ini").unwrap();
    assert!(report.diagnostics.is_empty(), "{:?}", report.diagnostics);
    assert_eq!(report.meters.len(), 3);
    assert!(
        matches!(&report.meters[0], Meter::Text { text, color: [1,2,3,200], .. } if text == "Hello world")
    );
    assert!(
        matches!(&report.meters[1], Meter::Image { path, .. } if path.ends_with("@Resources/Images/icon.png"))
    );
    assert!(matches!(
        &report.meters[2],
        Meter::Shape {
            shape: Shape::Rectangle { width: 30.0, .. },
            fill: [5, 6, 7, 128],
            ..
        }
    ));
}

#[test]
fn time_measure_and_literal_template() {
    let dir = tempfile::tempdir().unwrap();
    fixture(
        dir.path(),
        "[MeterClock]\nMeter=String\nMeasureName=measuretime\nText=Now: %1!\n[MeasureTime]\nMeasure=Time\nFormat=%H:%M:%S %% %I %p %d %m %Y %A %a %B %b\n[Static]\nMeter=String\nText=Hello\n",
    );
    let report = inspect_skin(dir.path(), "Skin.ini").unwrap();
    assert!(report.diagnostics.is_empty(), "{:?}", report.diagnostics);
    assert!(
        matches!(&report.meters[0], Meter::Text { text, clock_format: Some(format), .. } if text == "Now: %1!" && format == "%H:%M:%S %% %I %p %d %m %Y %A %a %B %b")
    );
    assert!(
        matches!(&report.meters[1], Meter::Text { text, clock_format: None, .. } if text == "Hello")
    );
}

#[test]
fn time_measure_rejects_unsafe_and_malformed_formats() {
    let dir = tempfile::tempdir().unwrap();
    for format in [
        "",
        "%Q",
        "%",
        "%_H",
        "%n",
        "%H[!Execute bad]",
        "#Missing#",
        "%H\t%M",
        "%H#unknown#",
    ] {
        fixture(
            dir.path(),
            &format!(
                "[MeasureTime]\nMeasure=Time\nFormat={format}\n[Clock]\nMeter=String\nMeasureName=MeasureTime\nText=%1\n"
            ),
        );
        let report = inspect_skin(dir.path(), "Skin.ini").unwrap();
        assert!(report.meters.is_empty(), "{format:?}: {report:?}");
        assert!(
            report
                .diagnostics
                .iter()
                .any(|d| d.section == "MeasureTime")
        );
        assert!(
            report
                .diagnostics
                .iter()
                .any(|d| d.section == "Clock" && d.code == "invalid_meter")
        );
    }
    fixture(
        dir.path(),
        "[MeasureTime]\nMeasure=Time\nFormat=%H\nOnUpdateAction=[!Execute bad]\n[Clock]\nMeter=String\nMeasureName=MeasureTime\nText=%1\n",
    );
    assert!(
        inspect_skin(dir.path(), "Skin.ini")
            .unwrap()
            .meters
            .is_empty()
    );
}

#[test]
fn time_meter_requires_resolved_reference_and_one_placeholder() {
    let dir = tempfile::tempdir().unwrap();
    for (reference, text) in [
        ("Missing", "%1"),
        ("MeasureTime,Other", "%1"),
        ("MeasureTime", "missing"),
        ("MeasureTime", "%1 %2"),
        ("MeasureTime", "%12"),
        ("MeasureTime", "%1 %1"),
        ("MeasureTime", "[MeasureTime] %1"),
    ] {
        fixture(
            dir.path(),
            &format!(
                "[MeasureTime]\nMeasure=Time\nFormat=%H:%M\n[Clock]\nMeter=String\nMeasureName={reference}\nText={text}\n"
            ),
        );
        let report = inspect_skin(dir.path(), "Skin.ini").unwrap();
        assert!(
            report.meters.is_empty(),
            "{reference:?} {text:?}: {report:?}"
        );
        assert!(
            report
                .diagnostics
                .iter()
                .any(|d| d.section == "Clock" && d.code == "invalid_meter")
        );
    }
    fixture(
        dir.path(),
        "[Other]\nMeasure=CPU\n[Clock]\nMeter=String\nMeasureName=Other\nText=%1\n",
    );
    let report = inspect_skin(dir.path(), "Skin.ini").unwrap();
    assert!(report.meters.is_empty());
    assert!(
        report
            .diagnostics
            .iter()
            .any(|d| d.code == "unsupported_measure")
    );
}

#[test]
fn blocked_actions_measures_plugins_lua_and_network() {
    let dir = tempfile::tempdir().unwrap();
    fixture(
        dir.path(),
        "[Rainmeter]\nOnRefreshAction=[!Execute foo]\n@Include=local.inc\n[Plugin]\nMeasure=Plugin\nPlugin=bad.dll\n[Lua]\nMeasure=Script\nScriptFile=bad.lua\n[Web]\nMeter=Image\nImageName=https://example.org/image.png\n[Command]\nMeter=String\nText=Hello\nLeftMouseUpAction=!Execute foo\n[Safe]\nMeter=String\nText=still safe\n",
    );
    let report = inspect_skin(dir.path(), "Skin.ini").unwrap();
    assert_eq!(report.meters.len(), 1);
    assert!(
        report
            .diagnostics
            .iter()
            .filter(|d| d.code == "unsupported_measure")
            .count()
            == 2
    );
    assert!(
        report
            .diagnostics
            .iter()
            .filter(|d| d.code == "unsafe_option")
            .count()
            >= 3
    );
    assert!(
        report
            .diagnostics
            .iter()
            .any(|d| d.code == "blocked_plugin")
    );
    assert!(
        report
            .diagnostics
            .iter()
            .any(|d| d.code == "blocked_script")
    );
    assert!(report.diagnostics.iter().any(|d| d.section == "Web"));
}

#[test]
fn variables_fail_closed_and_other_options_do_not_render() {
    let dir = tempfile::tempdir().unwrap();
    fixture(
        dir.path(),
        "[Variables]\nA=#B#\nB=#A#\n[Bad]\nMeter=String\nText=#A#\n[Dynamic]\nMeter=String\nText=unsafe\nDynamicVariables=1\n[Other]\nMeter=Bar\n[Good]\nMeter=Shape\nShape=Ellipse 5,6,7,8 | Fill Color 10,20,30\n",
    );
    let report = inspect_skin(dir.path(), "Skin.ini").unwrap();
    assert_eq!(report.meters.len(), 1);
    assert!(
        report
            .diagnostics
            .iter()
            .any(|d| d.section == "Bad" && d.code == "invalid_meter")
    );
    assert!(
        report
            .diagnostics
            .iter()
            .any(|d| d.section == "Dynamic" && d.code == "unsupported_option")
    );
    assert!(
        report
            .diagnostics
            .iter()
            .any(|d| d.section == "Other" && d.code == "unsupported_meter")
    );
}

#[test]
fn path_traversal_and_archives_rejected() {
    let dir = tempfile::tempdir().unwrap();
    fixture(
        dir.path(),
        "[Image]\nMeter=Image\nImageName=../secret.png\n[Nested]\nMeter=Image\nImageName=pack.zip/picture.png\n[Absolute]\nMeter=Image\nImageName=C:\\Windows\\x.png\n",
    );
    let report = inspect_skin(dir.path(), "Skin.ini").unwrap();
    assert_eq!(report.meters.len(), 0);
    assert_eq!(report.diagnostics.len(), 3);
    assert!(matches!(
        inspect_skin(dir.path(), "../Skin.ini"),
        Err(Error::UnsafePath(_))
    ));
    assert!(matches!(
        inspect_skin(dir.path(), "pack.rmskin/Skin.ini"),
        Err(Error::UnsafePath(_))
    ));
}

#[cfg(unix)]
#[test]
fn symlinks_rejected_for_ini_and_resource() {
    use std::os::unix::fs::symlink;
    let dir = tempfile::tempdir().unwrap();
    fixture(dir.path(), "[Image]\nMeter=Image\nImageName=link.png\n");
    symlink(
        dir.path().join("@Resources/Images/icon.png"),
        dir.path().join("link.png"),
    )
    .unwrap();
    let report = inspect_skin(dir.path(), "Skin.ini").unwrap();
    assert!(report.meters.is_empty());
    symlink(dir.path().join("Skin.ini"), dir.path().join("Alias.ini")).unwrap();
    assert!(matches!(
        inspect_skin(dir.path(), "Alias.ini"),
        Err(Error::UnsafePath(_))
    ));
}

#[test]
fn utf16_and_malformed_or_oversized_ini() {
    let dir = tempfile::tempdir().unwrap();
    fixture(dir.path(), "");
    let mut utf16 = vec![0xff, 0xfe];
    for unit in "[Hello]\nMeter=String\nText=Hi\n".encode_utf16() {
        utf16.extend(unit.to_le_bytes());
    }
    fs::write(dir.path().join("Skin.ini"), utf16).unwrap();
    assert_eq!(
        inspect_skin(dir.path(), "Skin.ini").unwrap().meters.len(),
        1
    );
    fs::write(
        dir.path().join("Skin.ini"),
        "[A]\nMeter=String\nMeter=Image\n",
    )
    .unwrap();
    assert!(matches!(
        inspect_skin(dir.path(), "Skin.ini"),
        Err(Error::InvalidIni(_))
    ));
    fs::write(dir.path().join("Skin.ini"), vec![b'x'; 1_048_577]).unwrap();
    assert!(matches!(
        inspect_skin(dir.path(), "Skin.ini"),
        Err(Error::UnsafePath(_))
    ));
}
