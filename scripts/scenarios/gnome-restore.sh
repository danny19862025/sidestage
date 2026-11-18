# Restore "Design notes" through the GNOME backend (same D-Bus call the UI makes).
SIDESTAGE_BACKEND=gnome "$ROOT/target/debug/examples/probe" 6 "Design notes" "$OUT" > "$OUT/probe.log" 2>&1
sleep 1
call ListWindows > "$OUT/windows-after.json"
shot 2
