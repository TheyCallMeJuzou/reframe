# Releasing reframe

Development happens on Gitea (`main` is always releasable; bigger work goes through short-lived branches); a push mirror copies every branch and tag to GitHub
([TheyCallMeJuzou/reframe](https://github.com/TheyCallMeJuzou/reframe)), where CI and releases run.

## Making a release

1. Make sure CI on GitHub is green for the commit you want to release.
2. Set the version in `Cargo.toml` (`[workspace.package]`, e.g. `version = "1.1.0"`; `cargo build` brings `Cargo.lock` along), write the release notes into `docs/releases/v1.1.0.md` (ending with the link to the [code signing policy](https://theycallmejuzou.github.io/reframe/code-signing.html), which SignPath asks for on every release page), and commit them on `main` (`chore: release 1.1.0`).
3. Tag it and push both: `git tag v1.1.0 && git push && git push --tags` (to Gitea; the mirror takes it to GitHub).
4. The [Release workflow](../.github/workflows/release.yml) builds the installer, `latest.yml` and the portable ZIP on Windows (`scripts/package.ps1`) and puts them into a **draft** release for that tag with those notes. It fails if the tag and `Cargo.toml` disagree.
5. Look at the draft on GitHub, try the installer, and publish it.

Publishing is what makes the update visible to installed copies: they check GitHub Releases half a minute after starting and then every six hours (unless turned off in Settings), download the installer in the background, check it against the SHA-512 in `latest.yml`, and install it on the next quit (or right away with "Restart to update" in the tray menu). Drafts and pre-releases are ignored. Installed 0.x copies (Electron) read the same `latest.yml` with electron-updater and install 1.x over themselves the same way. Publishing also brings the website's download buttons to the new version (see below).

The portable ZIP does not update itself: it tells about a new version and links to the release. macOS and Linux get no files from 1.x releases: their 0.10.x copies look for `latest-mac.yml` / `latest-linux.yml`, find none, and stay where they are.

## Building locally

```sh
powershell -File scripts/package.ps1             # dist\: reframe-Setup-<version>.exe, latest.yml, the portable ZIP
powershell -File scripts/test-installer.ps1      # builds it under names of its own and tries it out (CI does too)
```

`package.ps1` needs NSIS 3 (`makensis` on the PATH, in its usual folder, or `MAKENSIS`). The installer installs per user into `%LOCALAPPDATA%\Programs\reframe` under the uninstall entry of the Electron version's installer, so it installs over an installed 0.x, also when that copy's updater runs it (`--updated`, `/S` without a window, `--force-run` to start reframe afterwards). It removes the Electron files, keeps the user's data, shortcuts and autostart entry, and clears the updater's downloads. Run by hand while reframe runs, it asks reframe to quit as the user would (`reframe.exe --quit`).

## Website

The website ([theycallmejuzou.github.io/reframe](https://theycallmejuzou.github.io/reframe/)) is the plain HTML and CSS in `site/`: the home page, the downloads, and the code signing policy with the privacy policy. `scripts/site.mjs` builds it into `_site/` with the pictures from `docs/` and points the download buttons at the files of the latest release; a file that release does not have (the macOS and Linux downloads of 0.10) is taken from the newest release that has it (`node scripts/site.mjs`, then serve `_site/` to look at it). The [Pages workflow](../.github/workflows/pages.yml) publishes it when `site/`, the pictures or the script change on `main`, and when a release is published (GitHub Settings → Pages → Source: GitHub Actions).

The code signing policy and the privacy policy are in two places, the README and `site/code-signing.html` (the downloads page sums them up): change them together.

## Code signing (not set up yet)

Builds are unsigned for now. That works, but costs users a confirmation on the first start: without a signature, SmartScreen warns ("Windows protected your PC") until enough people have run that exact file, and every release is a new file. The plan is the [SignPath Foundation](https://signpath.org/)'s free signing for open source (Azure Trusted Signing takes individuals only in the US and Canada; an EV certificate no longer skips SmartScreen's reputation). The [policy](../README.md#code-signing-policy) is in the README; the release workflow is ready and signs once these are set:

1. [Apply](https://signpath.org/apply). Everyone in the policy's roles needs two-factor authentication on GitHub and SignPath.
2. In SignPath: a project with the slug `reframe`, the GitHub.com trusted build system linked to it, the artifact configuration from [`.signpath/artifact-configuration.xml`](../.signpath/artifact-configuration.xml), and a signing policy with the slug `release-signing` (approval by hand). A CI user's API token with permission to submit to it.
3. On GitHub (Settings → Secrets and variables → Actions): the secret `SIGNPATH_API_TOKEN` (that token) and the variable `SIGNPATH_ORGANIZATION_ID`.

From then on a release sends the installer and the portable ZIP to SignPath in one request and waits (up to four hours) until it is approved there; `scripts/signed.ps1` checks the signatures, puts the signed files in place and writes `latest.yml` anew, since the signed installer has another checksum. Once signed releases are out, say so where it still says "being set up" or "not code-signed yet": the README (Install, Code signing policy), `site/` (all three pages) and the release notes' install section.
