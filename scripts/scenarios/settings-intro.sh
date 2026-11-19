"$XINPUT" click 215 33
for i in 1 2 3 4 5 6; do python3 "$ROOT/scripts/screenshot.py" "$OUT/film-$i.png" >/dev/null; done
sleep 1.5; shot 2
click 508 290; sleep 1; shot 3      # Appearance
click 508 405; sleep 1.5; shot 4    # About
