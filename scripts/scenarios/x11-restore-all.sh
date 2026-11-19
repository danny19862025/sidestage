GDK_BACKEND=x11 python3 "$ROOT/scripts/test-window.py" "X11 first" 1.0 30 >/dev/null 2>&1 &
GDK_BACKEND=x11 python3 "$ROOT/scripts/test-window.py" "X11 second" 1.8 30 >/dev/null 2>&1 &
sleep 0.5
SIDESTAGE_BACKEND=x11 "$ROOT/target/debug/examples/probe" 6 "ALL:Test-window.py" > "$OUT/probe-x11.log" 2>&1
cat > "$OUT/l.js" <<'JS'
for (const w of workspace.windowList()) if (w.normalWindow) print("SIDESTAGE " + w.caption + " minimized=" + w.minimized + " active=" + w.active);
JS
id=$(qdbus6 org.kde.KWin /Scripting org.kde.kwin.Scripting.loadScript "$OUT/l.js" sst-l)
qdbus6 org.kde.KWin /Scripting/Script$id org.kde.kwin.Script.run >/dev/null; sleep 0.5
