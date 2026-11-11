# reframe-native

Throwaway prototype: [reframe](https://github.com/TheyCallMeJuzou/reframe) as a native
application without a webview (Rust, GPUI, `alacritty_terminal`).

It exists to answer one question before a rewrite is decided: does a GPUI terminal running real
`claude` sessions behave at least as well as the Electron version, and what does it save?
Results go into `FINDINGS.md`.

Nothing here is meant to ship.

## Build

```
cargo run --release
```

Needs the stable MSVC Rust toolchain and the Visual Studio C++ build tools.
