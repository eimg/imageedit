-- Configure only the mounted package's window; leave other Finder windows alone.
on run arguments
    set mountPath to item 1 of arguments
    set packageFolder to POSIX file mountPath as alias
    tell application "Finder"
        open packageFolder
        set packageWindow to container window of packageFolder
        set current view of packageWindow to icon view
        set toolbar visible of packageWindow to false
        set statusbar visible of packageWindow to false
        set pathbar visible of packageWindow to false
        set bounds of packageWindow to {200, 160, 840, 588}
        set options to icon view options of packageWindow
        set arrangement of options to not arranged
        set icon size of options to 112
        set text size of options to 14
        set label position of options to bottom
        set background picture of options to file ".background:background.tiff" of packageFolder
        set position of item "Lime.app" of packageFolder to {170, 210}
        set extension hidden of item "Lime.app" of packageFolder to true
        set position of item "Applications" of packageFolder to {470, 210}
        update packageFolder without registering applications
        close packageWindow
        -- Finder writes its .DS_Store asynchronously after the window closes.
        delay 2
    end tell
end run
