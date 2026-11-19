#!/usr/bin/env bash
# Runs Sidestage inside an isolated, virtual KWin Wayland session (own D-Bus,
# own XWayland) with real test windows, runs a scenario, saves screenshots.
# Usage: scripts/nested-kwin-test.sh OUTPUT_DIR [SCENARIO_SCRIPT]
# The scenario is sourced inside the session with $OUT, $ROOT, shot NAME,
# click X Y and move X Y available.
set -euo pipefail
OUT=${1:?output dir}
SCENARIO=${2:-}
ROOT=$(cd "$(dirname "$0")/.." && pwd)
BIN=${SIDESTAGE_BIN:-$ROOT/target/debug/sidestage}
XINPUT=$ROOT/target/debug/examples/fakeinput
mkdir -p "$OUT"
rm -f "$OUT"/shot-*.png
cp "$ROOT/scripts/fixtures/${FIXTURE:-demo-config.json}" "$OUT/config.json"
cat > "$OUT/session.sh" <<INNER
#!/usr/bin/env bash
set -u
OUT="$OUT"; ROOT="$ROOT"
export SIDESTAGE_CONFIG="$OUT/config.json"
export RUST_LOG=\${RUST_LOG:-info,usvg=error}
export XDG_CURRENT_DESKTOP=KDE XDG_SESSION_TYPE=wayland
export PATH="$ROOT/scripts/testbin:\$PATH" SIDESTAGE_XDG_OPEN_LOG="$OUT/xdg-open.log"
shot() { python3 "$ROOT/scripts/screenshot.py" "$OUT/shot-\$1.png" >/dev/null; }
XINPUT="$XINPUT"
click() { "$XINPUT" click "\$@"; sleep 0.8; }
move() { "$XINPUT" move "\$@"; sleep 0.5; }
glide() { "$XINPUT" glide "\$@"; sleep 0.3; }
"$BIN" > "$OUT/sidestage.log" 2>&1 &
APP=\$!
sleep 3
if [ -z "\${NO_TEST_WINDOWS:-}" ]; then
python3 "$ROOT/scripts/test-window.py" "Quarterly report.odt" 1.5 90 >/dev/null 2>&1 &
python3 "$ROOT/scripts/test-window.py" "Design notes" 2.0 90 >/dev/null 2>&1 &
TEST_STRIPES="\${TEST_STRIPES:-}" python3 "$ROOT/scripts/test-window.py" "Maximized window" 0 90 max >/dev/null 2>&1 &
fi
sleep 5
shot 1
${SCENARIO:+source "$SCENARIO"}
kill \$APP 2>/dev/null; wait \$APP 2>/dev/null
echo "exit=\$?" >> "$OUT/sidestage.log"
INNER
chmod +x "$OUT/session.sh"
export -n WAYLAND_DISPLAY DISPLAY
# Isolate KDE config (kglobalshortcutsrc, kwinrc, ...) from the real session.
mkdir -p "$OUT/xdg-config"
export XDG_CONFIG_HOME="$OUT/xdg-config"
QT_FORCE_STDERR_LOGGING=1 QT_LOGGING_RULES="kwin_scripting.debug=true" TEST_STRIPES="${TEST_STRIPES:-}" NO_TEST_WINDOWS="${NO_TEST_WINDOWS:-}" timeout 150 dbus-run-session -- kwin_wayland --virtual --xwayland --output-count ${OUTPUTS:-1} --width 1440 --height 900 \
  --socket sidestage-test-wl --exit-with-session "$OUT/session.sh" > "$OUT/kwin.log" 2>&1 || true
ls "$OUT"
