qdbus6 org.kde.KWin /VirtualDesktopManager org.kde.KWin.VirtualDesktopManager.createDesktop 1 "Two" >/dev/null
sleep 2
cat > "$OUT/e.js" <<'JS'
for (const w of workspace.windowList()) if (w.resourceClass == "sidestage" && w.caption == "Sidestage") {
  print("SIDESTAGE before onAll=" + w.onAllDesktops + " desktops=" + w.desktops.length + " special=" + w.specialWindow + " dock=" + w.dock);
  w.onAllDesktops = true;
  print("SIDESTAGE after-script onAll=" + w.onAllDesktops);
}
JS
id=$(qdbus6 org.kde.KWin /Scripting org.kde.kwin.Scripting.loadScript "$OUT/e.js" sst-e)
qdbus6 org.kde.KWin /Scripting/Script$id org.kde.kwin.Script.run >/dev/null; sleep 1
