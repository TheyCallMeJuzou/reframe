# reframe-native

[reframe](https://github.com/TheyCallMeJuzou/reframe) as a native application without a webview
(Rust, GPUI, `alacritty_terminal`). It is being built up to do everything reframe 0.10 does and
will then replace the Electron version as reframe 1.0.

It started as a prototype; what that showed and measured is in `FINDINGS.md` (the commands there
are the prototype's, as of commit `07c5f1a`).

## Layout

- `crates/reframe-core`: everything without a window (state and its file, layout, groups,
  workspaces, texts, finding and starting claude, hooks, session discovery). Ported module by
  module from the Electron version, with its tests; each file names where it comes from.
- `crates/reframe`: the app (`reframe-native.exe`): sessions on pseudo consoles, the terminal
  view, windows.
- `test/`: the stand-ins for claude and WSL the end-to-end tests run, taken from the Electron
  version unchanged.

## Build and run

```
cargo build --release
target\release\reframe-native.exe [--new <folder>]...
```

Needs the stable MSVC Rust toolchain and the Visual Studio C++ build tools.

Until it takes over from the Electron version it keeps a profile of its own
(`%APPDATA%\reframe-native`, or `REFRAME_USER_DATA`), so the two never write the same state file.
Sessions that were open are resumed on the next start. `--new <folder>` starts a new session
there right away, without the dialog.

## Where it stands

Done: sessions (start, resume, restart, end), the window with its split layout, separators that
snap, zoom, collapsed tiles, the toolbar, the New session, Settings and About dialogs, all
shortcuts except those named below, text size, the find bar, renaming a session in its tile, web
links, the window's place and size; tiles and groups dragged by their bar (within a window, into
another one, or out into a window of their own), groups with their menus and colours, files
dropped onto a tile, more than one window (Ctrl+Shift+N), and what closing a window does.

Around the window: the tray icon with its menu (windows shown and hidden, new session, new
window, about, quit), windows that stay in the tray when they are closed, one reframe per profile
(a second start shows the windows of the first), "Start with the system" in Settings,
notifications with a click that brings the session up, the taskbar button that flashes and
carries a badge, Ctrl+Shift+Space for the next session that needs you, and sessions that stay
open when Windows goes down.

The session list (Ctrl+Shift+L): reframe's sessions, claudes running in other terminals and past
sessions from Claude Code's transcripts, with search, "Show more", selecting several
(Shift/Ctrl+click) to open them together or as a group, rows dragged into the layout, renaming
and removing; it follows Claude Code's folder as it changes. A conversation that runs in another
terminal is taken over after asking (Ctrl+C twice there, then resumed here), also one of
reframe's own that someone resumed elsewhere. Workspaces: a window saved under a name, updated,
opened again as a window of its own, renamed and deleted, from the toolbar, the list and the
tray.

Sessions in WSL folders run their claude in that distribution (asked through the user's login
shell, its status from Claude Code's registry there, dropped files as Linux paths); the list shows
the sessions of running distributions, says which known one is not running and starts it, and
takes over a session running in a WSL terminal.

A terminal window with claude in it (also in WSL) dragged onto reframe, or a Windows Terminal tab
torn out over it, is offered for taking over: its sessions continue in reframe where it was let go,
and a tab with only a shell left is closed by typing "exit" there.

Still to come: installer and updater. Not there yet: a Windows Terminal tab dragged over reframe
shows no drop place before it is let go (GPUI's drop target takes no drags without data), and a
file that exists only in memory (a picture dragged out of a browser) is not taken by a tile.

Ctrl+Shift+D writes the focused terminal's screen to a text file in the temp folder.

## Checks

`scripts\check.ps1` runs what has to pass before a push: formatting, clippy, the unit tests and
the end-to-end tests. The end-to-end tests (`crates/reframe/tests`) start the app with a
throwaway profile and the fake claude and drive it over a local socket that only exists with
`REFRAME_E2E`; they need `node` on the PATH.

`scripts\measure.ps1 -RootPid <pid> -Label <name>` measures memory and idle CPU of a running app;
`scripts\run-electron.mjs` starts the Electron version with an empty profile for the same
measurement. `scripts\capture.ps1 <folder>` saves a picture of each window.

No check sends a prompt to Claude.
