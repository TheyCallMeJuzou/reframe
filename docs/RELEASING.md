# Releasing reframe

Development happens on Gitea (`main` is always releasable; bigger work goes through short-lived branches); a push mirror copies every branch and tag to GitHub
([TheyCallMeJuzou/reframe](https://github.com/TheyCallMeJuzou/reframe)), where CI and releases run.

## Making a release

1. Make sure CI on GitHub is green for the commit you want to release.
2. Set the version (`npm version 0.2.0 --no-git-tag-version`), write the release notes into `docs/releases/v0.2.0.md`, and commit both on `main` (`chore: release 0.2.0`).
3. Tag it and push both: `git tag v0.2.0 && git push && git push --tags` (to Gitea; the mirror takes it to GitHub).
4. The [Release workflow](../.github/workflows/release.yml) creates a **draft** release for that tag with those notes, then builds the installers on Windows, macOS and Linux and uploads them into it. It fails if the tag and `package.json` disagree.
5. Look at the draft on GitHub, try an installer, and publish it.

Publishing is what makes the update visible to installed apps: they check GitHub Releases half a minute after starting and then every six hours, download in the background, and install on the next quit (or right away with "Restart to update" in the tray menu). Draft and pre-releases are ignored.

What updates itself: the Windows installer and the Linux AppImage. The `.deb` is updated by installing the new one; macOS needs a signed build first (see below). The portable Windows ZIP (made by `scripts/portable.mjs` from the unpacked app, `npm run dist:portable` locally) only tells about a new version and links to the release.

## Building locally

`npm run dist` builds the installers for the system you are on into `dist/` (`npm run dist:dir`: only the unpacked app, which is faster). To check a build, run the end-to-end tests against it:

```sh
npm run dist:dir
REFRAME_E2E_APP="$PWD/dist/win-unpacked/reframe.exe" npx playwright test
```

## Code signing (not set up yet)

Builds are unsigned for now. That works, but costs users a confirmation on the first start, and on macOS it rules out automatic updates.

- **Windows**: without a signature, SmartScreen warns ("Windows protected your PC") until enough people have run that exact file. Options: [Azure Trusted Signing](https://learn.microsoft.com/azure/trusted-signing/) (cheapest, electron-builder supports it with `win.azureSignOptions`), or an OV/EV certificate from a CA (`CSC_LINK` / `CSC_KEY_PASSWORD`). Either way the secrets go into the GitHub repository's Actions secrets and the release workflow.
- **macOS**: needs an Apple Developer ID (Apple Developer Program) and notarization. electron-builder signs with `CSC_LINK` / `CSC_KEY_PASSWORD` and notarizes with `APPLE_API_KEY` / `APPLE_API_KEY_ID` / `APPLE_API_ISSUER` (set `mac.notarize: true`). Then remove `CSC_IDENTITY_AUTO_DISCOVERY: 'false'` from the release workflow and the macOS exception in `src/main/updates.ts`.
- **Linux**: nothing to sign.

## Notes

- The macOS build is for Apple silicon only (the runner's architecture). An Intel build needs the x64 packages of node-pty and koffi in the build, e.g. a second macOS job on an Intel runner.
- node-pty and koffi are Node-API modules, so their prebuilt binaries work in Electron as they are (`npmRebuild: false`). On Linux node-pty has no prebuilt binary and is compiled during `npm ci`.
- node-pty's `conpty` folder (the ConPTY reframe uses on Windows) and its worker script must stay outside the app archive (`asarUnpack` in `electron-builder.yml`). The end-to-end tests against the packaged app catch it when they do not.
