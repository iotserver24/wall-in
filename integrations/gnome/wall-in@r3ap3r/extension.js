import Clutter from 'gi://Clutter';
import Gio from 'gi://Gio';
import GLib from 'gi://GLib';
import Meta from 'gi://Meta';
import St from 'gi://St';

const MAX_WIDGET_DOCUMENT = 524288;
const MAX_WIDGETS_PER_SENDER = 32;
const MAX_WIDGETS_TOTAL = 64;
const MAX_IMAGE_BYTES = 131072;
const MAX_TEXT_LENGTH = 2048;
const PNG_SIGNATURE = [137, 80, 78, 71, 13, 10, 26, 10];

function widgetError(message) {
    throw new Error(`Invalid widget document: ${message}`);
}

function boundedInteger(value, min, max, name) {
    if (!Number.isInteger(value) || value < min || value > max)
        widgetError(name);
    return value;
}

function decodeImage(image) {
    if (!image || typeof image !== 'object' || Array.isArray(image) ||
        Object.keys(image).sort().join(',') !== 'data,mime' || image.mime !== 'image/png' ||
        typeof image.data !== 'string' || image.data.length > 174764 ||
        !/^(?:[A-Za-z0-9+/]{4})*(?:[A-Za-z0-9+/]{2}==|[A-Za-z0-9+/]{3}=)?$/.test(image.data))
        widgetError('image must be bounded base64 PNG');
    const bytes = GLib.base64_decode(image.data);
    if (bytes.length < 24 || bytes.length > MAX_IMAGE_BYTES ||
        !PNG_SIGNATURE.every((byte, index) => bytes[index] === byte) ||
        bytes[8] !== 0 || bytes[9] !== 0 || bytes[10] !== 0 || bytes[11] !== 13 ||
        bytes[12] !== 73 || bytes[13] !== 72 || bytes[14] !== 68 || bytes[15] !== 82)
        widgetError('image PNG header or size');
    const dimension = offset => (bytes[offset] * 0x1000000 +
        (bytes[offset + 1] << 16) + (bytes[offset + 2] << 8) + bytes[offset + 3]);
    const width = dimension(16);
    const height = dimension(20);
    if (!width || !height || width > 2048 || height > 2048 || width * height > 2097152)
        widgetError('image dimensions');
    return bytes;
}

import {Extension, InjectionManager} from 'resource:///org/gnome/shell/extensions/extension.js';
import * as Background from 'resource:///org/gnome/shell/ui/background.js';
import * as Main from 'resource:///org/gnome/shell/ui/main.js';
import * as Workspace from 'resource:///org/gnome/shell/ui/workspace.js';
import * as WorkspaceAnimation from 'resource:///org/gnome/shell/ui/workspaceAnimation.js';

const APP_ID = 'io.wallin.Renderer';
const IFACE = `<node><interface name="io.wallin.Integration">
  <method name="Probe"><arg type="s" direction="out"/></method>
  <method name="Register">
    <arg name="token" type="s" direction="in"/>
    <arg name="pid" type="u" direction="in"/>
    <arg name="monitor" type="s" direction="in"/>
    <arg type="b" direction="out"/>
  </method>
  <method name="Status"><arg type="s" direction="in"/><arg type="s" direction="out"/></method>
  <method name="Configure">
    <arg type="s" direction="in"/><arg type="s" direction="in"/><arg type="b" direction="out"/>
  </method>
  <method name="Unregister"><arg type="s" direction="in"/></method>
  <method name="SetWidgets"><arg name="document" type="s" direction="in"/><arg type="b" direction="out"/></method>
  <method name="GetWidgets"><arg type="s" direction="out"/></method>
  <method name="WidgetStatus"><arg type="s" direction="out"/></method>
  <method name="ClearWidgets"/>
</interface></node>`;

export default class WallInExtension extends Extension {
    enable() {
        try {
            this._enable();
        } catch (error) {
            this.disable();
            throw error;
        }
    }

    _enable() {
        this._requests = new Map();
        this._backgrounds = new Map();
        this._widgetOwners = new Map();
        this._clockTimer = 0;
        this._signals = [];
        this._unredirectDisabled = false;
        this._enabled = true;
        this._injections = new InjectionManager();
        this._supported = typeof Meta.Window.prototype.hide_from_window_list === 'function' &&
            typeof global.compositor.disable_unredirect === 'function' &&
            typeof Background.BackgroundManager.prototype._createBackgroundActor === 'function';
        if (!this._supported)
            throw new Error('Wall-in requires the GNOME 50 background/window-list APIs');

        const extension = this;
        this._injections.overrideMethod(Background.BackgroundManager.prototype,
            '_createBackgroundActor', original => function (...args) {
                const actor = original.apply(this, args);
                extension._trackBackground(actor, this._monitorIndex);
                return actor;
            });
        this._injections.overrideMethod(Main.wm, '_shouldAnimateActor', original =>
            function (actor, ...args) {
                const request = extension._requestFor(actor.meta_window);
                if (request) {
                    if (!request.actor && !request.error) {
                        try {
                            extension._attach(request, actor);
                        } catch (error) {
                            request.error = String(error);
                            actor.hide();
                        }
                    }
                    return false;
                }
                return original.call(this, actor, ...args);
            });
        this._injections.overrideMethod(Workspace.Workspace.prototype,
            '_isOverviewWindow', original => function (window) {
                return !extension._requestFor(window) && original.call(this, window);
            });
        // Workspace swipes clone sticky windows independently of skip-taskbar.
        this._injections.overrideMethod(WorkspaceAnimation.WorkspaceGroup.prototype,
            '_shouldShowWindow', original => function (window) {
                return !extension._requestFor(window) && original.call(this, window);
            });
        for (const manager of Main.layoutManager._bgManagers)
            this._trackBackground(manager.backgroundActor, manager._monitorIndex);
        const visit = actor => {
            if (actor instanceof Meta.BackgroundActor)
                this._trackBackground(actor, actor.monitor);
            for (const child of actor.get_children())
                visit(child);
        };
        visit(global.stage);

        this._connect(Main.layoutManager, 'monitors-changed', () => this._syncMonitors());
        this._connect(global.display, 'notify::focus-window', () => this._restoreFocus());
        this._connect(global.window_manager, 'map', (_manager, actor) => {
            const request = this._requestFor(actor.meta_window);
            if (!request || request.actor || request.error)
                return;
            try {
                this._attach(request, actor);
            } catch (error) {
                request.error = String(error);
                actor.hide();
            }
        });
        this._dbus = Gio.DBusExportedObject.wrapJSObject(IFACE, this);
        this._dbus.export(Gio.DBus.session, '/io/wallin/Integration');
    }

    _connect(object, signal, callback) {
        this._signals.push([object, object.connect(signal, callback)]);
    }

    _monitors() {
        const manager = global.backend.get_monitor_manager();
        return manager.get_monitors().map(physical => {
            const connector = physical.get_connector();
            const index = manager.get_monitor_for_connector(connector);
            const monitor = Main.layoutManager.monitors[index];
            return monitor ? {id: connector, name: connector,
                width: monitor.width, height: monitor.height, index} : null;
        }).filter(monitor => monitor !== null);
    }

    _monitor(id) {
        const descriptor = this._monitors().find(monitor => monitor.id === id);
        return descriptor ? Main.layoutManager.monitors[descriptor.index] : null;
    }

    Probe() {
        return JSON.stringify({version: 2, supported: this._supported,
            monitors: this._monitors(), message: 'GNOME 50 background actor bridge',
            widgets: {version: 1, maxPerSender: MAX_WIDGETS_PER_SENDER,
                maxTotal: MAX_WIDGETS_TOTAL, maxDocument: MAX_WIDGET_DOCUMENT,
                maxImageBytes: MAX_IMAGE_BYTES, imageMime: 'image/png'}});
    }

    RegisterAsync([token, pid, monitor], invocation) {
        const sender = invocation.get_sender();
        if (!/^[a-f0-9]{32}$/.test(token) || pid === 0 || this._requests.has(token) ||
            !this._monitor(monitor) || [...this._requests.values()].some(r => r.monitor === monitor)) {
            invocation.return_value(new GLib.Variant('(b)', [false]));
            return;
        }
        Gio.DBus.session.call('org.freedesktop.DBus', '/org/freedesktop/DBus',
            'org.freedesktop.DBus', 'GetConnectionUnixProcessID',
            new GLib.Variant('(s)', [sender]), new GLib.VariantType('(u)'),
            Gio.DBusCallFlags.NONE, 2000, null, (connection, result) => {
                try {
                    const [actualPid] = connection.call_finish(result).deepUnpack();
                    if (!this._enabled || actualPid !== pid || this._requests.has(token) ||
                        [...this._requests.values()].some(r => r.monitor === monitor)) {
                        invocation.return_value(new GLib.Variant('(b)', [false]));
                        return;
                    }
                    const request = {token, pid, monitor, sender, actor: null, clones: new Map(),
                        error: '', signals: [], registeredAt: GLib.get_monotonic_time(),
                        previousFocus: global.display.focus_window};
                    this._requests.set(token, request);
                    request.watch = Gio.bus_watch_name_on_connection(Gio.DBus.session, sender,
                        Gio.BusNameWatcherFlags.NONE, null, () => this._remove(token));
                    if (!this._unredirectDisabled) {
                        global.compositor.disable_unredirect();
                        this._unredirectDisabled = true;
                    }
                    if (!this._timer) {
                        this._timer = GLib.timeout_add(GLib.PRIORITY_DEFAULT, 100, () => {
                            this._scan();
                            return GLib.SOURCE_CONTINUE;
                        });
                    }
                    invocation.return_value(new GLib.Variant('(b)', [true]));
                } catch (error) {
                    invocation.return_dbus_error('io.wallin.Error', String(error));
                }
            });
    }

    _owned(token, invocation) {
        const request = this._requests.get(token);
        if (request?.sender === invocation.get_sender())
            return request;
        invocation.return_dbus_error('io.wallin.Error', 'Unknown renderer registration');
        return null;
    }

    _fullscreen(monitor) {
        const target = this._monitors().find(item => item.id === monitor);
        if (!target)
            return false;
        return global.get_window_actors().some(actor => {
            const window = actor.meta_window;
            return window && !this._requestFor(window) && window.get_monitor() === target.index &&
                window.is_fullscreen() && window.showing_on_its_workspace();
        });
    }

    StatusAsync([token], invocation) {
        const request = this._owned(token, invocation);
        if (!request)
            return;
        const actor = request.actor;
        const hasTexture = !!actor?.get_texture();
        const dimension = value => Number.isFinite(value) ? Math.max(0, Math.min(16384, value)) : 0;
        let matchingWindowCount = 0;
        for (const candidate of global.get_window_actors()) {
            if (this._requestFor(candidate.meta_window) === request && ++matchingWindowCount === 64)
                break;
        }
        const diagnostics = {
            actorExists: !!actor,
            actorWidth: dimension(actor?.width),
            actorHeight: dimension(actor?.height),
            hasTexture,
            cloneCount: Math.min(64, request.clones.size),
            backgroundCount: Math.min(64, this._backgrounds.size),
            matchingWindowCount,
        };
        const attached = !!actor && hasTexture && actor.width > 0 && actor.height > 0 &&
            request.clones.size > 0 && !request.error;
        invocation.return_value(new GLib.Variant('(s)', [JSON.stringify({
            attached, error: request.error, fullscreen: this._fullscreen(request.monitor), diagnostics,
        })]));
    }

    ConfigureAsync([token, monitor], invocation) {
        const request = this._owned(token, invocation);
        if (!request)
            return;
        if (!this._monitor(monitor) || [...this._requests.values()].some(
            other => other !== request && other.monitor === monitor)) {
            invocation.return_value(new GLib.Variant('(b)', [false]));
            return;
        }
        request.monitor = monitor;
        this._clearClones(request);
        this._position(request);
        this._addClones(request);
        invocation.return_value(new GLib.Variant('(b)', [true]));
    }

    UnregisterAsync([token], invocation) {
        if (!this._owned(token, invocation))
            return;
        this._remove(token);
        invocation.return_value(null);
    }

    _parseWidgets(document) {
        if (typeof document !== 'string' || document.length > MAX_WIDGET_DOCUMENT)
            widgetError('document size');
        let parsed;
        try {
            parsed = JSON.parse(document);
        } catch {
            widgetError('JSON syntax');
        }
        if (!parsed || typeof parsed !== 'object' || Array.isArray(parsed) ||
            Object.keys(parsed).join(',') !== 'widgets' || !Array.isArray(parsed.widgets) ||
            parsed.widgets.length > MAX_WIDGETS_PER_SENDER)
            widgetError('expected {widgets:[]} with at most 32 entries');
        const ids = new Set();
        const widgets = parsed.widgets.map(entry => {
            if (!entry || typeof entry !== 'object' || Array.isArray(entry) ||
                Object.keys(entry).some(key => !['id', 'monitor', 'x', 'y', 'width',
                    'height', 'text', 'color', 'font_size', 'image', 'clock_format'].includes(key)))
                widgetError('unknown widget field');
            const {id, monitor, x, y, width, height} = entry;
            if (typeof id !== 'string' || !/^[A-Za-z0-9_-]{1,64}$/.test(id) || ids.has(id))
                widgetError('widget id must be unique and 1–64 ASCII characters');
            ids.add(id);
            if (typeof monitor !== 'string' || !this._monitor(monitor))
                widgetError('unknown monitor connector');
            const geometry = this._monitor(monitor);
            boundedInteger(x, 0, geometry.width - 1, 'x');
            boundedInteger(y, 0, geometry.height - 1, 'y');
            boundedInteger(width, 1, Math.min(4096, geometry.width - x), 'width');
            boundedInteger(height, 1, Math.min(4096, geometry.height - y), 'height');
            if (!Object.hasOwn(entry, 'text') && !Object.hasOwn(entry, 'image'))
                widgetError('text or image required');
            if (Object.hasOwn(entry, 'text') &&
                (typeof entry.text !== 'string' || entry.text.length > MAX_TEXT_LENGTH ||
                /[\u0000-\u0009\u000b-\u001f\u007f]/.test(entry.text)))
                widgetError('text must be plain printable text');
            if (entry.color !== undefined &&
                (typeof entry.color !== 'string' || !/^#[0-9a-fA-F]{6}(?:[0-9a-fA-F]{2})?$/.test(entry.color)))
                widgetError('color must be #RRGGBB or #RRGGBBAA');
            if (entry.font_size !== undefined)
                boundedInteger(entry.font_size, 8, 96, 'font_size');
            if (entry.clock_format !== undefined) {
                if (typeof entry.clock_format !== 'string' || entry.clock_format.length === 0 ||
                    entry.clock_format.length > 64 ||
                    /[\u0000-\u001f\u007f\[\]#]/.test(entry.clock_format) ||
                    entry.clock_format.replace(/%[HMSIpdmYAaBb%]/g, '').includes('%') ||
                    typeof entry.text !== 'string' ||
                    entry.text.split('%1').length !== 2 || /%1[0-9]|%[02-9]/.test(entry.text))
                    widgetError('clock format or %1 template');
            }
            return {id, monitor, x, y, width, height, text: entry.text,
                color: entry.color ?? '#ffffff', fontSize: entry.font_size ?? 24,
                clockFormat: entry.clock_format,
                bytes: entry.image === undefined ? null : decodeImage(entry.image)};
        });
        return {widgets, document: JSON.stringify(parsed)};
    }

    _clockText(widget, now) {
        return widget.text.replace('%1', now.format(widget.clockFormat) ?? '').slice(0, MAX_TEXT_LENGTH);
    }

    _refreshClocks() {
        const now = GLib.DateTime.new_now_local();
        for (const owner of this._widgetOwners.values()) {
            for (const actors of owner.actors.values()) {
                for (const actor of actors) {
                    const clock = actor._wallInClock;
                    if (!clock)
                        continue;
                    const next = this._clockText(clock.widget, now);
                    if (clock.label.text === next)
                        continue;
                    clock.label.remove_all_transitions();
                    clock.label.ease({opacity: 120, duration: 120,
                        mode: Clutter.AnimationMode.EASE_OUT_QUAD, onComplete: () => {
                            if (!actor.get_parent())
                                return;
                            clock.label.text = next;
                            clock.label.ease({opacity: 255, duration: 180,
                                mode: Clutter.AnimationMode.EASE_OUT_QUAD});
                        }});
                }
            }
        }
    }

    _syncClockTimer() {
        const needed = [...this._widgetOwners.values()].some(owner =>
            owner.widgets.some(widget => widget.clockFormat !== undefined));
        if (needed && !this._clockTimer) {
            this._clockTimer = GLib.timeout_add_seconds(GLib.PRIORITY_DEFAULT, 1, () => {
                this._refreshClocks();
                return GLib.SOURCE_CONTINUE;
            });
        } else if (!needed && this._clockTimer) {
            GLib.source_remove(this._clockTimer);
            this._clockTimer = 0;
        }
    }

    _widgetActor(widget) {
        const actor = new St.Widget({x: widget.x, y: widget.y, width: widget.width,
            height: widget.height, reactive: false, can_focus: false, track_hover: false});
        actor.set_clip_to_allocation(true);
        actor._wallInWidgetId = widget.id;
        if (widget.bytes) {
            const icon = new St.Icon({gicon: Gio.BytesIcon.new(new GLib.Bytes(widget.bytes)),
                icon_size: Math.min(widget.width, widget.height), reactive: false});
            actor.add_child(icon);
        }
        if (widget.text !== undefined) {
            const label = new St.Label({text: widget.clockFormat === undefined ? widget.text :
                this._clockText(widget, GLib.DateTime.new_now_local()), reactive: false,
                style: `color: ${widget.color}; font-size: ${widget.fontSize}px;`});
            label.set_size(widget.width, widget.height);
            label.set_clip_to_allocation(true);
            actor.add_child(label);
            if (widget.clockFormat !== undefined)
                actor._wallInClock = {widget, label};
        }
        return actor;
    }

    _addWidgetsToBackground(owner, background, monitorIndex) {
        if (owner.actors.has(background))
            return;
        const actors = [];
        try {
            for (const widget of owner.widgets) {
                if (this._monitor(widget.monitor)?.index !== monitorIndex)
                    continue;
                const actor = this._widgetActor(widget);
                actors.push(actor);
                background.add_child(actor);
            }
            owner.actors.set(background, actors);
        } catch (error) {
            for (const actor of actors)
                actor.destroy();
            throw error;
        }
    }

    _clearWidgetOwner(sender) {
        const owner = this._widgetOwners.get(sender);
        if (!owner)
            return;
        this._widgetOwners.delete(sender);
        Gio.bus_unwatch_name(owner.watch);
        for (const actors of owner.actors.values()) {
            for (const actor of actors)
                actor.destroy();
        }
        owner.actors.clear();
        this._syncClockTimer();
    }

    SetWidgetsAsync([document], invocation) {
        const sender = invocation.get_sender();
        let owner;
        try {
            owner = this._parseWidgets(document);
            const count = [...this._widgetOwners.entries()].reduce((total, [name, existing]) =>
                total + (name === sender ? 0 : existing.widgets.length), owner.widgets.length);
            if (count > MAX_WIDGETS_TOTAL)
                widgetError('at most 64 widgets across all senders');
            owner.actors = new Map();
            for (const [background, {monitorIndex}] of this._backgrounds)
                this._addWidgetsToBackground(owner, background, monitorIndex);
            if (owner.widgets.length)
                owner.watch = Gio.bus_watch_name_on_connection(Gio.DBus.session, sender,
                    Gio.BusNameWatcherFlags.NONE, null, () => {
                        if (this._widgetOwners.get(sender) === owner)
                            this._clearWidgetOwner(sender);
                    });
        } catch (error) {
            if (owner?.watch)
                Gio.bus_unwatch_name(owner.watch);
            for (const actors of owner?.actors?.values() ?? []) {
                for (const actor of actors)
                    actor.destroy();
            }
            invocation.return_dbus_error('io.wallin.Error', String(error));
            return;
        }
        this._clearWidgetOwner(sender);
        if (owner.widgets.length)
            this._widgetOwners.set(sender, owner);
        this._syncClockTimer();
        invocation.return_value(new GLib.Variant('(b)', [true]));
    }

    GetWidgetsAsync([], invocation) {
        invocation.return_value(new GLib.Variant('(s)', [
            this._widgetOwners.get(invocation.get_sender())?.document ?? '{"widgets":[]}',
        ]));
    }

    WidgetStatusAsync([], invocation) {
        const owner = this._widgetOwners.get(invocation.get_sender());
        const actors = [...(owner?.actors.values() ?? [])].flat();
        const widgets = (owner?.widgets ?? []).map(widget => {
            const matching = actors.filter(actor => actor._wallInWidgetId === widget.id);
            return {id: widget.id, actorCount: matching.length,
                text: matching[0]?._wallInClock?.label.text ??
                    (widget.clockFormat === undefined ? widget.text ?? null : null)};
        });
        invocation.return_value(new GLib.Variant('(s)', [JSON.stringify({widgets})]));
    }

    ClearWidgetsAsync([], invocation) {
        this._clearWidgetOwner(invocation.get_sender());
        invocation.return_value(null);
    }

    _requestFor(window) {
        if (!window)
            return null;
        for (const request of this._requests.values()) {
            if (window.get_pid() === request.pid && window.get_title() === `Wall-in:${request.token}` &&
                (window.get_gtk_application_id() === APP_ID || window.get_wm_class() === APP_ID))
                return request;
        }
        return null;
    }

    _scan() {
        for (const request of this._requests.values()) {
            if (request.error)
                continue;
            try {
                if (!this._monitor(request.monitor))
                    throw new Error(`Monitor disconnected: ${request.monitor}`);
                if (!request.actor) {
                    const actor = global.get_window_actors().find(
                        candidate => this._requestFor(candidate.meta_window) === request);
                    if (actor)
                        this._attach(request, actor);
                    else if (GLib.get_monotonic_time() - request.registeredAt > 15000000)
                        throw new Error('Renderer window did not map within 15 seconds');
                }
            } catch (error) {
                request.error = String(error);
                console.error(`Wall-in: ${error}`);
            }
        }
    }

    _attach(request, actor) {
        request.actor = actor;
        const window = actor.meta_window;
        window.hide_from_window_list();
        window.stick();
        window.lower();
        actor.remove_all_transitions();
        actor.set_scale(1, 1);
        actor.opacity = 255;
        actor.reactive = false;
        // Clutter.Clone paints a hidden source; hiding prevents both normal painting and picking.
        request.signals.push([actor, actor.connect('notify::visible', () => {
            if (actor.visible)
                actor.hide();
        })]);
        request.signals.push([window, window.connect('unmanaged', () => {
            request.error = 'Renderer window was destroyed';
            this._clearClones(request);
            request.actor = null;
        })]);
        actor.hide();
        this._removeOverviewPreview(window);
        this._position(request);
        this._addClones(request);
        this._restoreFocus();
    }

    _removeOverviewPreview(window) {
        const visit = actor => {
            if (actor instanceof Workspace.Workspace && actor.containsMetaWindow(window))
                actor._doRemoveWindow(window);
            for (const child of actor.get_children())
                visit(child);
        };
        visit(global.stage);
    }

    _position(request) {
        const monitor = this._monitor(request.monitor);
        if (!monitor || !request.actor)
            return;
        const window = request.actor.meta_window;
        window.move_to_monitor(monitor.index);
        window.move_resize_frame(false, monitor.x, monitor.y, monitor.width, monitor.height);
    }

    _restoreFocus() {
        const request = this._requestFor(global.display.focus_window);
        if (!request)
            return;
        const previous = request.previousFocus;
        if (previous && !this._requestFor(previous) &&
            global.get_window_actors().some(actor => actor.meta_window === previous))
            previous.activate(global.get_current_time());
        else
            global.display.unset_input_focus(global.get_current_time());
    }

    _trackBackground(actor, monitorIndex) {
        if (!actor || this._backgrounds.has(actor))
            return;
        const destroyId = actor.connect('destroy', () => {
            this._backgrounds.delete(actor);
            for (const request of this._requests.values())
                request.clones.delete(actor);
            for (const owner of this._widgetOwners.values())
                owner.actors.delete(actor);
        });
        this._backgrounds.set(actor, {monitorIndex, destroyId});
        for (const request of this._requests.values())
            this._addClone(request, actor, monitorIndex);
        for (const owner of this._widgetOwners.values())
            this._addWidgetsToBackground(owner, actor, monitorIndex);
    }

    _addClone(request, background, monitorIndex) {
        if (!request.actor || request.clones.has(background) ||
            this._monitor(request.monitor)?.index !== monitorIndex)
            return;
        const clone = new Clutter.Clone({source: request.actor, reactive: false});
        clone.add_constraint(new Clutter.BindConstraint({source: background,
            coordinate: Clutter.BindCoordinate.SIZE}));
        const firstWidget = [...this._widgetOwners.values()]
            .flatMap(owner => owner.actors.get(background) ?? [])[0];
        if (firstWidget)
            background.insert_child_below(clone, firstWidget);
        else
            background.add_child(clone);
        clone.set_clip_to_allocation(true);
        request.clones.set(background, clone);
    }

    _addClones(request) {
        for (const [actor, {monitorIndex}] of this._backgrounds)
            this._addClone(request, actor, monitorIndex);
    }

    _clearClones(request) {
        for (const clone of request.clones.values())
            clone.destroy();
        request.clones.clear();
    }

    _syncMonitors() {
        for (const owner of this._widgetOwners.values()) {
            for (const actors of owner.actors.values()) {
                for (const actor of actors)
                    actor.destroy();
            }
            owner.actors.clear();
            for (const [background, {monitorIndex}] of this._backgrounds)
                this._addWidgetsToBackground(owner, background, monitorIndex);
        }
        for (const request of this._requests.values()) {
            this._clearClones(request);
            if (!this._monitor(request.monitor)) {
                request.error = `Monitor disconnected: ${request.monitor}`;
                continue;
            }
            this._position(request);
            this._addClones(request);
        }
    }

    _remove(token) {
        const request = this._requests.get(token);
        if (!request)
            return;
        this._requests.delete(token);
        Gio.bus_unwatch_name(request.watch);
        this._clearClones(request);
        for (const [object, id] of request.signals) {
            try {
                object.disconnect(id);
            } catch {
                // Mutter may have finalized an already-unmanaged surface.
            }
        }
        if (request.actor)
            request.actor.meta_window.delete(global.get_current_time());
        if (this._requests.size === 0) {
            if (this._timer) {
                GLib.source_remove(this._timer);
                this._timer = 0;
            }
            if (this._unredirectDisabled) {
                global.compositor.enable_unredirect();
                this._unredirectDisabled = false;
            }
        }
    }

    disable() {
        this._enabled = false;
        this._dbus?.unexport();
        this._dbus = null;
        for (const token of this._requests.keys())
            this._remove(token);
        for (const sender of this._widgetOwners.keys())
            this._clearWidgetOwner(sender);
        for (const [object, id] of this._signals)
            object.disconnect(id);
        this._signals = [];
        this._injections.clear();
        for (const [actor, {destroyId}] of this._backgrounds)
            actor.disconnect(destroyId);
        this._backgrounds.clear();
    }
}
