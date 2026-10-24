<p align="center">
  <img src="docs/logo/reframe-icon.svg" width="96" height="96" alt="reframe logo">
</p>

<h1 align="center">reframe</h1>

<p align="center">
  Keep your <a href="https://claude.com/claude-code">Claude Code</a> sessions side by side in one window,<br>
  group them, tear them out, pull terminal windows in, and get every session back after a restart.
</p>

---

## What it does

- **All sessions in view.** Every session is a tile, and all tiles are visible at once, in two columns that fill in turn. Drag a tile's header onto another tile's edge to split next to it; the drop zone shows where it lands.
- **Out of the way when you want.** The arrow in a tile's corner collapses it into the side bar; a click puts it back where it was.
- **Groups.** Put tiles into a group (right-click a tile, or drag it onto a group): a colored frame with a name. A group collapses, moves and leaves the window as one; dissolve it or end all its sessions from its menu.
- **Several windows.** Open a new, empty window (toolbar, tray or Ctrl+Shift+N), or drag a tile or a group out of the window and it gets a window of its own; drag it back, or into another window, whenever you like.
- **Session list.** The side bar on the left lists the sessions in reframe, the ones running in other terminals and your recent Claude Code sessions, with search. Click or drag one to open it: ended sessions resume, and sessions running elsewhere are taken over (ended there, continued here, history and name included). Select several with Shift or Ctrl+click and open them together from the right-click menu, also as a new group, here or in a new window. Sessions that turn up while reframe runs light up, and the Sessions button gets a dot while the list is hidden.
- **Pull terminal windows in** (Windows). Drag a Windows Terminal or console window (or just a tab of it) with a running `claude` onto reframe: after a short confirmation the session continues in reframe, at the spot you dropped it, and the terminal tab closes if nothing else runs in it.
- **Files and screenshots.** Drop files onto a session to type in their paths (Claude Code attaches images among them); Ctrl+V pastes text, files you copied in Explorer or Finder, or a screenshot from the clipboard.
- **Nothing gets lost.** Every change is saved right away. Quitting from the tray ends the sessions cleanly, and the next start resumes each one (`claude --resume`) in its folder, in its place.
- **Runs in the background.** Closing a window only hides it: sessions keep working, and you get a notification when Claude is done or needs you, unless you are looking at it anyway.
- **Starts with the system** if you like (tray menu), quietly in the tray, and **updates itself** from GitHub Releases.
- English and German, following the system language.

## Keyboard shortcuts

On macOS, use Cmd instead of Ctrl. Everything else goes to Claude Code as usual.

| Shortcut              | What it does                                                                                |
| --------------------- | ------------------------------------------------------------------------------------------- |
| Ctrl+1 … 9            | Focus the first … ninth tile in the window                                                  |
| Ctrl+Shift+Arrow keys | Focus the neighboring tile in that direction                                                |
| Ctrl+Shift+M          | Collapse the focused tile; without a focused tile, bring back the last one                  |
| Ctrl+Shift+L          | Show or hide the session list (and search it)                                               |
| Ctrl+Shift+T          | New session                                                                                 |
| Ctrl+Shift+N          | New window                                                                                  |
| Ctrl+C / Ctrl+V       | Copy the selection (otherwise Ctrl+C goes to Claude) / paste text, copied files or an image |

## Install

Download the installer for your system from [Releases](https://github.com/TheyCallMeJuzou/reframe/releases). You need [Claude Code](https://claude.com/claude-code) installed and logged in (`claude` on your `PATH`).

The builds are not code-signed yet, so the first start needs a confirmation:

- **Windows** (`reframe-Setup-<version>.exe`): installs for your user without admin rights. If SmartScreen warns, choose _More info_ → _Run anyway_.
- **macOS** (`.dmg`, Apple silicon): drag reframe into Applications, then right-click it → _Open_ the first time (or `xattr -dr com.apple.quarantine /Applications/reframe.app`). Updates on macOS need a signed build, so for now update by downloading the new version.
- **Linux**: the `.AppImage` (make it executable, then run it; it updates itself) or the `.deb`.

| Platform | Status                                                                                                                             |
| -------- | ---------------------------------------------------------------------------------------------------------------------------------- |
| Windows  | Everything above; this is where reframe is developed and tested.                                                                   |
| macOS    | Built in CI, not tested yet. Pulling terminal windows in is not available yet (use the session list to take sessions over).        |
| Linux    | Built in CI, not tested yet. Pulling terminal windows in is not available yet (planned for X11; on Wayland only the session list). |

## Development

Requirements: Node.js 24, and on Linux the tools to build [node-pty](https://github.com/microsoft/node-pty) (`python3`, `make`, a C++ compiler).

```sh
npm install        # install scripts are only allowed for node-pty and koffi (allowScripts in package.json)
npm run dev        # start with hot reload
npm test           # unit tests
npm run test:e2e   # builds, then runs the end-to-end tests against a fake claude
npm run typecheck && npm run lint
npm run dist       # installers for this system, into dist/ (npm run dist:dir: just the unpacked app)
```

A smoke test against the real CLI is opt-in (it starts real sessions; the hooks test sends one short prompt):

```sh
REFRAME_E2E_REAL=1 REFRAME_E2E_FOLDER=/path/to/a/trusted/folder npx playwright test real
```

See [CONTRIBUTING.md](CONTRIBUTING.md) for how to contribute and [docs/RELEASING.md](docs/RELEASING.md) for releases.

### Environment variables

| Variable                        | Purpose                                                                      |
| ------------------------------- | ---------------------------------------------------------------------------- |
| `REFRAME_USER_DATA`             | Use another profile directory (state, hook settings, log)                    |
| `REFRAME_CLAUDE_BIN`            | Path to the `claude` executable, or a `.js` stand-in                         |
| `REFRAME_NODE`                  | Node binary that runs a `.js` stand-in (needed on Windows)                   |
| `REFRAME_CLAUDE_HOME`           | Claude Code's config directory (default: `CLAUDE_CONFIG_DIR` or `~/.claude`) |
| `REFRAME_LOCALE`                | Force `en` or `de`                                                           |
| `REFRAME_DISABLE_NOTIFICATIONS` | Suppress OS notifications (tests)                                            |
| `REFRAME_E2E`                   | Exposes test hooks to the end-to-end tests (set by the tests)                |
| `REFRAME_E2E_APP`               | Run the end-to-end tests against a packaged app (e.g. `dist/win-unpacked/…`) |

The app log is in the profile directory, under `logs/main.log`.

## How it works

- The **main process** owns the state (`state.json` in the profile, written atomically with a backup) and applies the layout changes the windows ask for.
- A **PTY host** (`utilityProcess`) runs each `claude` in a pseudo terminal ([node-pty](https://github.com/microsoft/node-pty)) and mirrors its screen in a headless [xterm.js](https://xtermjs.org/), so any window can attach at any time and get the current screen. That is how tiles move between windows without restarting anything.
- **Status** comes from Claude Code's HTTP hooks (passed per session with `--settings`, merged with your own hooks; your settings files are never changed) and from Claude Code's session registry.
- **Taking over** a session that runs elsewhere ends it there the way you would (Ctrl+C), then resumes it in reframe under the same session id. On Windows, reframe follows window drags and new terminal windows (window events through [koffi](https://koffi.dev/)) and maps consoles to their terminal windows.

Design notes and findings (German): [docs/PLAN.md](docs/PLAN.md), [docs/PHASE0-FINDINGS.md](docs/PHASE0-FINDINGS.md).

## License

[MIT](LICENSE) © reframe Team
