# Restore a whole stack through the GNOME backend (the call the stack menu makes).
SIDESTAGE_BACKEND=gnome "$ROOT/target/debug/examples/probe" 6 "ALL:test-window.py" > "$OUT/probe.log" 2>&1
sleep 1
call ListWindows > "$OUT/after.json"
shot 2
