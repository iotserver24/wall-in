#!/usr/bin/env python3
"""Exercise a live GNOME clock sender inside the isolated nested session."""

import json
import time

import gi

gi.require_version("Gio", "2.0")
from gi.repository import Gio, GLib

connection = Gio.bus_get_sync(Gio.BusType.SESSION, None)


def call(method, values=None, signature=None):
    response = connection.call_sync(
        "org.gnome.Shell", "/io/wallin/Integration", "io.wallin.Integration", method,
        GLib.Variant(signature, values) if signature else None, None,
        Gio.DBusCallFlags.NONE, 5000, None,
    )
    return response.unpack() if response else ()


probe = json.loads(call("Probe")[0])
monitor = probe["monitors"][0]["id"]
widget = {"id": "clock_test", "monitor": monitor, "x": 40, "y": 40,
          "width": 300, "height": 65, "text": "Clock %1", "clock_format": "%H:%M:%S",
          "font_size": 32, "color": "#ffffff"}
try:
    assert call("SetWidgets", (json.dumps({"widgets": [widget]}),), "(s)") == (True,)
    first = json.loads(call("WidgetStatus")[0])["widgets"][0]
    assert first["actorCount"] > 0, first
    assert first["text"].startswith("Clock "), first
    second = first
    for _ in range(4):
        time.sleep(0.7)
        second = json.loads(call("WidgetStatus")[0])["widgets"][0]
        if first["text"] != second["text"]:
            break
    assert first["text"] != second["text"], (first, second)
    assert second["actorCount"] > 0, second
    print(f"PASS native clock actor updated: {first['text']} → {second['text']}; "
          f"actors={second['actorCount']}")
finally:
    call("ClearWidgets")
    assert json.loads(call("WidgetStatus")[0]) == {"widgets": []}
