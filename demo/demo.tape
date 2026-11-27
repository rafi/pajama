# pajama demo - pick a project, get its layout back, and jump back.
#
# Recorded against the sandbox that demo/fixture.sh builds under $DEMO_SANDBOX. Run it
# through `just demo`, which builds pj, seeds the sandbox and removes it afterwards.

Output demo/demo.gif

Set Shell bash

Require tmux
Require fzf
Require pj

Set Width 1000
Set Height 600
Set Framerate 30
Set TypingSpeed 80ms

Set Theme "Catppuccin Mocha"
Set BorderRadius 14
Set WindowBar Colorful
Set WindowBarSize 40
Set Margin 20
Set Padding 20
Set MarginFill "#674EFF"

Set FontFamily "PragmataPro"
Set FontSize 19
Set LineHeight 1.1

Hide
Type@0ms `[ -n "$DEMO_SANDBOX" ] || { echo "DEMO_SANDBOX unset, run: just demo"; exit 1; }`
Enter
Type@0ms `source "$DEMO_SANDBOX/env.sh" && exec tmux attach -t =web`
Enter
Wait+Screen /❯/
Ctrl+L
Sleep 500ms
Show

Sleep 1s
Type "pj"
Sleep 500ms
Enter
Wait+Screen /Name/
Sleep 2s
Screenshot demo/picker.png

Down
Sleep 1s
Down
Sleep 1s
Down
Sleep 1s
Type "api"
Sleep 1s
Enter
Wait+Screen@10s /listening on/
Sleep 3s

Type "pj -"
Sleep 500ms
Enter
Wait+Screen /❯/
Sleep 3s
