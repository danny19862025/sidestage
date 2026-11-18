cat > "$OUT/area.js" <<'JS'
const out = workspace.activeScreen;
const a = workspace.clientArea(KWin.MaximizeArea, out, workspace.currentDesktop);
print("SIDESTAGE maximize area " + a.x + "," + a.y + " " + a.width + "x" + a.height);
for (const w of workspace.windowList()) {
  if (w.resourceClass == "sidestage") print("SIDESTAGE dock=" + w.dock + " strut? geom=" + w.frameGeometry.x + "," + w.frameGeometry.width + "x" + w.frameGeometry.height + " x11=" + (w.windowId !== undefined));
}
JS
echo "bus=$DBUS_SESSION_BUS_ADDRESS" > "$OUT/qdbus.txt"; id=$(qdbus6 org.kde.KWin /Scripting org.kde.kwin.Scripting.loadScript "$OUT/area.js" sidestage-area)
echo "id=$id" >> "$OUT/qdbus.txt"; qdbus6 org.kde.KWin /Scripting/Script$id org.kde.kwin.Script.run >> "$OUT/qdbus.txt" 2>&1
sleep 1
