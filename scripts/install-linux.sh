#!/usr/bin/env bash
# Builds Sidestage and installs it for the current user (no root needed).
#   scripts/install-linux.sh [--no-build]
set -euo pipefail
ROOT=$(cd "$(dirname "$0")/.." && pwd)
PREFIX=${PREFIX:-$HOME/.local}
DATA=${XDG_DATA_HOME:-$HOME/.local/share}
if [[ ${1:-} != --no-build ]]; then
  cargo build --release --manifest-path "$ROOT/Cargo.toml" -p sidestage
fi
install -Dm755 "$ROOT/target/release/sidestage" "$PREFIX/bin/sidestage"
install -Dm644 "$ROOT/packaging/linux/sidestage.svg" "$DATA/icons/hicolor/scalable/apps/sidestage.svg"
mkdir -p "$DATA/applications"
sed "s|@BIN@|$PREFIX/bin/sidestage|" "$ROOT/packaging/linux/sidestage.desktop.in" > "$DATA/applications/sidestage.desktop"
command -v update-desktop-database >/dev/null && update-desktop-database "$DATA/applications" || true

EXT=sidestage@sidestage.app
if [[ ${XDG_CURRENT_DESKTOP:-} == *GNOME* ]] || command -v gnome-shell >/dev/null; then
  rm -rf "$DATA/gnome-shell/extensions/$EXT"
  mkdir -p "$DATA/gnome-shell/extensions"
  cp -r "$ROOT/integrations/gnome/$EXT" "$DATA/gnome-shell/extensions/"
  echo "GNOME extension installed to $DATA/gnome-shell/extensions/$EXT"
  if [[ ${XDG_CURRENT_DESKTOP:-} == *GNOME* ]]; then
    gnome-extensions enable "$EXT" 2>/dev/null \
      && echo "Enabled. On Wayland, log out and back in once so GNOME Shell loads it." \
      || echo "Log out and back in, then run: gnome-extensions enable $EXT"
  fi
fi
echo "Installed $PREFIX/bin/sidestage. Start it from your app launcher or run: sidestage"
