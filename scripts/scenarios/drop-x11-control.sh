GDK_BACKEND=x11 python3 "$ROOT/scripts/test-drop-target.py" > "$OUT/target.log" 2>&1 &
python3 "$ROOT/scripts/test-drag-source.py" file:///usr/share/applications/vlc.desktop > "$OUT/drag.log" 2>&1 &
sleep 3
cat > "$OUT/g.js" <<'JS'
for (const w of workspace.windowList()) if (w.caption == "Drag source" || w.caption == "Drop target") print("SIDESTAGE-GEO " + w.caption + " " + Math.round(w.frameGeometry.x + w.frameGeometry.width/2) + " " + Math.round(w.frameGeometry.y + w.frameGeometry.height/2));
JS
id=$(qdbus6 org.kde.KWin /Scripting org.kde.kwin.Scripting.loadScript "$OUT/g.js" sst-g); qdbus6 org.kde.KWin /Scripting/Script$id org.kde.kwin.Script.run >/dev/null; sleep 0.5
shot 2
"$XINPUT" drag 720 450 254 470; sleep 2
