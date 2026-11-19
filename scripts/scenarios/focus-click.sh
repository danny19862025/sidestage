active() {
  cat > "$OUT/a.js" <<'JS'
for (const w of workspace.windowList()) if (w.active) print("SIDESTAGE active=" + w.resourceClass + ":" + w.caption);
JS
  local id; id=$(qdbus6 org.kde.KWin /Scripting org.kde.kwin.Scripting.loadScript "$OUT/a.js" "sst-a-$1")
  echo "SIDESTAGE ---- $1" >&2
  qdbus6 org.kde.KWin /Scripting/Script$id org.kde.kwin.Script.run >/dev/null; sleep 0.4
  qdbus6 org.kde.KWin /Scripting org.kde.kwin.Scripting.unloadScript "sst-a-$1" >/dev/null
}
active before
click 120 800; sleep 0.5
active after-empty-click
click 47 200; sleep 2
active after-tile-click
