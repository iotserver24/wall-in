import assert from 'node:assert/strict';
import {readFileSync} from 'node:fs';
import test from 'node:test';

const source = readFileSync(new URL('../gnome/wall-in@r3ap3r/extension.js', import.meta.url), 'utf8')
    .replace(/^import .*;\n/gm, '')
    .replace('export default class WallInExtension', 'return class WallInExtension');

class Actor {
    constructor(properties = {}) {
        Object.assign(this, {width: 1920, height: 1080, visible: true, children: [], signals: new Map()}, properties);
    }
    connect(name, callback) { const id = Symbol(name); this.signals.set(id, {name, callback}); return id; }
    disconnect(id) { this.signals.delete(id); }
    add_child(actor) { this.children.push(actor); actor.parent = this; }
    get_parent() { return this.parent ?? null; }
    ease(options) { this.opacity = options.opacity; options.onComplete?.(); }
    insert_child_below(actor, sibling) {
        this.children.splice(this.children.indexOf(sibling), 0, actor);
    }
    get_children() { return this.children; }
    get_texture() { return {}; }
    add_constraint(constraint) { this.constraint = constraint; }
    set_clip_to_allocation(clip) { this.clip = clip; }
    set_size(width, height) { this.width = width; this.height = height; }
    remove_all_transitions() {}
    set_scale(x, y) { this.scale = [x, y]; }
    hide() { this.visible = false; }
    destroy() {
        this.destroyed = true;
        this.parent = null;
        for (const {name, callback} of this.signals.values()) {
            if (name === 'destroy') callback();
        }
    }
}

function fixture() {
    let actualPid = 42;
    let nextWatch = 0;
    const watches = new Map();
    const compositor = {disabled: 0, enabled: 0,
        disable_unredirect() { this.disabled++; }, enable_unredirect() { this.enabled++; }};
    const monitor = {index: 0, x: 0, y: 0, width: 1920, height: 1080};
    let connected = true;
    let clockTime = '12:30:01';
    let clockTick = null;
    const manager = {
        get_monitors: () => connected ? [{get_connector: () => 'eDP-1'}] : [],
        get_monitor_for_connector: () => 0,
    };
    const shell = {
        compositor,
        stage: new Actor(),
        window_manager: new Actor(),
        backend: {get_monitor_manager: () => manager},
        display: new Actor({focus_window: null, unset_input_focus() { this.focusReset = true; }}),
        get_current_time: () => 10,
        get_window_actors: () => actors,
    };
    const actors = [];
    const background = new Actor();
    const Main = {layoutManager: new Actor({monitors: [monitor],
        _bgManagers: [{backgroundActor: background, _monitorIndex: 0}]}),
    wm: {_shouldAnimateActor: () => true}};
    const Background = {BackgroundManager: class { _createBackgroundActor() { return new Actor(); } }};
    const Workspace = {Workspace: class extends Actor {
        _isOverviewWindow(window) { return !window.skip_taskbar; }
        containsMetaWindow(window) { return this.previews?.includes(window) ?? false; }
        _doRemoveWindow(window) { this.previews = this.previews.filter(preview => preview !== window); }
    }};
    const WorkspaceAnimation = {WorkspaceGroup: class { _shouldShowWindow() { return true; } }};
    class InjectionManager {
        saved = [];
        overrideMethod(object, name, factory) {
            this.saved.push([object, name, object[name]]);
            object[name] = factory(object[name]);
        }
        clear() { for (const [object, name, method] of this.saved) object[name] = method; }
    }
    const GLib = {Variant: class { constructor(signature, values) { this.values = values; } },
        VariantType: class {}, Bytes: class { constructor(bytes) { this.bytes = bytes; } },
        base64_decode: data => Uint8Array.from(Buffer.from(data, 'base64')),
        PRIORITY_DEFAULT: 0, SOURCE_CONTINUE: true,
        DateTime: {new_now_local: () => ({format: () => clockTime})},
        get_monotonic_time: () => 1000, timeout_add: () => 1,
        timeout_add_seconds: (_priority, _seconds, callback) => { clockTick = callback; return 2; },
        source_remove: id => { if (id === 2) clockTick = null; }};
    const Gio = {DBus: {session: {call(_a, _b, _c, _d, _e, _f, _g, _h, _i, callback) {
        callback({call_finish: () => ({deepUnpack: () => [actualPid]})}, {});
    }}}, BytesIcon: {new: bytes => ({bytes})},
    DBusCallFlags: {NONE: 0}, BusNameWatcherFlags: {NONE: 0},
    bus_watch_name_on_connection: (_bus, sender, _flags, _appeared, vanished) => {
        const id = ++nextWatch;
        watches.set(id, {sender, vanished});
        return id;
    },
    bus_unwatch_name: id => watches.delete(id),
    DBusExportedObject: {wrapJSObject: () => ({export() {}, unexport() {}})}};
    const ExtensionClass = new Function('Clutter', 'Gio', 'GLib', 'Meta', 'St', 'Extension',
        'InjectionManager', 'Background', 'Main', 'Workspace', 'WorkspaceAnimation', 'global', source)(
        {Clone: Actor, BindConstraint: class { constructor(p) { Object.assign(this, p); } },
            BindCoordinate: {SIZE: 1}, AnimationMode: {EASE_OUT_QUAD: 1}}, Gio, GLib,
        {Window: class { hide_from_window_list() {} }, BackgroundActor: class extends Actor {}},
        {Widget: Actor, Label: Actor, Icon: Actor}, class {}, InjectionManager,
        Background, Main, Workspace, WorkspaceAnimation, shell);
    const extension = new ExtensionClass();
    extension.enable();
    function invocation(sender = ':1.1') {
        return {get_sender: () => sender, return_value(value) { this.value = value?.values; },
            return_dbus_error(name, message) { this.error = {name, message}; }};
    }
    function register(token = 'a'.repeat(32), sender = ':1.1') {
        const call = invocation(sender);
        extension.RegisterAsync([token, 42, 'eDP-1'], call);
        return call;
    }
    function actor(token = 'a'.repeat(32)) {
        const window = new Actor({
            get_pid: () => 42, get_title: () => `Wall-in:${token}`,
            get_gtk_application_id: () => 'io.wallin.Renderer',
            get_monitor: () => 0, is_fullscreen: () => false,
            showing_on_its_workspace: () => true,
            hide_from_window_list() { this.hiddenFromList = true; },
            stick() { this.sticky = true; }, lower() {}, move_to_monitor() {}, move_resize_frame() {},
            delete() { this.closed = true; },
        });
        const actor = new Actor({meta_window: window});
        actors.push(actor);
        return actor;
    }
    return {extension, compositor, Main, background, Background, Workspace, WorkspaceAnimation, shell,
        register, actor, invocation, watches, setPid: pid => { actualPid = pid; },
        disconnectMonitor: () => { connected = false; },
        setClockTime: value => { clockTime = value; },
        tickClock: () => clockTick?.(),
        clockTimerActive: () => !!clockTick};
}

test('probe uses GNOME 50 physical connector to logical monitor mapping', () => {
    const f = fixture();
    const probe = JSON.parse(f.extension.Probe());
    assert.equal(probe.supported, true);
    assert.equal(probe.version, 2);
    assert.deepEqual(probe.widgets, {version: 1, maxPerSender: 32, maxTotal: 64,
        maxDocument: 524288, maxImageBytes: 131072, imageMime: 'image/png'});
    assert.deepEqual(probe.monitors, [{id: 'eDP-1', name: 'eDP-1', width: 1920, height: 1080, index: 0}]);
    f.extension.disable();
});

test('registered D-Bus PID must match the sender credentials', () => {
    const f = fixture();
    f.setPid(999);
    assert.deepEqual(f.register().value, [false]);
    assert.equal(f.extension._requests.size, 0);
    f.extension.disable();
});

test('duplicate monitor registration is rejected without changing the current background', () => {
    const f = fixture();
    assert.deepEqual(f.register().value, [true]);
    assert.deepEqual(f.register('b'.repeat(32), ':1.2').value, [false]);
    assert.equal(f.compositor.disabled, 1);
    f.extension.disable();
    assert.equal(f.compositor.enabled, 1);
});

test('source is hidden before map animation and clone is in the real background actor', () => {
    const f = fixture();
    f.register();
    const actor = f.actor();
    assert.equal(f.Main.wm._shouldAnimateActor(actor), false);
    assert.equal(actor.visible, false);
    assert.equal(actor.meta_window.hiddenFromList, true);
    assert.equal(actor.meta_window.sticky, true);
    assert.equal(f.background.children[0].source, actor);
    assert.equal(f.background.children[0].reactive, false);
    const status = f.invocation();
    f.extension.StatusAsync(['a'.repeat(32)], status);
    assert.equal(JSON.parse(status.value[0]).attached, true);
    assert.equal(new f.WorkspaceAnimation.WorkspaceGroup()._shouldShowWindow(actor.meta_window), false);
    f.extension.disable();
    assert.equal(actor.meta_window.closed, true);
    assert.equal(f.background.children[0].destroyed, true);
    assert.equal(f.Main.wm._shouldAnimateActor(actor), true);
});

test('a preview created before renderer attachment is removed without stopping playback', () => {
    const f = fixture();
    f.register();
    const actor = f.actor();
    const workspace = new f.Workspace.Workspace({previews: [actor.meta_window]});
    f.shell.stage.add_child(workspace);
    assert.equal(f.Main.wm._shouldAnimateActor(actor), false);
    assert.deepEqual(workspace.previews, []);
    assert.equal(f.extension._requests.get('a'.repeat(32)).actor, actor);
    assert.equal(f.background.children[0].source, actor);
    f.extension.disable();
});

test('future overview/workspace background actors receive live clones', () => {
    const f = fixture();
    f.register();
    f.Main.wm._shouldAnimateActor(f.actor());
    const manager = new f.Background.BackgroundManager();
    manager._monitorIndex = 0;
    const background = manager._createBackgroundActor();
    assert.equal(background.children.length, 1);
    background.destroy();
    assert.equal(f.extension._backgrounds.has(background), false);
    assert.equal(f.extension._requests.get('a'.repeat(32)).clones.has(background), false);
    f.extension.disable();
});

test('other D-Bus clients cannot control or unregister a renderer', () => {
    const f = fixture();
    f.register();
    const call = f.invocation(':1.9');
    f.extension.UnregisterAsync(['a'.repeat(32)], call);
    assert.equal(call.error.name, 'io.wallin.Error');
    assert.equal(f.extension._requests.size, 1);
    f.extension.disable();
});

test('monitor unplug removes clones and exposes a failure instead of fake readiness', () => {
    const f = fixture();
    f.register();
    f.Main.wm._shouldAnimateActor(f.actor());
    f.disconnectMonitor();
    f.extension._syncMonitors();
    const status = f.invocation();
    f.extension.StatusAsync(['a'.repeat(32)], status);
    const payload = JSON.parse(status.value[0]);
    assert.equal(payload.attached, false);
    assert.match(payload.error, /disconnected/);
    assert.equal(f.background.children[0].destroyed, true);
    f.extension.disable();
});

test('fullscreen policy only sees visible fullscreen windows on the selected monitor', () => {
    const f = fixture();
    f.register();
    const source = f.actor();
    f.Main.wm._shouldAnimateActor(source);
    source.meta_window.is_fullscreen = () => true;
    const other = f.actor('b'.repeat(32));
    other.meta_window.is_fullscreen = () => true;
    other.meta_window.get_monitor = () => 1;
    const status = () => {
        const call = f.invocation();
        f.extension.StatusAsync(['a'.repeat(32)], call);
        return JSON.parse(call.value[0]);
    };
    assert.equal(status().fullscreen, false);
    other.meta_window.get_monitor = () => 0;
    assert.equal(status().fullscreen, true);
    other.meta_window.showing_on_its_workspace = () => false;
    assert.equal(status().fullscreen, false);
    f.extension.disable();
});

test('status diagnoses mapped 0x0 sources without exposing another sender registration', () => {
    const f = fixture();
    f.register();
    const actor = f.actor();
    const status = (sender = ':1.1') => {
        const call = f.invocation(sender);
        f.extension.StatusAsync(['a'.repeat(32)], call);
        return call;
    };
    assert.deepEqual(JSON.parse(status().value[0]).diagnostics, {
        actorExists: false, actorWidth: 0, actorHeight: 0, hasTexture: false,
        cloneCount: 0, backgroundCount: 1, matchingWindowCount: 1,
    });
    assert.equal(status(':1.9').error.name, 'io.wallin.Error');
    actor.width = 0;
    actor.height = 0;
    f.Main.wm._shouldAnimateActor(actor);
    assert.deepEqual(JSON.parse(status().value[0]), {
        attached: false, error: '', fullscreen: false,
        diagnostics: {actorExists: true, actorWidth: 0, actorHeight: 0,
            hasTexture: true, cloneCount: 1, backgroundCount: 1, matchingWindowCount: 1},
    });
    actor.get_texture = () => null;
    actor.width = 1920;
    actor.height = 1080;
    assert.equal(JSON.parse(status().value[0]).diagnostics.hasTexture, false);
    assert.equal(JSON.parse(status().value[0]).attached, false);
    actor.get_texture = () => ({});
    assert.equal(JSON.parse(status().value[0]).attached, true);
    assert.equal(actor.meta_window.hiddenFromList, true);
    f.extension.disable();
});

test('status diagnostics clamp geometry and counts to bounded values', () => {
    const f = fixture();
    f.register();
    const actor = f.actor();
    actor.width = 20000;
    actor.height = -10;
    f.Main.wm._shouldAnimateActor(actor);
    for (let i = 0; i < 70; i++) {
        f.actor();
        f.extension._trackBackground(new Actor(), 0);
    }
    const call = f.invocation();
    f.extension.StatusAsync(['a'.repeat(32)], call);
    assert.deepEqual(JSON.parse(call.value[0]).diagnostics, {
        actorExists: true, actorWidth: 16384, actorHeight: 0, hasTexture: true,
        cloneCount: 64, backgroundCount: 64, matchingWindowCount: 64,
    });
    f.extension.disable();
});

test('renderer focus is explicitly cleared when there was no previous window', () => {
    const f = fixture();
    f.register();
    const actor = f.actor();
    f.shell.display.focus_window = actor.meta_window;
    f.Main.wm._shouldAnimateActor(actor);
    assert.equal(f.shell.display.focusReset, true);
    f.extension.disable();
});

test('native map signal attaches even if another extension bypasses animation filtering', () => {
    const f = fixture();
    f.register();
    const actor = f.actor();
    for (const {name, callback} of f.shell.window_manager.signals.values()) {
        if (name === 'map') callback(f.shell.window_manager, actor);
    }
    assert.equal(actor.visible, false);
    assert.equal(f.background.children[0].source, actor);
    f.extension.disable();
});

const widget = (id = 'clock', overrides = {}) => ({id, monitor: 'eDP-1', x: 10, y: 20,
    width: 240, height: 80, text: '<b>10:30</b>', color: '#aabbcc', font_size: 24, ...overrides});

function setWidgets(f, widgets, sender = ':1.1') {
    const call = f.invocation(sender);
    f.extension.SetWidgetsAsync([JSON.stringify({widgets})], call);
    return call;
}

test('native widget API uses plain noninteractive background actors and preserves wallpaper registration', () => {
    const f = fixture();
    assert.match(source, /<method name="SetWidgets"/);
    assert.match(source, /<method name="GetWidgets"/);
    assert.match(source, /<method name="ClearWidgets"/);
    assert.deepEqual(setWidgets(f, [widget()]).value, [true]);
    const drawn = f.background.children[0];
    assert.equal(drawn.reactive, false);
    assert.equal(drawn.can_focus, false);
    assert.equal(drawn.track_hover, false);
    assert.equal(drawn.clip, true);
    assert.equal(drawn.children[0].text, '<b>10:30</b>');
    assert.equal(drawn.children[0].style, 'color: #aabbcc; font-size: 24px;');
    assert.deepEqual(f.register().value, [true]);
    const wallpaper = f.actor();
    f.Main.wm._shouldAnimateActor(wallpaper);
    assert.equal(f.background.children[0].source, wallpaper);
    assert.equal(f.background.children[1], drawn);
    assert.equal(f.compositor.disabled, 1);
    f.extension.disable();
    assert.equal(drawn.destroyed, true);
    assert.equal(f.watches.size, 0);
});

test('replacement, per-sender query and clear isolate owners and reclaim actors', () => {
    const f = fixture();
    const first = f.background.children[0];
    assert.equal(first, undefined);
    setWidgets(f, [widget()]);
    const old = f.background.children[0];
    setWidgets(f, [widget('new', {text: 'new'})], ':1.2');
    const other = f.invocation(':1.2');
    f.extension.GetWidgetsAsync([], other);
    assert.equal(JSON.parse(other.value[0]).widgets[0].id, 'new');
    const stranger = f.invocation(':1.9');
    f.extension.GetWidgetsAsync([], stranger);
    assert.deepEqual(JSON.parse(stranger.value[0]), {widgets: []});
    f.extension.ClearWidgetsAsync([], stranger);
    assert.equal(f.extension._widgetOwners.size, 2);
    setWidgets(f, [widget('updated')]);
    assert.equal(old.destroyed, true);
    const query = f.invocation();
    f.extension.GetWidgetsAsync([], query);
    assert.equal(JSON.parse(query.value[0]).widgets[0].id, 'updated');
    const clear = f.invocation(':1.2');
    f.extension.ClearWidgetsAsync([], clear);
    assert.equal(f.extension._widgetOwners.size, 1);
    assert.equal(f.background.children[1].destroyed, true);
    assert.deepEqual(setWidgets(f, []).value, [true]);
    assert.equal(f.extension._widgetOwners.size, 0);
    assert.equal(f.watches.size, 0);
    f.extension.disable();
});

test('stale sender watch cannot clear a replacement and a failed update keeps the old document', () => {
    const f = fixture();
    assert.deepEqual(setWidgets(f, [widget()]).value, [true]);
    const oldOwner = f.extension._widgetOwners.get(':1.1');
    const staleWatch = [...f.watches.values()][0];
    const oldActor = f.background.children[0];
    f.background.add_child = () => { throw new Error('background unavailable'); };
    const failed = setWidgets(f, [widget('failed')]);
    assert.equal(failed.error.name, 'io.wallin.Error');
    assert.equal(f.extension._widgetOwners.get(':1.1'), oldOwner);
    assert.equal(oldActor.destroyed, undefined);
    assert.equal(f.watches.size, 1);
    f.background.add_child = Actor.prototype.add_child;
    assert.deepEqual(setWidgets(f, [widget('replacement')]).value, [true]);
    assert.equal(oldActor.destroyed, true);
    staleWatch.vanished();
    assert.equal(f.extension._widgetOwners.get(':1.1').widgets[0].id, 'replacement');
    const currentWatch = [...f.watches.values()][0];
    currentWatch.vanished();
    assert.equal(f.extension._widgetOwners.size, 0);
    assert.equal(f.watches.size, 0);
    f.extension.disable();
});

test('future workspace backgrounds receive widgets; sender disconnect and monitor unplug remove them', () => {
    const f = fixture();
    setWidgets(f, [widget()]);
    const manager = new f.Background.BackgroundManager();
    manager._monitorIndex = 0;
    const workspace = manager._createBackgroundActor();
    assert.equal(workspace.children.length, 1);
    f.disconnectMonitor();
    f.extension._syncMonitors();
    assert.equal(workspace.children[0].destroyed, true);
    assert.equal(f.background.children[0].destroyed, true);
    const watch = [...f.watches.values()][0];
    watch.vanished();
    assert.equal(f.extension._widgetOwners.size, 0);
    workspace.destroy();
    f.extension.disable();
});

test('bounded PNG bytes icons do not expose file paths or SVG loaders', () => {
    const f = fixture();
    const png = Buffer.from('iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO+j9XcAAAAASUVORK5CYII=', 'base64');
    assert.deepEqual(setWidgets(f, [widget('image', {text: 'safe', image: {
        mime: 'image/png', data: png.toString('base64'),
    }})]).value, [true]);
    assert.deepEqual(Buffer.from(f.background.children[0].children[0].gicon.bytes.bytes), png);
    assert.deepEqual(setWidgets(f, [widget('image-only', {text: undefined, image: {
        mime: 'image/png', data: png.toString('base64'),
    }})]).value, [true]);
    assert.equal(f.background.children[1].children.length, 1);
    const previous = f.extension._widgetOwners.get(':1.1');
    for (const image of [
        {mime: 'image/svg+xml', data: 'AA=='},
        {mime: 'image/png', data: 'AA=='},
        {mime: 'image/png', data: png.toString('base64'), path: '/etc/passwd'},
        {mime: 'image/png', data: Buffer.concat([png.subarray(0, 16),
            Buffer.from([0, 0, 16, 0]), png.subarray(20)]).toString('base64')},
    ]) {
        const rejected = setWidgets(f, [widget('bad', {image})]);
        assert.equal(rejected.error.name, 'io.wallin.Error');
        assert.equal(f.extension._widgetOwners.get(':1.1'), previous);
    }
    f.extension.disable();
});

test('malformed, oversized, offscreen, duplicate and executable-looking fields are rejected atomically', () => {
    const f = fixture();
    setWidgets(f, [widget()]);
    const owner = f.extension._widgetOwners.get(':1.1');
    const invalid = [
        [widget('a'), widget('a')],
        [widget('a', {x: 1900})],
        [widget('a', {monitor: 'unplugged'})],
        [widget('a', {text: '\u0000'})],
        [widget('a', {color: 'red; background: url(file:///etc/passwd)'})],
        [widget('a', {command: 'rm -rf /'})],
        [widget('a', {font_size: 500})],
        [widget('a', {width: 0})],
        [widget('a', {x: 1.5})],
        [widget('a', {text: undefined})],
        [widget('a', {image: {mime: 'image/png', data: '%%%'}})],
        Array.from({length: 33}, (_, i) => widget(`w${i}`)),
    ];
    for (const widgets of invalid) {
        const call = setWidgets(f, widgets);
        assert.equal(call.error.name, 'io.wallin.Error');
        assert.equal(f.extension._widgetOwners.get(':1.1'), owner);
    }
    for (const document of ['not JSON', '{"widgets":{},"command":"oops"}', 'x'.repeat(524289)]) {
        const call = f.invocation();
        f.extension.SetWidgetsAsync([document], call);
        assert.equal(call.error.name, 'io.wallin.Error');
    }
    assert.equal(f.background.children[0].destroyed, undefined);
    f.extension.disable();
});

test('widget cap is shared across senders, but replacing a document does not count twice', () => {
    const f = fixture();
    const many = Array.from({length: 32}, (_, i) => widget(`w${i}`));
    assert.deepEqual(setWidgets(f, many).value, [true]);
    assert.deepEqual(setWidgets(f, many, ':1.2').value, [true]);
    assert.deepEqual(setWidgets(f, many).value, [true]);
    const third = setWidgets(f, [widget('extra')], ':1.3');
    assert.equal(third.error.name, 'io.wallin.Error');
    assert.equal(f.extension._widgetOwners.size, 2);
    f.extension.disable();
});

test('clock text updates and fades on one bounded timer without replacing background actors', () => {
    const f = fixture();
    const clock = widget('live', {text: 'Time %1', clock_format: '%H:%M:%S'});
    assert.deepEqual(setWidgets(f, [clock]).value, [true]);
    const actor = f.background.children[0];
    assert.equal(actor.children[0].text, 'Time 12:30:01');
    assert.equal(f.clockTimerActive(), true);
    f.setClockTime('12:30:02');
    f.tickClock();
    assert.equal(actor.children[0].text, 'Time 12:30:02');
    assert.equal(actor.children[0].opacity, 255);
    assert.equal(f.background.children[0], actor);
    const status = f.invocation();
    f.extension.WidgetStatusAsync([], status);
    assert.deepEqual(JSON.parse(status.value[0]).widgets,
        [{id: 'live', actorCount: 1, text: 'Time 12:30:02'}]);
    const stranger = f.invocation(':1.9');
    f.extension.WidgetStatusAsync([], stranger);
    assert.deepEqual(JSON.parse(stranger.value[0]), {widgets: []});
    const workspace = new f.Background.BackgroundManager();
    workspace._monitorIndex = 0;
    const other = workspace._createBackgroundActor();
    assert.equal(other.children[0].children[0].text, 'Time 12:30:02');
    f.setClockTime('12:30:03');
    f.tickClock();
    assert.equal(other.children[0].children[0].text, 'Time 12:30:03');
    setWidgets(f, []);
    assert.equal(f.clockTimerActive(), false);
    f.extension.disable();
});

test('invalid clock directives and templates leave the previous widget document intact', () => {
    const f = fixture();
    setWidgets(f, [widget()]);
    const owner = f.extension._widgetOwners.get(':1.1');
    for (const fields of [
        {text: '%1', clock_format: '%n'},
        {text: '%1', clock_format: '%'},
        {text: '%1 %1', clock_format: '%H'},
        {text: 'No placeholder', clock_format: '%H'},
        {text: '%1', clock_format: 'x'.repeat(65)},
        {text: '%1', clock_format: '%H\n'},
    ]) {
        assert.equal(setWidgets(f, [widget('bad', fields)]).error.name, 'io.wallin.Error');
        assert.equal(f.extension._widgetOwners.get(':1.1'), owner);
    }
    assert.equal(f.clockTimerActive(), false);
    f.extension.disable();
});

test('renderer is excluded from overview previews while other windows remain visible', () => {
    const f = fixture();
    assert.equal(f.register().error, undefined);
    const rendererActor = f.actor();
    const normalActor = f.actor('b'.repeat(32));
    const workspace = new f.Workspace.Workspace();
    assert.equal(workspace._isOverviewWindow(rendererActor.meta_window), false);
    assert.equal(workspace._isOverviewWindow(normalActor.meta_window), true);
    assert.equal(new f.WorkspaceAnimation.WorkspaceGroup()._shouldShowWindow(rendererActor.meta_window), false);
    f.extension.disable();
    assert.equal(workspace._isOverviewWindow(rendererActor.meta_window), true);
});

test('unrelated normal windows retain default animation and workspace behavior', () => {
    const f = fixture();
    const actor = f.actor();
    assert.equal(f.Main.wm._shouldAnimateActor(actor), true);
    assert.equal(new f.Workspace.Workspace()._isOverviewWindow(actor.meta_window), true);
    assert.equal(new f.WorkspaceAnimation.WorkspaceGroup()._shouldShowWindow(actor.meta_window), true);
    f.extension.disable();
    assert.equal(actor.meta_window.closed, undefined);
});
