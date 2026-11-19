active_list() {
  cat > "$OUT/l.js" <<'JS'
for (const w of workspace.windowList()) if (w.normalWindow || w.dock) print("SIDESTAGE " + w.resourceClass + ":" + w.caption + " minimized=" + w.minimized + " active=" + w.active);
JS
  local id; id=$(qdbus6 org.kde.KWin /Scripting org.kde.kwin.Scripting.loadScript "$OUT/l.js" "sst-$1")
  echo "SIDESTAGE ---- $1" >&2
  qdbus6 org.kde.KWin /Scripting/Script$id org.kde.kwin.Script.run >/dev/null; sleep 0.4
  qdbus6 org.kde.KWin /Scripting org.kde.kwin.Scripting.unloadScript "sst-$1" >/dev/null
}
active_list before
"$XINPUT" keys ctrl+alt+space; sleep 1.2
"$XINPUT" type notes; sleep 0.8; shot 2
"$XINPUT" keys enter; sleep 1.5
active_list after-enter
"$XINPUT" keys ctrl+alt+space; sleep 1.2
"$XINPUT" type quar; sleep 0.8; shot 3
active_list during-search
"$XINPUT" keys esc; sleep 1.2
active_list after-esc
