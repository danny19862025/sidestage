for w in $(xprop -root _NET_CLIENT_LIST | sed 's/.*# //; s/,//g'); do
  xprop -id $w WM_CLASS | grep -q sidestage && SID=$w
done
python3 - "$SID" > "$OUT/shape.txt" 2>&1 <<'PY'
import sys
print("window", sys.argv[1])
PY
move 300 450; sleep 0.5
for x in 1 3 5 10 30; do move $x 450; sleep 0.6; done
shot 2
