sleep 1; shot 2
for i in 1 2 3 4 5; do python3 "$ROOT/scripts/screenshot.py" "$OUT/film-$i.png" >/dev/null; done
