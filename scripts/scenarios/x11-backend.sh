# EWMH backend against KWin's XWayland window manager, with X11 test windows.
GDK_BACKEND=x11 python3 "$ROOT/scripts/test-window.py" "X11 window A" 1.5 30 >/dev/null 2>&1 &
GDK_BACKEND=x11 python3 "$ROOT/scripts/test-window.py" "X11 window B" 0 30 >/dev/null 2>&1 &
SIDESTAGE_BACKEND=x11 "$ROOT/target/debug/examples/probe" 7 "X11 window A" "$OUT" > "$OUT/probe-x11.log" 2>&1
