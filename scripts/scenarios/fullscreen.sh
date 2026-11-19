area() {
  cat > "$OUT/a.js" <<'JS'
const s = workspace.screens;
for (let i = 0; i < s.length; i++) {
  const a = workspace.clientArea(KWin.MaximizeArea, s[i], workspace.currentDesktop);
  print("SIDESTAGE area screen" + i + " " + a.x + "," + a.y + " " + a.width + "x" + a.height);
}
JS
  local id; id=$(qdbus6 org.kde.KWin /Scripting org.kde.kwin.Scripting.loadScript "$OUT/a.js" "sst-a-$1")
  echo "SIDESTAGE ---- $1" >&2
  qdbus6 org.kde.KWin /Scripting/Script$id org.kde.kwin.Script.run >/dev/null; sleep 0.4
  qdbus6 org.kde.KWin /Scripting org.kde.kwin.Scripting.unloadScript "sst-a-$1" >/dev/null
}
area start
move 700 450
python3 "$ROOT/scripts/test-window.py" "Fullscreen on sidebar monitor" 0 12 full:1.5:7 >/dev/null 2>&1 &
sleep 4; shot 2
area during-fullscreen-1
sleep 5.5
area after-fullscreen-1
move 2100 450
python3 "$ROOT/scripts/test-window.py" "Fullscreen on other monitor" 0 9 full:1.5:7 >/dev/null 2>&1 &
sleep 4; shot 3
area during-fullscreen-2
