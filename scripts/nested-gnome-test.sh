#!/usr/bin/env bash
# Runs Sidestage against a headless GNOME Shell with the Sidestage extension,
# in a throwaway home/config/D-Bus. Usage: scripts/nested-gnome-test.sh OUT_DIR [SCENARIO]
set -euo pipefail
OUT=${1:?output dir}; SCENARIO=${2:-}
ROOT=$(cd "$(dirname "$0")/.." && pwd)
BIN=${SIDESTAGE_BIN:-$ROOT/target/debug/sidestage}
rm -rf "$OUT"; mkdir -p "$OUT"/home/{config,data/gnome-shell/extensions,cache} "$OUT/r"
chmod 700 "$OUT/r"
cp -r "$ROOT/integrations/gnome/sidestage@sidestage.app" "$OUT/home/data/gnome-shell/extensions/"
cp "$ROOT/scripts/fixtures/${FIXTURE:-demo-config.json}" "$OUT/config.json"
cat > "$OUT/session.sh" <<INNER
#!/usr/bin/env bash
set -u
OUT="$OUT"; ROOT="$ROOT"
gsettings set org.gnome.shell enabled-extensions "['sidestage@sidestage.app']"
gsettings set org.gnome.shell disable-user-extensions false
gsettings set org.gnome.shell welcome-dialog-last-shown-version '999'
SIDESTAGE_DEBUG=1 gnome-shell --headless --wayland --virtual-monitor 1440x900 --wayland-display sst-gnome > "\$OUT/gnome-shell.log" 2>&1 &
SHELL_PID=\$!
for i in \$(seq 60); do
  gdbus call --session --dest org.freedesktop.DBus --object-path /org/freedesktop/DBus --method org.freedesktop.DBus.NameHasOwner org.sidestage.Shell 2>/dev/null | grep -q true && break
  sleep 0.5
done
call() { gdbus call --session --dest org.sidestage.Shell --object-path /org/sidestage/Shell --method org.sidestage.Shell1."\$@"; }
shot() { call DebugScreenshot "\$OUT/shot-\$1.png" >/dev/null; }
export WAYLAND_DISPLAY=sst-gnome XDG_CURRENT_DESKTOP=GNOME XDG_SESSION_TYPE=wayland
python3 "$ROOT/scripts/test-window.py" "Wake XWayland" 0 1 >/dev/null 2>&1 &
sleep 2
export DISPLAY=\$(call DebugGetenv DISPLAY | sed "s/[(',)]//g")
export XAUTHORITY=\$(call DebugGetenv XAUTHORITY | sed "s/[(',)]//g")
echo "DISPLAY=\$DISPLAY" > "\$OUT/env.txt"
export SIDESTAGE_CONFIG="\$OUT/config.json" RUST_LOG=\${RUST_LOG:-info,usvg=error}
"$BIN" > "\$OUT/sidestage.log" 2>&1 &
APP=\$!
sleep 3
python3 "$ROOT/scripts/test-window.py" "Quarterly report.odt" 1.5 90 >/dev/null 2>&1 &
python3 "$ROOT/scripts/test-window.py" "Design notes" 2.0 90 >/dev/null 2>&1 &
python3 "$ROOT/scripts/test-window.py" "Maximized window" 0 90 max >/dev/null 2>&1 &
sleep 6
call ListWindows > "\$OUT/windows.json"
shot 1
${SCENARIO:+source "$SCENARIO"}
kill \$APP 2>/dev/null; wait \$APP 2>/dev/null
kill \$SHELL_PID 2>/dev/null
INNER
chmod +x "$OUT/session.sh"
env -u WAYLAND_DISPLAY -u DISPLAY -u DBUS_SESSION_BUS_ADDRESS \
  XDG_CONFIG_HOME="$OUT/home/config" XDG_DATA_HOME="$OUT/home/data" XDG_CACHE_HOME="$OUT/home/cache" \
  XDG_RUNTIME_DIR="$OUT/r" XDG_CURRENT_DESKTOP=GNOME \
  timeout 120 dbus-run-session -- "$OUT/session.sh" > "$OUT/session.log" 2>&1 || true
ls "$OUT"
