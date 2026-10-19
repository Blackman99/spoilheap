# Recorded on the demo home from scripts/demohome.sh (big = sparse files),
# with 3.1 GB free simulated. Re-record: scripts/demohome.sh /tmp/sh-big big && vhs docs/demo.tape
Output docs/demo.gif
Set Shell "bash"
Set FontFamily "DejaVu Sans Mono"
Set FontSize 15
Set Width 1300
Set Height 860
Set Padding 22
Set TypingSpeed 45ms
Set Theme { "name": "spoil", "black": "#151413", "red": "#e5533f", "green": "#a6d13c", "yellow": "#f0b44a", "blue": "#5b8fd6", "magenta": "#c78fb5", "cyan": "#6f9fe0", "white": "#f1efe9", "brightBlack": "#8c8679", "brightRed": "#f07a1a", "brightGreen": "#bfe06a", "brightYellow": "#cfa962", "brightBlue": "#7aa7e6", "brightMagenta": "#d9a6c8", "brightCyan": "#8fb6ee", "brightWhite": "#ffffff", "background": "#1f1b18", "foreground": "#f1efe9", "selection": "#3a2b22", "cursor": "#cfa962" }
Hide
Type "export PATH=/tmp/bin:$PATH SPOILHEAP_HOME=/tmp/sh-big SPOILHEAP_OS=macos SPOILHEAP_APPARENT_SIZE=1 SPOILHEAP_FAKE_FREE=3.1GB/494GB FORCE_COLOR=1 PS1='$ ' && cd /tmp/sh-big/code/payments && clear"
Enter
Sleep 400ms
Show
Type "spoilheap | less -R"
Sleep 300ms
Enter
Sleep 5s
Type " "
Sleep 4s
Type "q"
Sleep 300ms
Type "spoilheap clean --safe"
Sleep 300ms
Enter
Sleep 5s
Type "clear"
Enter
Type "echo '{}' | spoilheap guard --floor 10GB; echo exit=$?"
Sleep 300ms
Enter
Sleep 4.5s
