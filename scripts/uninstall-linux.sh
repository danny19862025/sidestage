#!/usr/bin/env bash
set -euo pipefail
PREFIX=${PREFIX:-$HOME/.local}
DATA=${XDG_DATA_HOME:-$HOME/.local/share}
"$PREFIX/bin/sidestage" --quit 2>/dev/null || true
gnome-extensions disable sidestage@sidestage.app 2>/dev/null || true
rm -rf "$PREFIX/bin/sidestage" "$DATA/applications/sidestage.desktop" \
  "$DATA/icons/hicolor/scalable/apps/sidestage.svg" "$DATA/gnome-shell/extensions/sidestage@sidestage.app" \
  "${XDG_CONFIG_HOME:-$HOME/.config}/autostart/sidestage.desktop"
echo "Sidestage removed. Your favourites remain in ${XDG_CONFIG_HOME:-$HOME/.config}/sidestage/config.json"
