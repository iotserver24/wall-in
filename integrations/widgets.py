#!/usr/bin/env python3
"""Keep a bounded native GNOME widget document alive for the service."""

import json
import os
import select
import sys

BUS_NAME = "org.gnome.Shell"
OBJECT_PATH = "/io/wallin/Integration"
INTERFACE = "io.wallin.Integration"
MAX_LINE = 524288


class WidgetBridge:
    def __init__(self, connection, GLib, Gio):
        self.connection = connection
        self.GLib = GLib
        self.Gio = Gio
        self.document = None
        self.owner = None

    def call(self, method, parameter=None, signature=None):
        result = self.connection.call_sync(
            BUS_NAME, OBJECT_PATH, INTERFACE, method,
            self.GLib.Variant(signature, parameter) if signature else None,
            None, self.Gio.DBusCallFlags.NONE, 5000, None,
        )
        return result.unpack() if result is not None else ()

    def shell_owner(self):
        result = self.connection.call_sync(
            "org.freedesktop.DBus", "/org/freedesktop/DBus",
            "org.freedesktop.DBus", "GetNameOwner",
            self.GLib.Variant("(s)", (BUS_NAME,)),
            self.GLib.VariantType("(s)"), self.Gio.DBusCallFlags.NONE, 3000, None,
        )
        return result.unpack()[0]

    def available(self):
        probe = json.loads(self.call("Probe")[0])
        if probe.get("version") != 2 or probe.get("widgets", {}).get("version") != 1:
            raise RuntimeError("GNOME widget bridge is not supported by this extension")
        self.owner = self.shell_owner()

    def set(self, widgets):
        if not isinstance(widgets, list) or len(widgets) > 32:
            raise ValueError("expected up to 32 widgets")
        document = json.dumps({"widgets": widgets}, ensure_ascii=False, separators=(",", ":"))
        if len(document.encode("utf-8")) > MAX_LINE:
            raise ValueError("widget document exceeds 512 KiB")
        if self.call("SetWidgets", (document,), "(s)")[0] is not True:
            raise RuntimeError("GNOME rejected widget document")
        self.document = document
        self.owner = self.shell_owner()

    def reconnect(self):
        if self.document is None:
            return
        try:
            owner = self.shell_owner()
            if owner != self.owner:
                self.available()
                if self.call("SetWidgets", (self.document,), "(s)")[0] is not True:
                    raise RuntimeError("GNOME rejected widget restoration")
        except self.GLib.Error:
            self.owner = None

    def stop(self):
        if self.document is not None and self.owner:
            try:
                self.call("ClearWidgets")
            except self.GLib.Error:
                pass
        self.document = None


def respond(event, message=None):
    result = {"event": event}
    if message is not None:
        result["message"] = str(message)
    print(json.dumps(result), flush=True)


def main():
    import gi
    gi.require_version("Gio", "2.0")
    from gi.repository import Gio, GLib

    bridge = WidgetBridge(Gio.bus_get_sync(Gio.BusType.SESSION, None), GLib, Gio)
    bridge.available()
    respond("ready")
    buffer = bytearray()
    try:
        stopped = False
        while not stopped:
            ready, _, _ = select.select([0], [], [], 2)
            if not ready:
                bridge.reconnect()
                continue
            chunk = os.read(0, 65536)
            if not chunk:
                break
            buffer.extend(chunk)
            while b"\n" in buffer:
                line, _, remaining = buffer.partition(b"\n")
                buffer = bytearray(remaining)
                if len(line) > MAX_LINE:
                    respond("error", "widget command exceeds 512 KiB")
                    stopped = True
                    break
                try:
                    command = json.loads(line)
                    if not isinstance(command, dict):
                        raise ValueError("widget command must be an object")
                    if command.get("action") == "stop" and set(command) == {"action"}:
                        stopped = True
                        break
                    if command.get("action") != "set" or set(command) != {"action", "widgets"}:
                        raise ValueError("unknown widget command")
                    bridge.set(command["widgets"])
                    respond("applied")
                except (ValueError, RuntimeError, GLib.Error) as error:
                    respond("error", error)
            if len(buffer) > MAX_LINE:
                respond("error", "widget command exceeds 512 KiB")
                break
    finally:
        bridge.stop()
    return 0


if __name__ == "__main__":
    try:
        sys.exit(main())
    except Exception as error:
        respond("error", error)
        sys.exit(1)
