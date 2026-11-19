# Filmstrip of the reveal animation (screenshots are ~60-120 ms apart).
"$XINPUT" glide 300 450 2 450
for i in 1 2 3 4 5 6 7 8; do python3 "$ROOT/scripts/screenshot.py" "$OUT/film-$i.png" >/dev/null; done
