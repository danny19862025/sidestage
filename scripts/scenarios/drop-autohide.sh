python3 "$ROOT/scripts/test-drag-source.py" file:///usr/share/applications/libreoffice-writer.desktop > "$OUT/drag.log" 2>&1 &
sleep 2.5
# Drag to the hidden sidebar's edge, wait for it to slide out, drop on "Everyday".
"$XINPUT" dragpath 840 450 3 420 120 420; sleep 2
shot 2
