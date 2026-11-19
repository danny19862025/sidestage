fsm() { echo "$1 $(call FullscreenMonitors)" >> "$OUT/fs.txt"; }
call DebugMovePointer 700 450 >/dev/null
python3 "$ROOT/scripts/test-window.py" "Fullscreen on sidebar monitor" 0 12 full:1.5:6 >/dev/null 2>&1 &
sleep 3.5; fsm during-1; shot 2
sleep 4; fsm after-1
call DebugMovePointer 2100 450 >/dev/null
python3 "$ROOT/scripts/test-window.py" "Fullscreen on other monitor" 0 10 full:1.5:7 >/dev/null 2>&1 &
sleep 3.5; fsm during-2
