move 700 450
python3 "$ROOT/scripts/test-window.py" "Fullscreen game" 0 14 full:1:8 >/dev/null 2>&1 &
sleep 3
echo "SIDESTAGE-MARK edge-during-fullscreen" >> "$OUT/marks"
glide 400 450 2 450; sleep 1.2; shot 2
glide 2 450 600 450; sleep 6
echo "SIDESTAGE-MARK edge-after-fullscreen" >> "$OUT/marks"
glide 400 450 2 450; sleep 1.2; shot 3
