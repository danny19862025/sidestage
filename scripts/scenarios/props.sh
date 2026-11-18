for w in $(xprop -root _NET_CLIENT_LIST | sed 's/.*# //; s/,//g'); do
  echo "== $w"; xprop -id $w WM_CLASS _NET_WM_WINDOW_TYPE _NET_WM_STRUT_PARTIAL _NET_WM_STATE WM_NORMAL_HINTS WM_HINTS
done > "$OUT/props.txt" 2>&1
xprop -root _NET_WORKAREA >> "$OUT/props.txt"
