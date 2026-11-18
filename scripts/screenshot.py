#!/usr/bin/env python3
"""Saves a PNG of the whole workspace via KWin ScreenShot2 (current D-Bus session)."""
import os, struct, sys, zlib, threading
import dbus

out = sys.argv[1]
bus = dbus.SessionBus()
iface = dbus.Interface(bus.get_object('org.kde.KWin', '/org/kde/KWin/ScreenShot2'), 'org.kde.KWin.ScreenShot2')
r, w = os.pipe()
chunks = []
t = threading.Thread(target=lambda: chunks.append(os.fdopen(r, 'rb').read()))
t.start()
meta = iface.CaptureWorkspace({'native-resolution': True}, dbus.types.UnixFd(w))
os.close(w)
t.join()
data = chunks[0]
width, height, stride, fmt = int(meta['width']), int(meta['height']), int(meta['stride']), int(meta['format'])
rows = []
for y in range(height):
    row = bytearray(data[y * stride: y * stride + width * 4])
    if fmt in (4, 5, 6):  # BGRA
        row[0::4], row[2::4] = row[2::4], row[0::4]
    rows.append(b'\x00' + bytes(row))
def chunk(t, d):
    c = t + d
    return struct.pack('>I', len(d)) + c + struct.pack('>I', zlib.crc32(c) & 0xffffffff)
png = b'\x89PNG\r\n\x1a\n' + chunk(b'IHDR', struct.pack('>IIBBBBB', width, height, 8, 6, 0, 0, 0)) \
    + chunk(b'IDAT', zlib.compress(b''.join(rows), 6)) + chunk(b'IEND', b'')
open(out, 'wb').write(png)
print(f"saved {out} {width}x{height} format={fmt}")
