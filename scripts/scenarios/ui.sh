# Launch, focus, drag-and-drop and settings checks.
active() {
  local id; id=$(qdbus6 org.kde.KWin /Scripting org.kde.kwin.Scripting.loadScript "$ROOT/scripts/scenarios/active.js" "sidestage-active-$1")
  qdbus6 org.kde.KWin /Scripting/Script$id org.kde.kwin.Script.run >/dev/null; sleep 0.3
  qdbus6 org.kde.KWin /Scripting org.kde.kwin.Scripting.unloadScript "sidestage-active-$1" >/dev/null
  echo "marker $1" >&2
}
active before-click
click 120 800        # empty sidebar area: must not take focus
active after-sidebar-click
"$XINPUT" drag 50 200 122 110; sleep 1   # VS Code tile -> Games section
shot 2
click 122 205        # first tile in Coding (Text Editor after the move)
sleep 4
shot 3
active after-launch
click 215 33         # gear -> Settings
sleep 2
shot 4
