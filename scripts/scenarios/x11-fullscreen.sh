GDK_BACKEND=x11 python3 "$ROOT/scripts/test-window.py" "X11 fullscreen" 0 12 full:1.5:4.5 >/dev/null 2>&1 &
SIDESTAGE_BACKEND=x11 "$ROOT/target/debug/examples/probe" 7 > "$OUT/probe-x11.log" 2>&1
