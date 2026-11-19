qdbus6 org.kde.KWin /VirtualDesktopManager org.kde.KWin.VirtualDesktopManager.createDesktop 1 "Two" >/dev/null
sleep 2.5
cat > "$OUT/d.js" <<'JS'
workspace.currentDesktop = workspace.desktops[1];
for (const w of workspace.windowList()) if (w.resourceClass == "sidestage" && w.caption == "Sidestage")
  print("SIDESTAGE sidebar onAll=" + w.onAllDesktops + " current=" + workspace.currentDesktop.name + " desktops=" + workspace.desktops.length);
JS
id=$(qdbus6 org.kde.KWin /Scripting org.kde.kwin.Scripting.loadScript "$OUT/d.js" sst-d)
qdbus6 org.kde.KWin /Scripting/Script$id org.kde.kwin.Script.run >/dev/null; sleep 1.5
shot 2
