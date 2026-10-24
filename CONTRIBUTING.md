# Contributing to reframe

Thanks for helping! Bug reports, ideas and pull requests are all welcome. For anything bigger than a small fix, please open an issue first so we can agree on the approach before you put time into it.

## Reporting a bug

Please include your OS and version, your reframe and Claude Code versions (`claude --version`), what you did, what you expected and what happened. The app log helps a lot: it is `logs/main.log` in reframe's profile directory (`%APPDATA%\reframe` on Windows, `~/Library/Application Support/reframe` on macOS, `~/.config/reframe` on Linux). It contains folder paths and session ids, but no conversation content; look through it before you attach it.

## Setting up

You need Node.js 24 (and on Linux `python3`, `make` and a C++ compiler for node-pty). Then:

```sh
npm install
npm run dev
```

`npm run dev` uses your normal profile and your real `claude`. To keep experiments apart, start it with another profile: `REFRAME_USER_DATA=/tmp/reframe-dev npm run dev`.

## Before you open a pull request

```sh
npm run typecheck
npm run lint
npx prettier --check .   # npm run format fixes it
npm test                 # unit tests (Vitest)
npm run test:e2e         # end-to-end tests (Playwright, against a fake claude in test/fake-claude)
```

CI runs the same on Windows, macOS and Linux, plus the end-to-end tests against the packaged app.

- **Tests**: new behavior comes with a test. Pure logic (layout, state, parsing) gets unit tests next to the code (`*.test.ts`); behavior you see in the app gets an end-to-end test in `e2e/`. The fake claude (`test/fake-claude/fake-claude.mjs`) behaves like the parts of the real CLI that reframe relies on; extend it when you need more.
- **Code**: TypeScript, formatted by Prettier. Code, comments, commit messages and docs for contributors are in English. Match the surrounding code: its naming, its comment density, its idioms. Comments say why, not what.
- **UI text**: every string the UI shows lives in `src/shared/i18n.ts`, in English and German. If you do not speak German, add the English text in both places and say so in the pull request.
- **Commits**: one topic per commit, with a short summary line like `fix: tiles keep their size after a restart` (`feat:`, `fix:`, `docs:`, `test:`, `chore:`) and a body that explains why.
- **Dependencies**: add as few as possible, pin exact versions, and only use releases that are at least two weeks old. Packages with install scripts must be allowed explicitly (`allowScripts` in `package.json`).

## Screenshots

The pictures in the README (`docs/screenshots/`) are taken from the real app by a script, so they can be made again after the UI changes:

```sh
npm run screenshots
```

It builds reframe and starts it a few times with demo sessions in a throwaway profile, so windows open briefly. The sessions are drawn by a stand-in for `claude` (`screenshots/demo-claude.mjs`) with the made-up conversations in `screenshots/scenes.mjs`; the scenes and their layouts are in `screenshots/capture.spec.ts`. With `SCREENSHOTS_FRAMES=<folder>`, each GIF's frames also land in that folder as PNGs, to look at them one by one. GitHub's social preview (`social-preview.png`) is uploaded by hand, under Settings → General → Social preview.

## Where things are

| Path                | What                                                                                          |
| ------------------- | --------------------------------------------------------------------------------------------- |
| `src/main/`         | Main process: state, windows, sessions, tray, hooks server, Claude Code integration, takeover |
| `src/ptyhost/`      | The PTY host process that runs every `claude` and mirrors its screen                          |
| `src/consolehelper` | Short-lived helper that attaches to other consoles (Windows only)                             |
| `src/renderer/`     | The window UI (React, xterm.js)                                                               |
| `src/shared/`       | Code both sides use: layout tree, state schema, IPC contract, translations                    |
| `e2e/`              | End-to-end tests                                                                              |
| `screenshots/`      | The script that takes the README's pictures                                                   |
| `docs/`             | Plan, findings, release notes for maintainers, logo, screenshots                              |

[docs/PLAN.md](docs/PLAN.md) and [docs/PHASE0-FINDINGS.md](docs/PHASE0-FINDINGS.md) (German) explain the design and what was learned about Claude Code, ConPTY and Windows along the way.

## License

By contributing, you agree that your contributions are licensed under the [MIT License](LICENSE).
