click 122 205      # "Text Editor" tile in Coding
sleep 5
shot 2
cat > "$OUT/list.js" <<'JS'
for (const w of workspace.windowList()) print("SIDESTAGE window " + w.resourceClass + " | " + w.caption);
JS
id=$(qdbus6 org.kde.KWin /Scripting org.kde.kwin.Scripting.loadScript "$OUT/list.js" sidestage-list)
qdbus6 org.kde.KWin /Scripting/Script$id org.kde.kwin.Script.run >/dev/null; sleep 0.5
