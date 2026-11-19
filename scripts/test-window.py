#!/usr/bin/env python3
"""Opens a colourful test window, minimizes it after a delay, closes later.
Used by the integration checks; never touches other windows."""
import sys
import gi
gi.require_version("Gtk", "3.0")
from gi.repository import Gtk, GLib

title = sys.argv[1] if len(sys.argv) > 1 else "Sidestage test window"
minimize_after = float(sys.argv[2]) if len(sys.argv) > 2 else 2
close_after = float(sys.argv[3]) if len(sys.argv) > 3 else 10
maximize = len(sys.argv) > 4 and sys.argv[4] == "max"
# "full:ON:OFF" enters fullscreen after ON seconds and leaves after OFF.
full = sys.argv[4].split(":")[1:] if len(sys.argv) > 4 and sys.argv[4].startswith("full") else None

w = Gtk.Window(title=title)
w.set_default_size(640, 400)
box = Gtk.Box(orientation=Gtk.Orientation.VERTICAL)
css = Gtk.CssProvider()
css.load_from_data(b"window { background: linear-gradient(135deg, #ff7a59, #6a5cff); } label { font-size: 40px; color: white; }")
Gtk.StyleContext.add_provider_for_screen(w.get_screen(), css, Gtk.STYLE_PROVIDER_PRIORITY_APPLICATION)
box.pack_start(Gtk.Label(label=title), True, True, 0)
w.add(box)
w.connect("destroy", Gtk.main_quit)
w.connect("window-state-event", lambda _w, e: print("state", e.new_window_state, flush=True))
w.show_all()
if maximize:
    w.maximize()
    GLib.timeout_add(1500, lambda: (print("geometry", *w.get_position(), *w.get_size(), flush=True), False)[1])
if full:
    GLib.timeout_add(int(float(full[0]) * 1000), lambda: (w.fullscreen(), False)[1])
    GLib.timeout_add(int(float(full[1]) * 1000), lambda: (w.unfullscreen(), False)[1])
if minimize_after > 0:
    GLib.timeout_add(int(minimize_after * 1000), lambda: (w.iconify(), False)[1])
GLib.timeout_add(int(close_after * 1000), lambda: (w.destroy(), False)[1])
Gtk.main()
