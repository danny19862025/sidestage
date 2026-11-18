# GNOME has no fake-input protocol for clients; move the pointer from the
# extension test hook instead.
call DebugMovePointer 400 450; sleep 0.3
for x in 300 200 100 50 10 2; do call DebugMovePointer $x 450; sleep 0.05; done; sleep 1; shot 2
for x in 30 60 90 120; do call DebugMovePointer $x 450; sleep 0.05; done; sleep 0.5
for x in 160 200 240 280 400 700 900; do call DebugMovePointer $x 450; sleep 0.05; done; sleep 1.5; shot 3
