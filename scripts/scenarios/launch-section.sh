list_windows() {
  cat > "$OUT/list.js" <<'JS'
for (const w of workspace.windowList()) if (w.normalWindow) print("SIDESTAGE window " + w.caption + " | active=" + w.active);
JS
  local id; id=$(qdbus6 org.kde.KWin /Scripting org.kde.kwin.Scripting.loadScript "$OUT/list.js" "sidestage-list-$1")
  echo "SIDESTAGE ---- $1" >&2
  qdbus6 org.kde.KWin /Scripting/Script$id org.kde.kwin.Script.run >/dev/null; sleep 0.4
  qdbus6 org.kde.KWin /Scripting org.kde.kwin.Scripting.unloadScript "sidestage-list-$1" >/dev/null
}
click 90 72 right; sleep 0.6
click 150 92; sleep 0.4        # Launch section
click 90 72 right; sleep 0.5   # menu again during cool-down
shot 2
"$XINPUT" click 700 600; sleep 0.3   # dismiss menu
sleep 4; shot 3
list_windows after-section
click 47 195; sleep 3          # click the excluded favourite
list_windows after-excluded
click 50 264 right; sleep 0.6; shot 4   # empty section menu
