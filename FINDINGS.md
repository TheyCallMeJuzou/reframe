# Findings

State on 2026-11-11, after the automated checks. The hands-on checklist at the end is still open,
so the recommendation is provisional.

## Result so far

A native reframe is technically feasible with this stack. Two real `claude` sessions (Windows and
WSL) run, draw correctly and take input in a GPUI window with no webview, and a tile moves to
another window without touching its session. Nothing found so far questions the stack.

**Provisional recommendation: go on to the hands-on checklist.** The rewrite decision depends on
how it feels at the keyboard, which no script here can judge.

## Stack that worked

| Part | Choice | Note |
|---|---|---|
| UI | `gpui-kit` 0.9.1 (GPUI snapshot `gpui-pre` 0.3.12) | First option tried; built and ran on Windows at once. Brings resizable panels. |
| Terminal state | `alacritty_terminal` 0.26.0 | Also provides the ConPTY backend and the pty thread. |
| ConPTY | `conpty.dll` + `OpenConsole.exe` 1.23 next to the exe | Confirmed loaded instead of the one built into Windows. |
| Toolchain | Rust 1.99.1 (MSVC) | First full build about 1.5 min (debug), 6 min (release). |

The terminal view is about 850 lines, the whole prototype about 1,700. No code was taken from
Zed's GPL terminal crates.

## Measurements

Same machine, two idle real `claude` sessions in the same folder, measured with
`scripts/measure.ps1`. "Own" means the app's processes only; `claude`, its helpers and the
console hosts are the same on both sides and are left out.

| | reframe 0.10.0 (Electron) | reframe-native (release) |
|---|---|---|
| Own processes | 5 | 1 |
| Own private working set | 161 MB | 25 MB |
| Own committed memory | 310 MB | 116 MB |
| Idle CPU (10 s) | 1.9 % of one core | 2.2 % of one core |
| Size on disk | 340 MB installed | 20 MB (exe + ConPTY files) |
| Time to first window | 4.4 s (see note) | 0.3 s (283-316 ms, 3 runs) |

- The start-up times are **not measured the same way**. The Electron figure is from Playwright,
  which adds its own attach time; the native figure polls for the window handle. A like-for-like
  run of the Electron app was not done.
- Idle CPU is not better. The cause was not looked into; a likely one is that every terminal
  wakeup redraws the whole tile.
- One run each, except start-up of the prototype.

## Automated checks that passed

- `--dump`: a session starts, the screen parses, Claude Code's registry entry exists for the
  pty's pid and is removed after two Ctrl+C.
- WSL: `wsl.exe -d Ubuntu --cd ... --exec env PATH=... claude` starts and draws (trust dialog of
  the home folder).
- Self-test with two sessions in the fullscreen TUI (alternate screen, mouse reporting, focus
  reporting on): typed letters arrive through the text-input path, Backspace through the key
  path, a Shift-drag selects text, Ctrl+C twice ends each session.
- Tear-out: the tile moves to a second window, 99x61 becomes 200x61 cells, same pid, session
  alive, the typed text still in the prompt. The window left behind focuses a tile again.
- Window captures: colours, bold, italic, dim, the frame and the block-art logo are drawn
  correctly and join up cell to cell.

Input in these checks is dispatched into the window by GPUI, not typed on a physical keyboard.
Layout-dependent input (AltGr, dead keys) is therefore untested.

## What changed compared with the plan

- **Restore snapshot is not needed.** The terminal state lives once in the process and any
  window draws it, so the headless mirror and its serialization have no counterpart. The price:
  sessions and UI share one process. The Electron version survives a renderer crash because the
  ptys live in a separate host process; a native version has to decide whether to keep that split.
- Gitea's API token cannot create repositories; the repo was created by hand.

## Things noticed

- `NO_COLOR` in the environment makes claude draw without colours. An automation shell set it;
  it is not a fault of the prototype.
- claude inside WSL uses 256-colour codes, not 24-bit: `COLORTERM` does not cross into WSL.
- Symbols missing from Cascadia Mono came from the colour emoji font until a fallback to
  Segoe UI Symbol was added.
- GPUI is pre-1.0 and its packaging is confusing: the crates.io `gpui` is a year old, current
  snapshots come through `gpui-kit`. A rewrite ties itself to that project's releases.

## Image paste (found during the hands-on test)

Pasting an image "takes forever" in the Electron version and did so in the prototype at first.
Both sent Claude Code's image-paste key (Alt+V) and let it read the clipboard itself. Reading a
189x311 picture from the clipboard through PowerShell took 10.6 s on this machine; that is taken
to be the route Claude Code uses on Windows, which was not confirmed in its code.

The prototype now reads the image itself (under 1 ms), writes it to
`%TEMP%eframe-native-paste\` as PNG and pastes the path, which Claude Code turns into
`[Image #1]` (checked with a real session, nothing submitted). The Electron version already does
this for WSL sessions (`src/renderer/paste.ts`), so the same change there is small. The saved
files are not cleaned up yet.

## Not covered by the prototype

Status dots and the hooks server, session list, takeover of external terminals, tray, settings,
groups, updater, installer, find bar, link detection, pasting copied files into WSL as `/mnt`
paths, accessibility. None of these was judged risky, but they are most of the remaining work.

## Hands-on checklist (open)

Run next to reframe 0.10.0 and compare:

```
target\release\reframe-native.exe --tile <folder> --tile \\wsl.localhost\Ubuntu\home\<user> ^
  --wsl-claude <claude in the distro> --wsl-path <login PATH>
```

- [ ] Fullscreen TUI renders correctly, scrolls with the wheel, clicks work.
- [ ] Right-click pastes exactly once with mouse mode on; Shift+right-click pastes once;
      right-click copies a selection with mouse mode off.
- [ ] German layout: `@ { } [ ] \ | ~ €` with AltGr, dead keys, umlauts, emoji from Win+.
- [ ] Ctrl+C with and without a selection, Ctrl+V text, multi-line paste, image paste (now fast?),
      Shift+Enter, Ctrl+Shift+- (undo), Esc, arrow keys in menus, Tab completion.
- [ ] Dragging the splitter fast while claude is streaming: no garbage, no lost lines, no crash.
- [ ] A long streaming answer: smooth, spinner and box characters aligned.
- [ ] The WSL tile behaves the same; Ctrl+Shift+O keeps screen, scrollback and session.
- [ ] Closing two sessions at once several times: no crash.

Decision rule: go only if every item is at least as good as reframe 0.10.0 or has a known,
bounded fix, and the measured gain is worth a full rewrite.
