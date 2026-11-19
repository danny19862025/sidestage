#!/usr/bin/env python3
"""A window whose whole area is a drag source offering text/uri-list.
Usage: test-drag-source.py URI [URI...]   (e.g. file:///usr/share/applications/firefox.desktop)"""
import sys
import gi
gi.require_version("Gtk", "3.0")
gi.require_version("Gdk", "3.0")
from gi.repository import Gtk, Gdk, GLib

uris = sys.argv[1:]
w = Gtk.Window(title="Drag source")
w.set_default_size(300, 200)
box = Gtk.EventBox()
box.add(Gtk.Label(label="Drag me"))
w.add(box)
box.drag_source_set(Gdk.ModifierType.BUTTON1_MASK, [], Gdk.DragAction.COPY | Gdk.DragAction.LINK)
box.drag_source_add_uri_targets()
def on_get(_w, _ctx, data, _info, _time):
    data.set_uris(uris)
    print("drag data requested", flush=True)
box.connect("drag-data-get", on_get)
box.connect("drag-end", lambda *a: print("drag end", flush=True))
w.connect("destroy", Gtk.main_quit)
w.move(700, 300)
w.show_all()
GLib.timeout_add(30000, Gtk.main_quit)
Gtk.main()
