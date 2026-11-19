active_list() {
  cat > "$OUT/l.js" <<'JS'
for (const w of workspace.windowList()) if (w.normalWindow) print("SIDESTAGE " + w.caption + " minimized=" + w.minimized + " active=" + w.active);
JS
  local id; id=$(qdbus6 org.kde.KWin /Scripting org.kde.kwin.Scripting.loadScript "$OUT/l.js" "sst-$1")
  echo "SIDESTAGE ---- $1" >&2
  qdbus6 org.kde.KWin /Scripting/Script$id org.kde.kwin.Script.run >/dev/null; sleep 0.4
  qdbus6 org.kde.KWin /Scripting org.kde.kwin.Scripting.unloadScript "sst-$1" >/dev/null
}
active_list before
click 120 610 right; sleep 0.6; shot 2
click 175 630; sleep 1.5; shot 3
active_list after
