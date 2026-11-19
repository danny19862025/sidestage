for w in $(xprop -root _NET_CLIENT_LIST | sed 's/.*# //; s/,//g'); do
  xprop -id $w WM_CLASS | grep -q sidestage && xprop -id $w WM_NAME XdndAware >> "$OUT/xdnd.txt"
done
