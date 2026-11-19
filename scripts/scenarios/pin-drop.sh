python3 "$ROOT/scripts/test-drag-source.py" file:///usr/share/applications/libreoffice-writer.desktop > "$OUT/drag.log" 2>&1 &
glide 400 450 2 450; sleep 1.0
click 220 33; sleep 0.5          # pin
glide 220 33 700 300; sleep 2.0  # leave: must stay open
echo "MARK pinned-and-away" >> "$OUT/marks"
shot 2
"$XINPUT" drag 840 450 120 420; sleep 2   # straight onto the sidebar
