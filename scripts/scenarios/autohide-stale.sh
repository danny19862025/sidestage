mark() { echo "MARK $1" >> "$OUT/sidestage.log"; }
# A: open a section menu, then leave.
mark A-menu
glide 400 450 2 450; sleep 1.0
click 90 72 right; sleep 0.6
glide 120 72 900 450; sleep 3.0
mark A-end
# B: open Settings, then work in it.
glide 400 450 2 450; sleep 1.0
click 190 33; sleep 2
glide 120 33 900 450; sleep 2.5
mark B-end
shot 2
"$XINPUT" keys alt+f4; sleep 1
# C: search, then click into another app.
glide 400 450 2 450; sleep 1.0
click 128 33; sleep 0.6
"$XINPUT" type des; sleep 0.5
click 900 450; sleep 3.0
mark C-end
