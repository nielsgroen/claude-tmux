# Claude Code Manager

## Introduction
This is a TUI program that manages claude code for you using tmux.
It is able to run in a tmux display-popup, and can execute tmux commands to manage tmux sessions running a Claude Code pane.


## Installation

To use the executable, add the following line to your tmux configuration file:
```
bind-key C-c display-popup -E -w 80 -h 20 "~/repositories/ccm/target/release/ccm"
```

Now, you can use `ctrl-B, ctrl-C` to open the Claude Code Manager.
