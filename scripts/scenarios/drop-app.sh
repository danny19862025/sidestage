python3 "$ROOT/scripts/test-drag-source.py" "${DROP_URI:-file:///usr/share/applications/firefox.desktop}" > "$OUT/drag.log" 2>&1 &
sleep 2.5
shot 2
# Find the drag source window centre via KWin.
cat > "$OUT/g.js" <<'JS'
for (const w of workspace.windowList()) if (w.caption == "Drag source") print("SIDESTAGE-GEO " + Math.round(w.frameGeometry.x + w.frameGeometry.width/2) + " " + Math.round(w.frameGeometry.y + w.frameGeometry.height/2));
JS
id=$(qdbus6 org.kde.KWin /Scripting org.kde.kwin.Scripting.loadScript "$OUT/g.js" sst-g); qdbus6 org.kde.KWin /Scripting/Script$id org.kde.kwin.Script.run >/dev/null; sleep 0.5
# Drag from the source onto the "Everyday" section of the sidebar.
"$XINPUT" drag 840 450 120 420; sleep 2
shot 3
