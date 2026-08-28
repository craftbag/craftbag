# README GIF: rank a project catalog, then load the matching skill.
# Placeholders __REPO__, __DEMO__, and __BIN_DIR__ are filled by demo/setup.sh.
Output "__REPO__/demo/demo.gif"

Require craftbag

Set Shell bash
Set Width 1100
Set Height 580
Set FontSize 15
Set Padding 16
Set Theme "Catppuccin Mocha"
Set Framerate 25
Set PlaybackSpeed 0.75
Set TypingSpeed 90ms
Set CursorBlink false

Hide
Set TypingSpeed 1ms
Type "export HOME=/tmp/cb-demo-home PATH=__BIN_DIR__:$PATH"
Enter
Type "cd __DEMO__"
Enter
Ctrl+L
Type "craftbag list --catalog --context review"
Enter
Show

Sleep 5s
Ctrl+L
Sleep 400ms
Set TypingSpeed 90ms
Type "craftbag load review-pr"
Enter
Sleep 8s
