# Contributing to reframe

Thanks for helping! Bug reports, ideas and pull requests are all welcome. For anything bigger than a small fix, please open an issue first so we can agree on the approach before you put time into it.

## Reporting a bug

Please include your Windows version, your reframe and Claude Code versions (`claude --version`), what you did, what you expected and what happened. The app log helps a lot: it is `logs\main.log` in reframe's profile directory (`%APPDATA%\reframe`; for the portable copy, its `data` folder). It contains folder paths and session ids, but no conversation content; look through it before you attach it.

## Setting up

You need the stable MSVC Rust toolchain (`rustup`), the Visual Studio C++ build tools, Node.js (the end-to-end tests run a fake `claude`, which is a script) and, for the installer, [NSIS](https://nsis.sourceforge.io/) 3. Then:

```sh
cargo run
```

A development build uses your normal profile (`%APPDATA%\reframe`) and your real `claude`, and it cannot run while the installed reframe runs on the same profile (it says so). To keep experiments apart, start it with another profile (`REFRAME_USER_DATA=C:\temp\reframe-dev`), or with `scripts\try-fake.ps1`, which starts it with fake sessions in a throwaway profile.

## Before you open a pull request

```sh
powershell -File scripts/check.ps1
```

It runs what CI runs: `cargo fmt --check`, clippy (warnings are errors), the unit tests and the end-to-end tests. CI also builds the installer and tries it out (`scripts\test-installer.ps1`, under names of its own, so it cannot touch an installed reframe).

- **Tests**: new behavior comes with a test. Logic without a window (layout, state, parsing, what to do when) lives in `crates/reframe-core` with unit tests next to it (`*_tests.rs` or a `tests` module); behavior you see in the app gets an end-to-end test in `crates/reframe/tests`. Those start the app with a throwaway profile and the fake claude (`test/fake-claude/fake-claude.mjs`, which behaves like the parts of the real CLI that reframe relies on: extend it when you need more) and drive it over a local test channel that exists only with `REFRAME_E2E` (`crates/reframe/src/e2e.rs`), and through UI Automation, as a screen reader does (`tests/common/ui.rs`). While they run, windows of the app and console windows appear for a moment, and the drop tests move the pointer over the app's window.
- **Against the real claude**: `crates/reframe/tests/real.rs`, on request only (see its top for what it needs; three of its tests send short prompts).
- **Code**: Rust, formatted by `cargo fmt` (`rustfmt.toml`). Code, comments, commit messages and docs for contributors are in English. Match the surrounding code: its naming, its comment density, its idioms. Comments say why, not what.
- **UI text**: every text the UI shows lives in `crates/reframe-core/src/i18n.rs`, in English and German. If you do not speak German, add the English text in both places and say so in the pull request.
- **Commits**: one topic per commit, with a short summary line like `fix: tiles keep their size after a restart` (`feat:`, `fix:`, `docs:`, `test:`, `chore:`) and a body that explains why.
- **Dependencies**: add as few as possible, and only releases that are at least two weeks old. GPUI's versions are pinned and upgraded on purpose (its Windows backend is a patched copy in `vendor/`: see its `REFRAME.md` for what to carry over).

## Where things are

| Path                       | What                                                                                                   |
| -------------------------- | ------------------------------------------------------------------------------------------------------ |
| `crates/reframe-core/`     | Everything without a window: state and its file, layout, groups, workspaces, texts, finding and starting claude, hooks, session discovery, the session list, updates |
| `crates/reframe/src/`      | The app (`reframe.exe`): sessions on pseudo consoles, the terminal view, windows, dialogs, tray, WSL, taking over terminal windows, the updater |
| `crates/reframe/tests/`    | End-to-end tests                                                                                       |
| `crates/reframe/vendor/`   | ConPTY (`conpty.dll`, `OpenConsole.exe`, from the Windows Terminal project via node-pty)               |
| `vendor/gpui-pre-windows/` | GPUI's Windows backend with reframe's changes                                                          |
| `installer/`               | The NSIS installer                                                                                     |
| `scripts/`                 | Checks, packaging, the installer's test, screenshots of windows, measurements, the website's build     |
| `test/`                    | The stand-ins the tests run: claude, WSL, the installer                                                |
| `docs/`                    | Findings, releasing, release notes, logo, screenshots                                                  |
| `site/`                    | The website on GitHub Pages (built by `scripts/site.mjs`, see docs/RELEASING.md)                       |

[docs/FINDINGS.md](docs/FINDINGS.md) has what the move from Electron to the native app found and measured. The Electron version (0.x) is in this repository's history up to `v0.10.1`.

## Screenshots

The pictures in the README (`docs/screenshots/`) are still the 0.x app's (to be taken again from this one); `scripts\capture.ps1 <folder>` saves a picture of each window of a reframe built here, and `scripts\try-fake.ps1 -Shots <folder>` starts one with fake sessions and takes them. GitHub's social preview (`social-preview.png`) is uploaded by hand, under Settings → General → Social preview.

## License

By contributing, you agree that your contributions are licensed under the [MIT License](LICENSE).
