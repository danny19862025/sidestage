# Sidestage starts while the extension is off; enabling it later must
# activate the shortcut without restarting Sidestage.
gnome-extensions disable sidestage@sidestage.app; sleep 1.5
kill $APP 2>/dev/null; wait $APP 2>/dev/null
"$BIN" > "$OUT/sidestage2.log" 2>&1 &
APP=$!
sleep 4
gnome-extensions enable sidestage@sidestage.app; sleep 6
call DebugKey "Control_L+Alt_L+space" >/dev/null; sleep 1.5
