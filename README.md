# reframe-native

Throwaway prototype: [reframe](https://github.com/TheyCallMeJuzou/reframe) as a native
application without a webview (Rust, GPUI, `alacritty_terminal`).

It exists to answer one question before a rewrite is decided: does a GPUI terminal running real
`claude` sessions behave at least as well as the Electron version, and what does it save?
Results go into `FINDINGS.md`.

Nothing here is meant to ship.

## Build and run

```
cargo build --release
targeteleaseeframe-native.exe --tile <folder> --tile <folder>
```

Needs the stable MSVC Rust toolchain and the Visual Studio C++ build tools. `claude.exe` is taken
from the PATH, or from `--claude <path>`.

A tile under `\wsl.localhost\<distro>\...` runs claude inside that distro. It needs two more
values, which the Electron version probes by itself: `--wsl-claude <path of claude in the distro>`
and `--wsl-path <the login shell's PATH there>`.

Keys the prototype adds: Ctrl+Shift+O moves the focused tile into a window of its own,
Ctrl+Shift+D writes its screen to a text file in the temp folder. Copy, paste and right-click
follow the Electron version.

## Checks

- `--dump`: one session without a window; prints its screen and whether Claude Code registered it.
- `scripts\selftest.ps1 <folder>`: two real sessions; types, selects with the mouse and tears a
  tile out through the window's own input path, captures the windows, ends the sessions.
- `scripts\measure.ps1 -RootPid <pid> -Label <name>`: memory and idle CPU of a running app.
  `scriptsun-electron.mjs` starts the Electron version with an empty profile for the same
  measurement; `--hold <seconds>` keeps the prototype idle and then ends it.

No check sends a prompt to Claude.
