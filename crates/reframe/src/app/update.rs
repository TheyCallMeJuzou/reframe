//! The updater's place in the app: when it checks, what it found, and installing what it
//! downloaded when reframe quits (see [`crate::updates`]).
//!
//! Ported from the Electron version's `src/main/app.ts` (`watchUpdates`, `restartToUpdate`, the
//! tray's update entry) and `src/main/updates.ts` (when checks run).

use std::path::PathBuf;

use futures::channel::oneshot;
use gpui_kit::*;
use reframe_core::updates::{CACHE_FOLDER, CHECK_EVERY, FEED, FIRST_CHECK};
use serde_json::json;

use super::{AppEvent, Config, Controller, CopyKind};
use crate::updates::{self, Feed, Found, Ready, UpdateMode};

/// What the updater knows and does.
pub struct Updates {
    pub(super) mode: UpdateMode,
    feed: Feed,
    /// Downloaded and waiting to be installed.
    ready: Option<Ready>,
    /// A newer version the portable copy can download.
    available: Option<String>,
    checking: bool,
    /// Left from earlier updates; cleared before the first check.
    clear_cache: bool,
    /// "Restart to update" was picked: the installer starts reframe again.
    restart: bool,
    schedule: Option<Task<()>>,
}

impl Updates {
    pub fn new(config: &Config) -> Updates {
        let mode = match config.copy {
            CopyKind::Installed => UpdateMode::Install,
            CopyKind::Portable => UpdateMode::Notify,
            CopyKind::Development | CopyKind::Release => UpdateMode::None,
        };
        let set = |name: &str| config.env.get(name).filter(|value| !value.is_empty()).cloned();
        // The tests keep their downloads in their profile, never in the user's folder.
        let cache = if config.e2e {
            config.user_data.join(CACHE_FOLDER)
        } else if cfg!(windows) {
            set("LOCALAPPDATA")
                .map_or_else(|| config.home.join("AppData").join("Local"), PathBuf::from)
                .join(CACHE_FOLDER)
        } else {
            // Where electron-updater kept 0.x's downloads on Linux.
            set("XDG_CACHE_HOME")
                .map_or_else(|| config.home.join(".cache"), PathBuf::from)
                .join(CACHE_FOLDER)
        };
        Updates {
            mode,
            feed: Feed {
                url: set("REFRAME_UPDATE_FEED").unwrap_or_else(|| FEED.to_owned()),
                cache,
                current: env!("CARGO_PKG_VERSION").to_owned(),
            },
            ready: None,
            available: None,
            checking: false,
            clear_cache: true,
            restart: false,
            schedule: None,
        }
    }
}

impl Controller {
    /// Whether this copy checks for updates at all (Settings offers to turn that off).
    pub fn update_checks(&self) -> bool {
        self.updates.mode != UpdateMode::None
    }

    /// What this copy does about updates.
    pub fn update_mode(&self) -> UpdateMode {
        self.updates.mode
    }

    /// The version downloaded and waiting to be installed.
    pub fn update_ready(&self) -> Option<&str> {
        self.updates.ready.as_ref().map(|ready| ready.version.as_str())
    }

    /// A newer version for the portable copy to download by hand.
    pub fn update_available(&self) -> Option<&str> {
        self.updates.available.as_deref()
    }

    /// Checks a while after the start, then every few hours, as long as the checks are on in
    /// Settings (turned on again, the next scheduled check runs). The end-to-end tests check
    /// when they say so.
    pub fn start_update_checks(&mut self, cx: &mut Context<Self>) {
        if self.updates.mode == UpdateMode::None || self.config.e2e {
            return;
        }
        self.updates.schedule = Some(cx.spawn(async move |this, cx| {
            cx.background_executor().timer(FIRST_CHECK).await;
            loop {
                let checked = this.update(cx, |this, cx| {
                    if this.state.preferences.check_for_updates {
                        this.check_for_updates(cx);
                    }
                });
                if checked.is_err() {
                    break;
                }
                cx.background_executor().timer(CHECK_EVERY).await;
            }
        }));
    }

    /// Checks now (in the background); what it finds comes as [`AppEvent::UpdateReady`] or
    /// [`AppEvent::UpdateAvailable`].
    pub fn check_for_updates(&mut self, cx: &mut Context<Self>) {
        let updates = &mut self.updates;
        if updates.checking || updates.mode == UpdateMode::None {
            return;
        }
        updates.checking = true;
        let clear = std::mem::take(&mut updates.clear_cache) && updates.mode == UpdateMode::Install;
        let (feed, mode, log) = (updates.feed.clone(), updates.mode, self.log.clone());
        let have = updates.ready.as_ref().map(|ready| ready.version.clone());
        let (sender, receiver) = oneshot::channel();
        std::thread::spawn(move || {
            if clear {
                updates::clear_cache(&feed.cache);
            }
            let _ = sender.send(updates::check(&feed, mode, have.as_deref(), &log));
        });
        cx.spawn(async move |this, cx| {
            let found = receiver.await.unwrap_or_else(|_| Err("the check stopped".to_owned()));
            let _ = this.update(cx, |this, cx| this.update_checked(found, cx));
        })
        .detach();
    }

    fn update_checked(&mut self, found: Result<Found, String>, cx: &mut Context<Self>) {
        self.updates.checking = false;
        match found {
            Ok(Found::Nothing) => {}
            Ok(Found::Ready(ready)) => {
                let version = ready.version.clone();
                self.log.info(
                    "update downloaded, installs on the next quit",
                    Some(json!({ "version": version, "installer": ready.installer })),
                );
                self.updates.ready = Some(ready);
                cx.emit(AppEvent::UpdateReady(version));
            }
            Ok(Found::Available(version)) => {
                if self.updates.available.as_deref() == Some(version.as_str()) {
                    return;
                }
                self.log.info(
                    "update available for the portable copy, to download by hand",
                    Some(json!({ "version": version })),
                );
                self.updates.available = Some(version.clone());
                cx.emit(AppEvent::UpdateAvailable(version));
            }
            Err(error) => {
                self.log.warn("update check failed", Some(json!({ "error": error })));
                // A newer download takes the place of the one that waited; when it fails, that one
                // is gone too.
                if self
                    .updates
                    .ready
                    .as_ref()
                    .is_some_and(|ready| !ready.installer.exists())
                {
                    self.updates.ready = None;
                    cx.emit(AppEvent::Preferences);
                }
            }
        }
    }

    /// Ends the sessions as on quit (they resume after the restart), then installs the update,
    /// which starts reframe again.
    pub fn restart_to_update(&mut self, cx: &mut Context<Self>) {
        let Some(version) = self.update_ready().map(str::to_owned) else {
            return;
        };
        self.log
            .info("restarting to install an update", Some(json!({ "version": version })));
        self.updates.restart = true;
        self.shutdown(cx);
    }

    /// Called last when reframe quits: a downloaded update is installed now, by an installer
    /// that waits for reframe to be gone.
    pub(super) fn install_update_on_quit(&mut self) {
        let Some(ready) = self.updates.ready.clone() else {
            return;
        };
        if self.updates.mode != UpdateMode::Install {
            return;
        }
        let restart = self.updates.restart;
        self.log.info(
            "installing the update",
            Some(json!({ "version": ready.version, "restart": restart })),
        );
        #[cfg(not(windows))]
        {
            // The AppImage takes the place of the running one (an installed copy on Linux is an
            // AppImage: see `CopyKind::of`).
            let Some(running) = self.config.env.get("APPIMAGE").filter(|image| !image.is_empty()) else {
                return;
            };
            match updates::install_app_image(&ready.installer, std::path::Path::new(running), restart) {
                Ok(placed) => {
                    self.log.info("update installed", Some(json!({ "appImage": placed })));
                    // Starting with the system starts the new file.
                    if self.starts_with_system
                        && let Err(error) = crate::platform::start_with_system_file(&placed)
                    {
                        self.log.warn(
                            "could not renew starting with the system",
                            Some(json!({ "error": error })),
                        );
                    }
                }
                Err(error) => self.log.warn(
                    "the update could not be installed",
                    Some(json!({ "error": error.to_string() })),
                ),
            }
        }
        #[cfg(windows)]
        {
            let env = &self.config.env;
            let fake = env
                .get("REFRAME_FAKE_INSTALLER")
                .zip(env.get("REFRAME_NODE"))
                .map(|(script, node)| (node.as_str(), script.as_str()));
            if let Err(error) = updates::run_installer(&ready.installer, restart, fake) {
                self.log.warn(
                    "the installer could not be started",
                    Some(json!({ "error": error.to_string() })),
                );
            }
        }
    }

    /// Test hook (REFRAME_E2E): this copy updates as an installed or a portable one does, from
    /// the feed the test gives (`REFRAME_UPDATE_FEED`).
    pub fn fake_update_mode(&mut self, mode: UpdateMode, cx: &mut Context<Self>) {
        self.updates.mode = mode;
        cx.emit(AppEvent::Preferences);
    }
}
