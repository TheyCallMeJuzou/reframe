# reframe

A desktop app that keeps your [Claude Code](https://claude.com/claude-code) sessions side by side in one window, and brings all of them back after a restart.

- Sessions run as tiles that are all visible at once; drag a tile's header onto another tile's edge to split next to it.
- The arrow in a tile's corner collapses it into the side bar; a click puts it back where it was.
- Every change is saved right away. Quitting from the tray ends the sessions cleanly; the next start resumes each one (`claude --resume`) in its folder.
- Closing the window only hides it: sessions keep working and you get a notification when Claude is done or needs you.
- English and German UI, following the system language.

> **Status:** early development, Windows first. Multiple windows, groups, a session list and dragging existing terminal windows in are planned; see [docs/PLAN.md](docs/PLAN.md) (German).

## Requirements

- [Claude Code](https://claude.com/claude-code) installed and logged in (`claude` on your `PATH`)
- Node.js 22 or newer for development

## Development

```sh
npm install        # node-pty's install script is allowed in package.json (allowScripts)
npm run dev        # start with hot reload
npm test           # unit tests
npm run test:e2e   # builds, then runs end-to-end tests against a fake claude
npm run typecheck && npm run lint
```

A smoke test against the real CLI is opt-in (it starts real sessions but sends no prompts):

```sh
REFRAME_E2E_REAL=1 REFRAME_E2E_FOLDER=/path/to/a/trusted/folder npx playwright test real
```

### Environment variables

| Variable                        | Purpose                                                                      |
| ------------------------------- | ---------------------------------------------------------------------------- |
| `REFRAME_USER_DATA`             | Use another profile directory (state, hook settings)                         |
| `REFRAME_CLAUDE_BIN`            | Path to the `claude` executable, or a `.js` stand-in                         |
| `REFRAME_NODE`                  | Node binary that runs a `.js` stand-in (needed on Windows)                   |
| `REFRAME_CLAUDE_HOME`           | Claude Code's config directory (default: `CLAUDE_CONFIG_DIR` or `~/.claude`) |
| `REFRAME_LOCALE`                | Force `en` or `de`                                                           |
| `REFRAME_DISABLE_NOTIFICATIONS` | Suppress OS notifications (tests)                                            |

## How it works

- **Main process** owns the state (`state.json` in the profile, written atomically with a backup) and applies layout changes the windows ask for.
- A **PTY host** (`utilityProcess`) runs each `claude` in a pseudo terminal ([node-pty](https://github.com/microsoft/node-pty)) and mirrors its screen in a headless [xterm.js](https://xtermjs.org/), so a window can attach at any time and get the current screen.
- **Status** comes from Claude Code's HTTP hooks (passed per session with `--settings`, merged with your own hooks; your settings files are never changed) and from Claude Code's session registry.

Design notes and findings: [docs/PLAN.md](docs/PLAN.md), [docs/PHASE0-FINDINGS.md](docs/PHASE0-FINDINGS.md).

## License

MIT
