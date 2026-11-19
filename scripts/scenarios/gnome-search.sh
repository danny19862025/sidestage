focus() { call ListWindows | python3 -c "
import sys,json,ast
s=ast.literal_eval(sys.stdin.read().strip())[0]
print('$1', [ (w['title'], 'min' if w['minimized'] else '', 'FOCUSED' if w['focused'] else '') for w in json.loads(s)])" >> "$OUT/focus.txt"; }
typ() { for c in $(echo "$1" | sed 's/./& /g'); do call DebugKey "$c" >/dev/null; sleep 0.08; done; }
focus before
call DebugKey "Control_L+Alt_L+space" >/dev/null; sleep 1.2
typ notes; sleep 0.8; shot 2
call DebugKey Return >/dev/null; sleep 1.5
focus after-enter
call DebugKey "Control_L+Alt_L+space" >/dev/null; sleep 1.2
typ quar; sleep 0.6
focus during-search
call DebugKey Escape >/dev/null; sleep 1.2
focus after-esc
