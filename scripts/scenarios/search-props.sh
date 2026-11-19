props() { for w in $(xprop -root _NET_CLIENT_LIST | sed 's/.*# //; s/,//g'); do xprop -id $w WM_NAME | grep -q '"Sidestage"' && { echo "== $1"; xprop -id $w WM_PROTOCOLS WM_HINTS _NET_WM_USER_TIME; }; done >> "$OUT/props.txt"; }
props idle
"$XINPUT" keys ctrl+alt+space; sleep 1.2
props after-shortcut
