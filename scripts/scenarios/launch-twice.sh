# Trigger "Launch section" twice in quick succession: only one batch may run.
"$XINPUT" click 90 72 right; sleep 0.25
"$XINPUT" click 150 92; sleep 0.25
"$XINPUT" click 90 72 right; sleep 0.25
"$XINPUT" click 150 92; sleep 0.25
shot 2
sleep 3
