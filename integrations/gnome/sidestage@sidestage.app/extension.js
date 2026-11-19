// Sidestage GNOME Shell integration.
//
// GNOME (Mutter) does not expose other applications' windows to clients, and
// has no layer-shell, so this extension runs inside gnome-shell and exports
// org.sidestage.Shell1 on the session bus for the Sidestage app:
//
//   ListWindows() -> s               JSON array of normal windows
//   Activate(s id)                   unminimize + focus a window
//   Snapshot(s id, u max) -> s       PNG path of a still preview ('' if none)
//   SetReservedSpace(s monitor, s edge, u size)   strut for the sidebar
//   WatchPointer(i x, i y, i w, i h) report pointer crossings of the sidebar
//                                    (XWayland does not send LeaveNotify when
//                                    the pointer moves onto a Wayland window)
//   SetShortcut(s accel) -> (b, s)   global shortcut for Sidestage search
//   FocusPrevious()                  refocus the last non-Sidestage window
//   signal Changed()                 window set or state changed
//   signal ShortcutActivated()
//   signal PointerInside(b)
//
// Previews are captured when a window starts minimizing (its actor is still
// painted then) and cached until it is restored or closed.

import Clutter from 'gi://Clutter';
import Gio from 'gi://Gio';
import GLib from 'gi://GLib';
import Meta from 'gi://Meta';
import Shell from 'gi://Shell';
import St from 'gi://St';

import {Extension} from 'resource:///org/gnome/shell/extensions/extension.js';
import * as Main from 'resource:///org/gnome/shell/ui/main.js';
import * as PanelMenu from 'resource:///org/gnome/shell/ui/panelMenu.js';
import * as PopupMenu from 'resource:///org/gnome/shell/ui/popupMenu.js';

const BUS_NAME = 'org.sidestage.Shell';
const OBJECT_PATH = '/org/sidestage/Shell';
const IFACE_XML = `
<node>
  <interface name="org.sidestage.Shell1">
    <method name="ListWindows"><arg type="s" direction="out" name="json"/></method>
    <method name="Activate"><arg type="s" direction="in" name="id"/></method>
    <method name="Snapshot">
      <arg type="s" direction="in" name="id"/>
      <arg type="u" direction="in" name="max_size"/>
      <arg type="s" direction="out" name="png_path"/>
    </method>
    <method name="SetReservedSpace">
      <arg type="s" direction="in" name="monitor"/>
      <arg type="s" direction="in" name="edge"/>
      <arg type="u" direction="in" name="size"/>
    </method>
    <method name="DebugScreenshot">
      <arg type="s" direction="in" name="path"/>
    </method>
    <method name="DebugMovePointer">
      <arg type="i" direction="in" name="x"/>
      <arg type="i" direction="in" name="y"/>
    </method>
    <method name="DebugClick">
      <arg type="i" direction="in" name="x"/>
      <arg type="i" direction="in" name="y"/>
      <arg type="u" direction="in" name="button"/>
    </method>
    <method name="DebugKey">
      <arg type="s" direction="in" name="keys"/>
    </method>
    <method name="DebugGetenv">
      <arg type="s" direction="in" name="name"/>
      <arg type="s" direction="out" name="value"/>
    </method>
    <method name="WatchPointer">
      <arg type="i" direction="in" name="x"/>
      <arg type="i" direction="in" name="y"/>
      <arg type="i" direction="in" name="width"/>
      <arg type="i" direction="in" name="height"/>
    </method>
    <method name="SetShortcut">
      <arg type="s" direction="in" name="accelerator"/>
      <arg type="b" direction="out" name="ok"/>
      <arg type="s" direction="out" name="message"/>
    </method>
    <method name="FocusPrevious"/>
    <signal name="ShortcutActivated"/>
    <signal name="Changed"/>
    <signal name="PointerInside"><arg type="b" name="inside"/></signal>
  </interface>
</node>`;

function thumbDir() {
    const dir = GLib.build_filenamev([GLib.get_user_runtime_dir(), 'sidestage-gnome']);
    GLib.mkdir_with_parents(dir, 0o700);
    return dir;
}

function isListed(win) {
    if (!win || win.is_skip_taskbar())
        return false;
    const type = win.get_window_type();
    return type === Meta.WindowType.NORMAL || type === Meta.WindowType.DIALOG && !win.get_transient_for();
}

function spawn(argv) {
    try {
        Gio.Subprocess.new(argv, Gio.SubprocessFlags.NONE);
    } catch (e) {
        Main.notifyError('Sidestage', `Could not run ${argv[0]}: ${e.message}`);
    }
}

export default class SidestageExtension extends Extension {
    enable() {
        this._windowSignals = new Map();
        this._snapshots = new Map();   // window id -> png path
        this._minimizedAt = new Map(); // window -> monotonic µs
        this._strut = null;
        this._changeSource = 0;

        this._dbus = Gio.DBusExportedObject.wrapJSObject(IFACE_XML, this);
        this._dbus.export(Gio.DBus.session, OBJECT_PATH);
        this._nameId = Gio.bus_own_name_on_connection(Gio.DBus.session, BUS_NAME,
            Gio.BusNameOwnerFlags.REPLACE, null, null);

        this._grab = null;
        this._lastFocus = null;
        this._displaySignals = [
            global.display.connect('window-created', (_d, win) => {
                this._track(win);
                this._changed();
            }),
            global.display.connect('notify::focus-window', () => {
                const w = global.display.focus_window;
                if (w && !this._isSidestage(w))
                    this._lastFocus = w;
            }),
            global.display.connect('accelerator-activated', (_d, action) => {
                if (!this._grab || action !== this._grab.action)
                    return;
                // Give the sidebar keyboard focus, then let the app open search.
                const side = global.display.list_all_windows().find(w => this._isSidestage(w) && w.get_title() === 'Sidestage');
                if (side)
                    Main.activateWindow(side, global.get_current_time());
                this._dbus?.emit_signal('ShortcutActivated', null);
            }),
        ];
        for (const actor of global.get_window_actors())
            this._track(actor.meta_window);

        this._indicator = new PanelMenu.Button(0.0, 'Sidestage', false);
        this._indicator.add_child(new St.Icon({
            gicon: Gio.icon_new_for_string(`${this.path}/sidestage-symbolic.svg`),
            style_class: 'system-status-icon',
        }));
        const menu = this._indicator.menu;
        menu.addAction('Show Sidestage', () => spawn(['sidestage']));
        menu.addAction('Settings…', () => spawn(['sidestage', '--settings']));
        menu.addMenuItem(new PopupMenu.PopupSeparatorMenuItem());
        menu.addAction('Quit Sidestage', () => spawn(['sidestage', '--quit']));
        Main.panel.addToStatusArea(this.uuid, this._indicator);
    }

    disable() {
        // Let the app notice that integration went away.
        this._dbus?.emit_signal('Changed', null);
        if (this._changeSource) {
            GLib.source_remove(this._changeSource);
            this._changeSource = 0;
        }
        for (const [win, ids] of this._windowSignals)
            ids.forEach(id => win.disconnect(id));
        this._windowSignals.clear();
        this._displaySignals.forEach(id => global.display.disconnect(id));
        this._displaySignals = [];
        this.SetReservedSpace('', 'left', 0);
        this.WatchPointer(0, 0, 0, 0);
        this._ungrab();
        this._lastFocus = null;
        for (const path of this._snapshots.values())
            GLib.unlink(path);
        this._snapshots.clear();
        this._dbus.unexport();
        this._dbus = null;
        Gio.bus_unown_name(this._nameId);
        this._indicator.destroy();
        this._indicator = null;
    }

    _track(win) {
        if (!win || this._windowSignals.has(win))
            return;
        const ids = [
            win.connect('notify::minimized', () => {
                if (win.minimized)
                    this._minimizedAt.set(win, GLib.get_monotonic_time());
                else
                    this._minimizedAt.delete(win);
                if (win.minimized)
                    this._capture(win);
                else
                    this._dropSnapshot(win);
                this._changed();
            }),
            win.connect('notify::title', () => this._changed()),
            win.connect('notify::skip-taskbar', () => this._changed()),
            win.connect('unmanaged', () => {
                const sigs = this._windowSignals.get(win);
                sigs?.forEach(id => win.disconnect(id));
                this._windowSignals.delete(win);
                this._minimizedAt.delete(win);
                this._dropSnapshot(win);
                this._changed();
            }),
        ];
        this._windowSignals.set(win, ids);
    }

    _changed() {
        // Coalesce bursts into one signal.
        if (this._changeSource)
            return;
        this._changeSource = GLib.idle_add(GLib.PRIORITY_DEFAULT_IDLE, () => {
            this._changeSource = 0;
            this._dbus?.emit_signal('Changed', null);
            return GLib.SOURCE_REMOVE;
        });
    }

    _find(id) {
        return global.display.list_all_windows().find(w => String(w.get_id()) === id) ?? null;
    }

    _dropSnapshot(win) {
        const id = String(win.get_id());
        const path = this._snapshots.get(id);
        if (path) {
            GLib.unlink(path);
            this._snapshots.delete(id);
        }
    }

    async _capture(win) {
        try {
            const actor = win.get_compositor_private();
            if (!actor)
                return '';
            const content = actor.paint_to_content(null);
            const texture = content?.get_texture();
            if (!texture)
                return '';
            const id = String(win.get_id());
            const path = GLib.build_filenamev([thumbDir(), `thumb-${id}.png`]);
            const file = Gio.File.new_for_path(path);
            const stream = file.replace(null, false, Gio.FileCreateFlags.NONE, null);
            const scale = actor.get_resource_scale?.() ?? 1;
            await Shell.Screenshot.composite_to_stream(texture, 0, 0, -1, -1, scale, null, 0, 0, 1, stream);
            stream.close(null);
            this._snapshots.set(id, path);
            return path;
        } catch (e) {
            console.debug(`Sidestage: snapshot failed: ${e}`);
            return '';
        }
    }

    // --- D-Bus methods -----------------------------------------------------

    ListWindows() {
        const tracker = Shell.WindowTracker.get_default();
        const list = [];
        for (const win of global.display.list_all_windows()) {
            if (!isListed(win))
                continue;
            this._track(win);
            const app = tracker.get_window_app(win);
            // Window-backed apps (no .desktop file) get a per-window id;
            // group those by GTK application id / WM_CLASS instead.
            const known = app && !app.is_window_backed();
            list.push({
                id: String(win.get_id()),
                app_id: known ? app.get_id() : (win.get_gtk_application_id() ?? win.get_wm_class() ?? ''),
                app_name: known ? app.get_name() : '',
                title: win.get_title() ?? '',
                minimized: win.minimized,
                focused: win.has_focus(),
                // Windows minimized before tracking started count as oldest.
                minimized_at: win.minimized ? (this._minimizedAt.get(win) ?? 0) : null,
                icon: app?.get_app_info()?.get_icon()?.to_string() ?? '',
            });
        }
        return JSON.stringify(list);
    }

    Activate(id) {
        const win = this._find(id);
        if (!win)
            return;
        // Handles unminimize and workspace switching.
        Main.activateWindow(win, global.get_current_time());
    }

    async SnapshotAsync([id, _maxSize], invocation) {
        let path = this._snapshots.get(id) ?? '';
        if (!path) {
            const win = this._find(id);
            if (win)
                path = await this._capture(win);
        }
        // The app deletes the file after loading; re-capture next time.
        this._snapshots.delete(id);
        invocation.return_value(new GLib.Variant('(s)', [path]));
    }

    SetReservedSpace(monitorName, edge, size) {
        if (this._strut) {
            Main.layoutManager.removeChrome(this._strut);
            this._strut.destroy();
            this._strut = null;
        }
        if (!size)
            return;
        const lm = Main.layoutManager;
        let index = lm.primaryIndex;
        try {
            const mm = global.backend.get_monitor_manager();
            const i = mm.get_monitor_for_connector(monitorName);
            if (i >= 0)
                index = i;
        } catch (_e) {}
        const m = lm.monitors[index] ?? lm.primaryMonitor;
        // An invisible, non-reactive actor whose only job is the strut.
        this._strut = new St.Widget({reactive: false, opacity: 0, name: 'sidestage-strut'});
        this._strut.set_position(edge === 'right' ? m.x + m.width - size : m.x, m.y);
        this._strut.set_size(size, m.height);
        lm.addChrome(this._strut, {affectsStruts: true, trackFullscreen: true});
    }

    _isSidestage(win) {
        return (win.get_wm_class() ?? '').toLowerCase() === 'sidestage';
    }

    _ungrab() {
        if (!this._grab)
            return;
        global.display.ungrab_accelerator(this._grab.action);
        Main.wm.allowKeybinding(this._grab.name, Shell.ActionMode.NONE);
        this._grab = null;
    }

    SetShortcut(accel) {
        this._ungrab();
        if (!accel)
            return [true, 'disabled'];
        // Mutter refuses accelerators another binding already owns.
        const action = global.display.grab_accelerator(accel, Meta.KeyBindingFlags.NONE);
        if (action === Meta.KeyBindingAction.NONE)
            return [false, `${accel} is already used by GNOME or another extension`];
        const name = Meta.external_binding_name_for_action(action);
        Main.wm.allowKeybinding(name, Shell.ActionMode.NORMAL | Shell.ActionMode.OVERVIEW);
        this._grab = {action, name};
        return [true, `${accel} via GNOME Shell`];
    }

    FocusPrevious() {
        const w = this._lastFocus;
        if (w && !w.minimized && global.display.list_all_windows().includes(w))
            Main.activateWindow(w, global.get_current_time());
    }

    WatchPointer(x, y, width, height) {
        if (this._cursorId) {
            this._cursorTracker.disconnect(this._cursorId);
            this._cursorId = 0;
        }
        this._pointerRect = width > 0 && height > 0 ? {x, y, width, height} : null;
        this._pointerInside = null;
        if (!this._pointerRect)
            return;
        this._cursorTracker = global.backend.get_cursor_tracker();
        const check = () => {
            const [px, py] = global.get_pointer();
            const r = this._pointerRect;
            const inside = px >= r.x && px < r.x + r.width && py >= r.y && py < r.y + r.height;
            if (inside !== this._pointerInside) {
                this._pointerInside = inside;
                this._dbus?.emit_signal('PointerInside', new GLib.Variant('(b)', [inside]));
            }
        };
        this._cursorId = this._cursorTracker.connect('position-invalidated', check);
        check();
    }

    async DebugScreenshotAsync([path], invocation) {
        // Test hook used by scripts/nested-gnome-test.sh; only active when
        // gnome-shell runs with SIDESTAGE_DEBUG=1.
        if (GLib.getenv('SIDESTAGE_DEBUG') !== '1') {
            invocation.return_dbus_error('org.sidestage.Error.Disabled', 'debug hooks disabled');
            return;
        }
        try {
            Main.overview.hide();
            await new Promise(r => GLib.timeout_add(GLib.PRIORITY_DEFAULT, 400, () => r() ?? GLib.SOURCE_REMOVE));
            const shooter = new Shell.Screenshot();
            const stream = Gio.File.new_for_path(path).replace(null, false, Gio.FileCreateFlags.NONE, null);
            await shooter.screenshot(false, stream);
            stream.close(null);
            invocation.return_value(null);
        } catch (e) {
            invocation.return_dbus_error('org.sidestage.Error.Failed', String(e));
        }
    }

    DebugGetenv(name) {
        if (GLib.getenv('SIDESTAGE_DEBUG') !== '1')
            return '';
        return GLib.getenv(name) ?? '';
    }

    DebugMovePointer(x, y) {
        if (GLib.getenv('SIDESTAGE_DEBUG') !== '1')
            return;
        const seat = global.stage.context.get_backend().get_default_seat();
        if (!this._virtualPointer)
            this._virtualPointer = seat.create_virtual_device(Clutter.InputDeviceType.POINTER_DEVICE);
        this._virtualPointer.notify_absolute_motion(GLib.get_monotonic_time(), x, y);
    }

    DebugClick(x, y, button) {
        if (GLib.getenv('SIDESTAGE_DEBUG') !== '1')
            return;
        this.DebugMovePointer(x, y);
        const b = button || Clutter.BUTTON_PRIMARY;
        const now = () => GLib.get_monotonic_time();
        GLib.timeout_add(GLib.PRIORITY_DEFAULT, 80, () => {
            this._virtualPointer.notify_button(now(), b, Clutter.ButtonState.PRESSED);
            GLib.timeout_add(GLib.PRIORITY_DEFAULT, 60, () => {
                this._virtualPointer.notify_button(now(), b, Clutter.ButtonState.RELEASED);
                return GLib.SOURCE_REMOVE;
            });
            return GLib.SOURCE_REMOVE;
        });
    }

    // Test hook: presses a chord of keyvals given as "Control_L+Alt_L+space".
    DebugKey(keys) {
        if (GLib.getenv('SIDESTAGE_DEBUG') !== '1')
            return;
        const seat = global.stage.context.get_backend().get_default_seat();
        if (!this._virtualKeyboard)
            this._virtualKeyboard = seat.create_virtual_device(Clutter.InputDeviceType.KEYBOARD_DEVICE);
        const vals = keys.split('+').map(k => Clutter[`KEY_${k}`]);
        const kb = this._virtualKeyboard;
        let t = GLib.get_monotonic_time();
        for (const v of vals)
            kb.notify_keyval(t++, v, Clutter.KeyState.PRESSED);
        for (const v of vals.reverse())
            kb.notify_keyval(t++, v, Clutter.KeyState.RELEASED);
    }
}
