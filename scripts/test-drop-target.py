#!/usr/bin/env python3
"""Minimal drop target; prints what it receives. Run with GDK_BACKEND=x11 to test XWayland drops."""
import gi
gi.require_version("Gtk", "3.0")
gi.require_version("Gdk", "3.0")
from gi.repository import Gtk, Gdk, GLib
w = Gtk.Window(title="Drop target")
w.set_default_size(300, 300)
w.move(100, 300)
lab = Gtk.Label(label="Drop here")
w.add(lab)
w.drag_dest_set(Gtk.DestDefaults.ALL, [], Gdk.DragAction.COPY | Gdk.DragAction.LINK)
w.drag_dest_add_uri_targets()
w.connect("drag-data-received", lambda _w, _c, _x, _y, data, _i, _t: print("RECEIVED", data.get_uris(), flush=True))
w.connect("drag-motion", lambda *a: (print("motion", flush=True), False)[1])
w.connect("destroy", Gtk.main_quit)
w.show_all()
GLib.timeout_add(30000, Gtk.main_quit)
Gtk.main()
