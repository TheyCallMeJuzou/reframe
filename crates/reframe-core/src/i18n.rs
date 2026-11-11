//! UI strings in English and German. English is the fallback for any other system language.
//!
//! Ported from the Electron version's `src/shared/i18n.ts`. The keys keep their dotted names
//! (`tile.error.claudeNotFound`), because some are put together at run time (`tile.error.<error>`,
//! `status.<status>`, `color.<color>`); [`MessageKey::new`] is where such a name is checked.
//!
//! The two tables are in the order of the original, one line per key, and hold the texts character for
//! character. The tests make sure neither language lacks a key the other has.

use serde::{Deserialize, Serialize};

use crate::state::Language;

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[serde(rename_all = "lowercase")]
pub enum Locale {
    En,
    De,
}

impl Locale {
    /// The locale as the original spells it: `en` or `de`.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::En => "en",
            Self::De => "de",
        }
    }
}

/// How many messages there are: `en` in the original has this many keys, and `de` must have the same.
pub const MESSAGE_COUNT: usize = 215;

#[rustfmt::skip]
static EN: [(&str, &str); MESSAGE_COUNT] = [
    ("toolbar.newSession", "New session"),
    ("toolbar.sessions", "Sessions"),
    ("toolbar.newWindow", "New window"),
    ("keys.ctrl", "Ctrl"),
    ("keys.shift", "Shift"),
    ("list.label", "Session list"),
    ("list.search", "Search sessions"),
    ("list.inApp", "In reframe"),
    ("list.found", "Found"),
    ("list.emptyApp", "No sessions yet"),
    ("list.emptyFound", "Nothing found"),
    ("list.wslStopped", "{distro} (WSL) is not running: its sessions show up here once it runs."),
    ("list.wslStart", "Start"),
    ("list.more", "Show more"),
    ("list.running", "running elsewhere"),
    ("list.window", "window {n}"),
    ("list.ended", "ended"),
    ("list.usedNow", "now"),
    ("list.usedMinutes", "{n}m"),
    ("list.usedHours", "{n}h"),
    ("list.usedDays", "{n}d"),
    ("list.usedAt", "Last used {when}"),
    ("list.remove", "Remove from list"),
    ("list.openHint", "Click to open here, or drag onto a tile. Shift/Ctrl+click selects several, right-click for more."),
    ("list.newFound", "New sessions found"),
    ("list.showHint", "Click to show it"),
    ("takeover.title", "Take over session?"),
    ("takeover.message", "{label} is running in another terminal."),
    ("takeover.detail", "reframe ends it there, as if you pressed Ctrl+C twice, and continues it here with its whole history."),
    ("takeover.confirm", "Take over"),
    ("takeover.messageMany", "{count} sessions are running in other terminals: {labels}."),
    ("listMenu.open", "Open"),
    ("listMenu.openMany", "Open {count} sessions"),
    ("listMenu.group", "Open in a new group"),
    ("listMenu.groupInNewWindow", "Open in a new group in a new window"),
    ("takeover.failed", "The session could not be ended in the other terminal."),
    ("foreign.title", "Take over from the terminal"),
    ("foreign.single", "Continue this session here?"),
    ("foreign.multiple", "Several Claude sessions run in this terminal window. Which ones should move here?"),
    ("foreign.detail", "reframe ends them in the terminal, as if you pressed Ctrl+C twice, and continues them here with their whole history. If only an empty shell is left in a terminal tab, that tab is closed; other tabs stay as they are."),
    ("empty.title", "No sessions yet"),
    ("empty.hint", "Start a Claude Code session to get going."),
    ("tile.collapse", "Collapse to the side bar"),
    ("tile.zoom", "Fill the window"),
    ("tile.unzoom", "Show all tiles again"),
    ("tile.dropFiles", "Drop to type in the path"),
    ("session.rename", "Rename"),
    ("session.renameHint", "Double-click to rename"),
    ("session.automaticName", "Automatic name"),
    ("tile.end", "End session"),
    ("tile.dragHint", "Drag to move"),
    ("tile.ended", "Session ended"),
    ("tile.resume", "Resume"),
    ("tile.close", "Close"),
    ("tile.startFresh", "Start a new session here"),
    ("tile.error.resumeFailed", "This session could not be resumed."),
    ("tile.error.transcriptMissing", "Claude Code no longer has this session's transcript."),
    ("tile.error.claudeNotFound", "Claude Code was not found: `claude` is not on your PATH. Install it on this computer: claude.com/claude-code"),
    ("tile.error.spawnFailed", "Claude Code could not be started."),
    ("tile.error.folderMissing", "The session folder no longer exists."),
    ("tile.error.hostCrashed", "reframe's terminal host stopped unexpectedly, and the session with it. It can be resumed."),
    ("tile.error.runningElsewhere", "This session is running in another terminal. Take it over to continue it here."),
    ("status.starting", "Starting"),
    ("status.idle", "Ready"),
    ("status.busy", "Working"),
    ("status.waiting", "Needs you"),
    ("status.ended", "Ended"),
    ("status.error", "Error"),
    ("dock.label", "Collapsed sessions"),
    ("dock.expand", "Show {label} again"),
    ("dock.expandGroup", "Show group {label} again ({count})"),
    ("menu.newGroup", "New group"),
    ("menu.addToGroup", "Add to group"),
    ("menu.removeFromGroup", "Remove from group"),
    ("group.defaultName", "Group {n}"),
    ("group.collapse", "Collapse the group to the side bar"),
    ("group.dragHint", "Drag to move the group"),
    ("group.options", "Group options"),
    ("group.rename", "Rename"),
    ("group.renameHint", "Double-click to rename"),
    ("group.color", "Color"),
    ("group.dissolve", "Dissolve group"),
    ("group.endAll", "End all sessions"),
    ("color.orange", "Orange"),
    ("color.blue", "Blue"),
    ("color.green", "Green"),
    ("color.purple", "Purple"),
    ("color.pink", "Pink"),
    ("color.yellow", "Yellow"),
    ("color.teal", "Teal"),
    ("color.red", "Red"),
    ("dialog.title", "New Claude Code session"),
    ("dialog.folder", "Folder"),
    ("dialog.folderPlaceholder", "Path to a project folder"),
    ("dialog.browse", "Browse…"),
    ("dialog.recent", "Recent folders"),
    ("dialog.name", "Name (optional)"),
    ("dialog.namePlaceholder", "Shown on the tile and in notifications"),
    ("dialog.nameFromFolder", "Use the folder name"),
    ("dialog.worktree", "Run in its own git worktree"),
    ("dialog.worktreeName", "Worktree name (optional)"),
    ("dialog.advanced", "Advanced"),
    ("dialog.model", "Model"),
    ("dialog.modelPlaceholder", "Default"),
    ("dialog.permissionMode", "Permission mode"),
    ("dialog.default", "Default"),
    ("dialog.start", "Start"),
    ("dialog.cancel", "Cancel"),
    ("dialog.error.folderMissing", "This folder does not exist."),
    ("dialog.error.claudeNotFound", "Claude Code was not found: `claude` is not on your PATH. Install it on this computer: claude.com/claude-code"),
    ("error.claudeOnlyInWsl", "Installed it only in WSL? Choose a folder in WSL (\\\\wsl.localhost\\…), and it runs there."),
    ("error.claudeNotFoundInWsl", "Claude Code was not found in {distro} (WSL). Install it there: claude.com/claude-code"),
    ("tray.tooltip", "reframe"),
    ("find.placeholder", "Find"),
    ("find.count", "{n} of {total}"),
    ("find.found", "{total} found"),
    ("find.none", "No results"),
    ("find.next", "Next (Enter)"),
    ("find.previous", "Previous (Shift+Enter)"),
    ("find.close", "Close (Esc)"),
    ("title.waitingOne", "reframe – a session needs you"),
    ("title.waiting", "reframe – {n} sessions need you"),
    ("tray.window", "Window {n}"),
    ("tray.newSession", "New session"),
    ("tray.newWindow", "New window"),
    ("tray.quit", "Quit reframe"),
    ("tray.about", "About reframe"),
    ("fullscreen.hint", "Press F11 to exit full screen"),
    ("fullscreen.exit", "Exit full screen"),
    ("tray.workspaces", "Workspaces"),
    ("toolbar.workspaces", "Workspaces"),
    ("workspace.save", "Save this window as a workspace …"),
    ("workspace.update", "Update workspace \"{name}\""),
    ("workspace.none", "No saved workspaces"),
    ("workspace.openMark", "(open)"),
    ("workspace.open", "Open"),
    ("workspace.rename", "Rename"),
    ("workspace.delete", "Delete"),
    ("workspace.deleteQuestion", "Delete the workspace \"{name}\"?"),
    ("workspace.deleteDetail", "Only the saved layout goes; its sessions stay in the session list, and an open window of it stays open."),
    ("workspace.defaultName", "Workspace {n}"),
    ("workspace.nameTitle", "Save as a workspace"),
    ("workspace.nameHint", "Saves this window (its tiles, groups and size) under a name, to open it again later as a window of its own."),
    ("workspace.nameLabel", "Name"),
    ("workspace.saveButton", "Save"),
    ("workspace.meta", "{count} sessions · {when}"),
    ("list.workspaces", "Workspaces"),
    ("list.workspaceHint", "Click to open as a window of its own, right-click for more."),
    ("about.title", "About reframe"),
    ("about.version", "Version {version}"),
    ("about.tagline", "Your Claude Code sessions side by side, and all of them back after a restart."),
    ("settings.title", "Settings"),
    ("settings.language", "Language"),
    ("settings.languageSystem", "System ({language})"),
    ("settings.textSize", "Text size"),
    ("settings.smaller", "Smaller"),
    ("settings.bigger", "Larger"),
    ("settings.normal", "Normal"),
    ("settings.textSizeHint", "Also {bigger} and {smaller} ({normal}: normal), or the mouse wheel with {ctrl} held over a tile."),
    ("settings.snap", "Snap separators to a grid"),
    ("settings.snapHint", "While dragging, a separator lines up with the others. Hold Alt to place it freely."),
    ("settings.autostart", "Start with the system"),
    ("settings.autostartHint", "When you log in, reframe starts in the tray with its windows hidden and resumes your sessions in the background."),
    ("settings.updates", "Check for updates"),
    ("settings.updatesHint", "Half a minute after starting and then every six hours, reframe asks GitHub whether there is a newer version. Nothing else is sent."),
    ("settings.closeWindow", "Closing a window with sessions"),
    ("settings.closeWindowAsk", "Ask"),
    ("settings.closeWindowEnd", "Ends them"),
    ("settings.closeWindowKeep", "Keeps them running in the tray"),
    ("settings.closeWindowHint", "Ended sessions stay in the session list, where you can open them again. A window without sessions always just closes."),
    ("settings.claude", "Claude Code"),
    ("settings.claudeChoose", "Choose…"),
    ("settings.claudeUsePath", "Use the one on the PATH"),
    ("settings.claudeFound", "From the PATH:"),
    ("settings.claudeNotFound", "Not found on the PATH"),
    ("settings.claudeChosen", "Chosen:"),
    ("settings.claudeMissing", "Chosen, but not there any more:"),
    ("settings.claudeHint", "Sessions in WSL folders run the Claude Code of their distribution."),
    ("settings.programs", "Programs"),
    ("settings.allFiles", "All files"),
    ("settings.defaultModel", "Model for new sessions"),
    ("settings.defaultPermissionMode", "Permission mode for new sessions"),
    ("settings.launchHint", "Filled in when you start a session, where you can change them."),
    ("settings.close", "Close"),
    ("closeWindow.title", "Close window"),
    ("closeWindow.messageOne", "End the session in this window?"),
    ("closeWindow.message", "End the {count} sessions in this window?"),
    ("closeWindow.detail", "Ended, they stay in the session list and the window closes for good. Kept, they go on running and the window waits in the tray."),
    ("closeWindow.end", "End sessions"),
    ("closeWindow.keep", "Keep in the tray"),
    ("closeWindow.remember", "Don't ask again (Settings can change it)"),
    ("about.github", "reframe on GitHub"),
    ("about.star", "Give reframe a star on GitHub"),
    ("about.madeWith", "Made with ❤️ in Austria by reframe Team aka"),
    ("about.license", "License (MIT)"),
    ("about.close", "Close"),
    ("otherBuild.title", "reframe is already running"),
    ("otherBuild.message", "You started the {started}, but the {running} is running already, and only one can run at a time."),
    ("otherBuild.detail", "To use the other one, quit this one from the tray menu (\"Quit reframe\") and start it again."),
    ("otherBuild.installed", "installed version"),
    ("otherBuild.development", "development build (npm run dev)"),
    ("startFailed.title", "reframe could not start"),
    ("startFailed.message", "Something went wrong while starting: {error}\n\nThe log tells more: {log}"),
    ("state.newerTitle", "reframe cannot open its sessions"),
    ("state.newerMessage", "Your sessions and windows ({file}) were saved by a newer version of reframe. Install that version (or a newer one) to get them back: this one leaves them alone and quits."),
    ("tray.restartToUpdate", "Restart to update to {version}"),
    ("tray.downloadUpdate", "Download reframe {version}"),
    ("update.availableTitle", "reframe {version} is out"),
    ("update.availableBody", "This portable copy does not update itself. Click to download the new version."),
    ("update.readyTitle", "Update ready"),
    ("update.readyBody", "reframe {version} is installed when you quit it, or right away with \"Restart to update\" in the tray menu."),
    ("notify.done", "{label} is done"),
    ("notify.doneBody", "Claude finished responding."),
    ("notify.waiting", "{label} needs you"),
    ("notify.waitingBody", "Claude is waiting for your input."),
];

#[rustfmt::skip]
static DE: [(&str, &str); MESSAGE_COUNT] = [
    ("toolbar.newSession", "Neue Session"),
    ("toolbar.sessions", "Sessions"),
    ("toolbar.newWindow", "Neues Fenster"),
    ("keys.ctrl", "Strg"),
    ("keys.shift", "Umschalt"),
    ("list.label", "Session-Liste"),
    ("list.search", "Sessions durchsuchen"),
    ("list.inApp", "In der App"),
    ("list.found", "Gefunden"),
    ("list.emptyApp", "Noch keine Sessions"),
    ("list.emptyFound", "Nichts gefunden"),
    ("list.wslStopped", "{distro} (WSL) läuft nicht: Seine Sessions erscheinen hier, sobald es läuft."),
    ("list.wslStart", "Starten"),
    ("list.more", "Mehr anzeigen"),
    ("list.running", "läuft woanders"),
    ("list.window", "Fenster {n}"),
    ("list.ended", "beendet"),
    ("list.usedNow", "jetzt"),
    ("list.usedMinutes", "{n} Min."),
    ("list.usedHours", "{n} Std."),
    ("list.usedDays", "{n} Tg."),
    ("list.usedAt", "Zuletzt benutzt {when}"),
    ("list.remove", "Aus Liste entfernen"),
    ("list.openHint", "Klicken zum Öffnen hier, oder auf eine Kachel ziehen. Umschalt/Strg+Klick wählt mehrere, Rechtsklick für mehr."),
    ("list.newFound", "Neue Sessions gefunden"),
    ("list.showHint", "Klicken zum Anzeigen"),
    ("takeover.title", "Session übernehmen?"),
    ("takeover.message", "{label} läuft gerade in einem anderen Terminal."),
    ("takeover.detail", "reframe beendet sie dort, wie zweimal Strg+C, und setzt sie hier mit dem ganzen Verlauf fort."),
    ("takeover.confirm", "Übernehmen"),
    ("takeover.messageMany", "{count} Sessions laufen gerade in anderen Terminals: {labels}."),
    ("listMenu.open", "Öffnen"),
    ("listMenu.openMany", "{count} Sessions öffnen"),
    ("listMenu.group", "In neuer Gruppe öffnen"),
    ("listMenu.groupInNewWindow", "In neuer Gruppe in neuem Fenster öffnen"),
    ("takeover.failed", "Die Session ließ sich im anderen Terminal nicht beenden."),
    ("foreign.title", "Aus dem Terminal übernehmen"),
    ("foreign.single", "Diese Session hier fortsetzen?"),
    ("foreign.multiple", "In diesem Terminal-Fenster laufen mehrere Claude-Sessions. Welche sollen hierher?"),
    ("foreign.detail", "reframe beendet sie im Terminal, wie zweimal Strg+C, und setzt sie hier mit dem ganzen Verlauf fort. Bleibt in einem Terminal-Tab nur eine leere Shell übrig, wird dieser Tab geschlossen. Andere Tabs bleiben, wie sie sind."),
    ("empty.title", "Noch keine Sessions"),
    ("empty.hint", "Starte eine Claude-Code-Session, um loszulegen."),
    ("tile.collapse", "In die Seitenleiste einklappen"),
    ("tile.zoom", "Fenster ausfüllen"),
    ("tile.unzoom", "Wieder alle Kacheln zeigen"),
    ("tile.dropFiles", "Loslassen, um den Pfad einzufügen"),
    ("session.rename", "Umbenennen"),
    ("session.renameHint", "Doppelklick zum Umbenennen"),
    ("session.automaticName", "Automatischer Name"),
    ("tile.end", "Session beenden"),
    ("tile.dragHint", "Ziehen zum Verschieben"),
    ("tile.ended", "Session beendet"),
    ("tile.resume", "Fortsetzen"),
    ("tile.close", "Schließen"),
    ("tile.startFresh", "Neue Session hier starten"),
    ("tile.error.resumeFailed", "Diese Session konnte nicht fortgesetzt werden."),
    ("tile.error.transcriptMissing", "Claude Code hat das Transcript dieser Session nicht mehr."),
    ("tile.error.claudeNotFound", "Claude Code wurde nicht gefunden: `claude` ist nicht im PATH. Installiere es auf diesem Computer: claude.com/claude-code"),
    ("tile.error.spawnFailed", "Claude Code konnte nicht gestartet werden."),
    ("tile.error.folderMissing", "Der Ordner der Session existiert nicht mehr."),
    ("tile.error.hostCrashed", "Der Terminal-Prozess von reframe wurde unerwartet beendet und die Session mit ihm. Sie lässt sich fortsetzen."),
    ("tile.error.runningElsewhere", "Diese Session läuft in einem anderen Terminal. Übernimm sie, um sie hier fortzusetzen."),
    ("status.starting", "Startet"),
    ("status.idle", "Bereit"),
    ("status.busy", "Arbeitet"),
    ("status.waiting", "Wartet auf dich"),
    ("status.ended", "Beendet"),
    ("status.error", "Fehler"),
    ("dock.label", "Eingeklappte Sessions"),
    ("dock.expand", "{label} wieder anzeigen"),
    ("dock.expandGroup", "Gruppe {label} wieder anzeigen ({count})"),
    ("menu.newGroup", "Neue Gruppe"),
    ("menu.addToGroup", "Zu Gruppe hinzufügen"),
    ("menu.removeFromGroup", "Aus Gruppe entfernen"),
    ("group.defaultName", "Gruppe {n}"),
    ("group.collapse", "Gruppe in die Seitenleiste einklappen"),
    ("group.dragHint", "Ziehen, um die Gruppe zu verschieben"),
    ("group.options", "Gruppenoptionen"),
    ("group.rename", "Umbenennen"),
    ("group.renameHint", "Doppelklick zum Umbenennen"),
    ("group.color", "Farbe"),
    ("group.dissolve", "Gruppe auflösen"),
    ("group.endAll", "Alle Sessions beenden"),
    ("color.orange", "Orange"),
    ("color.blue", "Blau"),
    ("color.green", "Grün"),
    ("color.purple", "Lila"),
    ("color.pink", "Pink"),
    ("color.yellow", "Gelb"),
    ("color.teal", "Türkis"),
    ("color.red", "Rot"),
    ("dialog.title", "Neue Claude-Code-Session"),
    ("dialog.folder", "Ordner"),
    ("dialog.folderPlaceholder", "Pfad zu einem Projektordner"),
    ("dialog.browse", "Durchsuchen…"),
    ("dialog.recent", "Zuletzt benutzt"),
    ("dialog.name", "Name (optional)"),
    ("dialog.namePlaceholder", "Steht auf der Kachel und in Benachrichtigungen"),
    ("dialog.nameFromFolder", "Ordnernamen verwenden"),
    ("dialog.worktree", "In eigenem Git-Worktree starten"),
    ("dialog.worktreeName", "Name des Worktrees (optional)"),
    ("dialog.advanced", "Erweitert"),
    ("dialog.model", "Modell"),
    ("dialog.modelPlaceholder", "Standard"),
    ("dialog.permissionMode", "Permission-Mode"),
    ("dialog.default", "Standard"),
    ("dialog.start", "Starten"),
    ("dialog.cancel", "Abbrechen"),
    ("dialog.error.folderMissing", "Diesen Ordner gibt es nicht."),
    ("dialog.error.claudeNotFound", "Claude Code wurde nicht gefunden: `claude` ist nicht im PATH. Installiere es auf diesem Computer: claude.com/claude-code"),
    ("error.claudeOnlyInWsl", "Nur in WSL installiert? Wähle einen Ordner in WSL (\\\\wsl.localhost\\…), dann läuft es dort."),
    ("error.claudeNotFoundInWsl", "Claude Code wurde in {distro} (WSL) nicht gefunden. Installiere es dort: claude.com/claude-code"),
    ("tray.tooltip", "reframe"),
    ("find.placeholder", "Suchen"),
    ("find.count", "{n} von {total}"),
    ("find.found", "{total} gefunden"),
    ("find.none", "Keine Treffer"),
    ("find.next", "Weiter (Eingabe)"),
    ("find.previous", "Zurück (Umschalt+Eingabe)"),
    ("find.close", "Schließen (Esc)"),
    ("title.waitingOne", "reframe – eine Session wartet auf dich"),
    ("title.waiting", "reframe – {n} Sessions warten auf dich"),
    ("tray.window", "Fenster {n}"),
    ("tray.newSession", "Neue Session"),
    ("tray.newWindow", "Neues Fenster"),
    ("tray.quit", "reframe beenden"),
    ("tray.about", "Über reframe"),
    ("fullscreen.hint", "F11 beendet den Vollbildmodus"),
    ("fullscreen.exit", "Vollbild beenden"),
    ("tray.workspaces", "Workspaces"),
    ("toolbar.workspaces", "Workspaces"),
    ("workspace.save", "Dieses Fenster als Workspace speichern …"),
    ("workspace.update", "Workspace \"{name}\" aktualisieren"),
    ("workspace.none", "Keine gespeicherten Workspaces"),
    ("workspace.openMark", "(offen)"),
    ("workspace.open", "Öffnen"),
    ("workspace.rename", "Umbenennen"),
    ("workspace.delete", "Löschen"),
    ("workspace.deleteQuestion", "Workspace \"{name}\" löschen?"),
    ("workspace.deleteDetail", "Nur das gespeicherte Layout wird gelöscht; die Sessions bleiben in der Session-Liste, und ein offenes Fenster davon bleibt offen."),
    ("workspace.defaultName", "Workspace {n}"),
    ("workspace.nameTitle", "Als Workspace speichern"),
    ("workspace.nameHint", "Speichert dieses Fenster (Kacheln, Gruppen und Größe) unter einem Namen, um es später wieder als eigenes Fenster zu öffnen."),
    ("workspace.nameLabel", "Name"),
    ("workspace.saveButton", "Speichern"),
    ("workspace.meta", "{count} Sessions · {when}"),
    ("list.workspaces", "Workspaces"),
    ("list.workspaceHint", "Klicken öffnet ihn als eigenes Fenster, Rechtsklick für mehr."),
    ("about.title", "Über reframe"),
    ("about.version", "Version {version}"),
    ("about.tagline", "Deine Claude-Code-Sessions nebeneinander, und nach einem Neustart alle wieder da."),
    ("settings.title", "Einstellungen"),
    ("settings.language", "Sprache"),
    ("settings.languageSystem", "System ({language})"),
    ("settings.textSize", "Textgröße"),
    ("settings.smaller", "Kleiner"),
    ("settings.bigger", "Größer"),
    ("settings.normal", "Normal"),
    ("settings.textSizeHint", "Auch mit {bigger} und {smaller} ({normal}: normal) oder mit dem Mausrad und gedrückter {ctrl}-Taste über einer Kachel."),
    ("settings.snap", "Trennlinien an einem Raster einrasten"),
    ("settings.snapHint", "Beim Ziehen richtet sich eine Trennlinie an den anderen aus. Mit gedrückter Alt-Taste setzt du sie frei."),
    ("settings.autostart", "Mit dem System starten"),
    ("settings.autostartHint", "Wenn du dich anmeldest, startet reframe im Tray mit verborgenen Fenstern und setzt deine Sessions im Hintergrund fort."),
    ("settings.updates", "Nach Updates suchen"),
    ("settings.updatesHint", "Eine halbe Minute nach dem Start und dann alle sechs Stunden fragt reframe bei GitHub nach, ob es eine neuere Version gibt. Sonst wird nichts gesendet."),
    ("settings.closeWindow", "Ein Fenster mit Sessions schließen"),
    ("settings.closeWindowAsk", "Fragt nach"),
    ("settings.closeWindowEnd", "Beendet sie"),
    ("settings.closeWindowKeep", "Lässt sie im Tray weiterlaufen"),
    ("settings.closeWindowHint", "Beendete Sessions bleiben in der Session-Liste, dort kannst du sie wieder öffnen. Ein Fenster ohne Sessions schließt sich immer einfach."),
    ("settings.claude", "Claude Code"),
    ("settings.claudeChoose", "Auswählen…"),
    ("settings.claudeUsePath", "Den aus dem PATH nehmen"),
    ("settings.claudeFound", "Aus dem PATH:"),
    ("settings.claudeNotFound", "Nicht im PATH gefunden"),
    ("settings.claudeChosen", "Ausgewählt:"),
    ("settings.claudeMissing", "Ausgewählt, aber nicht mehr vorhanden:"),
    ("settings.claudeHint", "Sessions in WSL-Ordnern nutzen das Claude Code ihrer Distribution."),
    ("settings.programs", "Programme"),
    ("settings.allFiles", "Alle Dateien"),
    ("settings.defaultModel", "Modell neuer Sessions"),
    ("settings.defaultPermissionMode", "Permission-Mode neuer Sessions"),
    ("settings.launchHint", "Wird beim Starten einer Session eingetragen und lässt sich dort ändern."),
    ("settings.close", "Schließen"),
    ("closeWindow.title", "Fenster schließen"),
    ("closeWindow.messageOne", "Die Session in diesem Fenster beenden?"),
    ("closeWindow.message", "Die {count} Sessions in diesem Fenster beenden?"),
    ("closeWindow.detail", "Beendet bleiben sie in der Session-Liste, und das Fenster schließt sich ganz. Behalten laufen sie weiter, und das Fenster wartet im Tray."),
    ("closeWindow.end", "Sessions beenden"),
    ("closeWindow.keep", "Im Tray behalten"),
    ("closeWindow.remember", "Nicht mehr fragen (in den Einstellungen änderbar)"),
    ("about.github", "reframe auf GitHub"),
    ("about.star", "reframe auf GitHub einen Stern geben"),
    ("about.madeWith", "Mit ❤️ in Österreich gemacht vom reframe Team aka"),
    ("about.license", "Lizenz (MIT)"),
    ("about.close", "Schließen"),
    ("otherBuild.title", "reframe läuft schon"),
    ("otherBuild.message", "Du hast die {started} gestartet, es läuft aber schon die {running}, und es kann nur eine laufen."),
    ("otherBuild.detail", "Um die andere zu nutzen, beende diese über das Tray-Menü (\"reframe beenden\") und starte die andere erneut."),
    ("otherBuild.installed", "installierte Version"),
    ("otherBuild.development", "Entwicklerversion (npm run dev)"),
    ("startFailed.title", "reframe konnte nicht starten"),
    ("startFailed.message", "Beim Starten ist etwas schiefgegangen: {error}\n\nMehr steht im Log: {log}"),
    ("state.newerTitle", "reframe kann seine Sessions nicht öffnen"),
    ("state.newerMessage", "Deine Sessions und Fenster ({file}) wurden von einer neueren reframe-Version gespeichert. Installiere diese (oder eine neuere), um sie zurückzubekommen: Diese Version lässt sie unverändert und beendet sich."),
    ("tray.restartToUpdate", "Neu starten und auf {version} aktualisieren"),
    ("tray.downloadUpdate", "reframe {version} herunterladen"),
    ("update.availableTitle", "reframe {version} ist da"),
    ("update.availableBody", "Diese portable Version aktualisiert sich nicht selbst. Klicke, um die neue Version herunterzuladen."),
    ("update.readyTitle", "Update bereit"),
    ("update.readyBody", "reframe {version} wird beim Beenden installiert, oder sofort mit \"Neu starten und aktualisieren\" im Tray-Menü."),
    ("notify.done", "{label} ist fertig"),
    ("notify.doneBody", "Claude hat geantwortet."),
    ("notify.waiting", "{label} wartet auf dich"),
    ("notify.waitingBody", "Claude braucht deine Eingabe."),
];

/// The name of a message, known to be one of the English table's (the original's `keyof typeof en`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct MessageKey(&'static str);

impl MessageKey {
    /// The key of this name, or `None` if there is no such message.
    pub fn new(key: &str) -> Option<Self> {
        EN.iter().find(|(name, _)| *name == key).map(|(name, _)| Self(name))
    }

    /// The dotted name, as in the original: `toolbar.newSession`.
    pub fn as_str(self) -> &'static str {
        self.0
    }
}

impl std::fmt::Display for MessageKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.0)
    }
}

/// Every message key, in the order of the original.
pub fn keys() -> impl Iterator<Item = MessageKey> {
    EN.iter().map(|(name, _)| MessageKey(name))
}

/// Whether there is a message of this name.
pub fn has(key: &str) -> bool {
    MessageKey::new(key).is_some()
}

pub fn resolve_locale(system_locale: &str) -> Locale {
    if system_locale.to_lowercase().starts_with("de") {
        Locale::De
    } else {
        Locale::En
    }
}

/// The locale in use: the chosen language, or the system's while the setting says "system". (In the Electron
/// version this is a line of `src/main/app.ts`, `chosen === 'system' ? this.systemLocale : chosen`.)
pub fn locale_of(language: Language, system_locale: Locale) -> Locale {
    match language {
        Language::System => system_locale,
        Language::En => Locale::En,
        Language::De => Locale::De,
    }
}

/// The text of a message in a locale, with `{name}` replaced by the parameter of that name. A text the locale
/// lacks is the English one; a `{name}` without a parameter stays as it is.
///
/// The original takes numbers as parameters too and writes them as `String(n)` does; here the caller does that.
/// Given a name twice, the later value counts, as in an object literal.
pub fn translate(locale: Locale, key: MessageKey, params: &[(&str, &str)]) -> String {
    interpolate(template(locale, key), params)
}

/// [`translate`] for a key by its name. `None` for a name that is no message: the original's types rule that
/// out, and where they are bypassed it has no text either (`undefined`, or a `TypeError` with parameters).
pub fn t(locale: Locale, key: &str, params: &[(&str, &str)]) -> Option<String> {
    MessageKey::new(key).map(|key| translate(locale, key, params))
}

/// `dictionaries[locale][key] ?? en[key]`.
fn template(locale: Locale, key: MessageKey) -> &'static str {
    let table = match locale {
        Locale::En => &EN,
        Locale::De => &DE,
    };
    text_in(table, key.0)
        .or_else(|| text_in(&EN, key.0))
        .unwrap_or_default()
}

fn text_in(table: &'static [(&'static str, &'static str)], key: &str) -> Option<&'static str> {
    table.iter().find(|(name, _)| *name == key).map(|(_, text)| *text)
}

/// `template.replace(/\{(\w+)\}/g, (m, name) => String(params[name] ?? m))`: `\w` is a letter of the English
/// alphabet, a digit or `_`, and what was put in is not looked at again.
fn interpolate(template: &str, params: &[(&str, &str)]) -> String {
    let mut out = String::with_capacity(template.len());
    let mut rest = template;
    while let Some(start) = rest.find('{') {
        out.push_str(&rest[..start]);
        let after = &rest[start + 1..];
        let length = after
            .find(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
            .unwrap_or(after.len());
        if length > 0 && after[length..].starts_with('}') {
            let name = &after[..length];
            match params.iter().rev().find(|(param, _)| *param == name) {
                Some((_, value)) => out.push_str(value),
                None => out.push_str(&rest[start..start + length + 2]),
            }
            rest = &after[length + 1..];
        } else {
            out.push('{');
            rest = after;
        }
    }
    out.push_str(rest);
    out
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::*;

    fn key(name: &str) -> MessageKey {
        MessageKey::new(name).expect("a message key")
    }

    // There is no i18n test file in the Electron version; these cover what its two functions do.

    #[test]
    fn both_tables_have_as_many_entries_as_the_original() {
        // `const en = { … }` in src/shared/i18n.ts has 215 keys, and `de` is a `Record<MessageKey, string>`.
        assert_eq!(EN.len(), 215);
        assert_eq!(DE.len(), 215);
        assert_eq!(keys().count(), 215);
    }

    #[test]
    fn every_english_key_has_a_german_text_and_the_other_way_round() {
        let english: Vec<&str> = EN.iter().map(|(name, _)| *name).collect();
        let german: Vec<&str> = DE.iter().map(|(name, _)| *name).collect();
        let missing_in_german: Vec<&&str> = english.iter().filter(|name| !german.contains(name)).collect();
        let missing_in_english: Vec<&&str> = german.iter().filter(|name| !english.contains(name)).collect();
        assert!(missing_in_german.is_empty(), "no German text: {missing_in_german:?}");
        assert!(missing_in_english.is_empty(), "no English text: {missing_in_english:?}");
        // The same order too, so the tables read side by side.
        assert_eq!(english, german);
    }

    #[test]
    fn no_key_is_there_twice() {
        for table in [&EN, &DE] {
            let names: HashSet<&str> = table.iter().map(|(name, _)| *name).collect();
            assert_eq!(names.len(), table.len());
        }
    }

    #[test]
    fn no_text_is_empty_and_both_languages_take_the_same_parameters() {
        fn params(text: &str) -> Vec<String> {
            let mut names: Vec<String> = text
                .split('{')
                .skip(1)
                .filter_map(|piece| piece.split_once('}'))
                .map(|(name, _)| name.to_string())
                .collect();
            names.sort();
            names
        }
        for ((name, english), (_, german)) in EN.iter().zip(DE.iter()) {
            assert!(!english.is_empty() && !german.is_empty(), "{name}");
            assert_eq!(params(english), params(german), "{name}");
        }
    }

    #[test]
    fn translates_into_the_locale() {
        assert_eq!(translate(Locale::En, key("toolbar.newSession"), &[]), "New session");
        assert_eq!(translate(Locale::De, key("toolbar.newSession"), &[]), "Neue Session");
        assert_eq!(translate(Locale::En, key("dialog.browse"), &[]), "Browse…");
        assert_eq!(
            translate(Locale::De, key("title.waitingOne"), &[]),
            "reframe – eine Session wartet auf dich"
        );
        assert_eq!(
            translate(Locale::En, key("error.claudeOnlyInWsl"), &[]),
            "Installed it only in WSL? Choose a folder in WSL (\\\\wsl.localhost\\…), and it runs there."
        );
    }

    #[test]
    fn puts_the_parameters_in() {
        assert_eq!(
            translate(Locale::En, key("notify.done"), &[("label", "shop")]),
            "shop is done"
        );
        assert_eq!(
            translate(Locale::De, key("find.count"), &[("n", "2"), ("total", "7")]),
            "2 von 7"
        );
        assert_eq!(
            translate(
                Locale::En,
                key("startFailed.message"),
                &[("error", "Error: no"), ("log", "C:\\log.txt")]
            ),
            "Something went wrong while starting: Error: no\n\nThe log tells more: C:\\log.txt"
        );
        // A parameter the text does not name is ignored.
        assert_eq!(translate(Locale::En, key("list.usedNow"), &[("n", "3")]), "now");
    }

    #[test]
    fn leaves_a_placeholder_without_a_parameter_as_it_is() {
        assert_eq!(translate(Locale::En, key("list.window"), &[]), "window {n}");
        assert_eq!(translate(Locale::En, key("list.window"), &[("m", "3")]), "window {n}");
        assert_eq!(translate(Locale::De, key("find.count"), &[("total", "7")]), "{n} von 7");
    }

    #[test]
    fn interpolates_as_the_regular_expression_of_the_original_does() {
        let params = [("n", "1"), ("a_b2", "x"), ("n", "2"), ("loop", "{n}")];
        // The later value of a name counts.
        assert_eq!(interpolate("{n}", &params), "2");
        assert_eq!(interpolate("{a_b2}{n}", &params), "x2");
        // Only `{word}` is a placeholder.
        assert_eq!(interpolate("{} { n } {n {ä} {n-1}", &params), "{} { n } {n {ä} {n-1}");
        assert_eq!(interpolate("{{n}} n} {", &params), "{2} n} {");
        // What was put in is not replaced again.
        assert_eq!(interpolate("{loop}", &params), "{n}");
        assert_eq!(interpolate("", &params), "");
    }

    #[test]
    fn a_text_missing_in_german_would_be_the_english_one() {
        // Every key has a German text, so the fallback is shown on the lookup it is made of.
        assert_eq!(
            text_in(&DE, "no.such.key").or_else(|| text_in(&EN, "list.more")),
            Some("Show more")
        );
        assert_eq!(template(Locale::De, key("list.more")), "Mehr anzeigen");
    }

    #[test]
    fn knows_its_keys_by_name() {
        assert!(has("tile.error.hostCrashed"));
        assert!(!has("tile.error.unknown"));
        assert!(!has(""));
        assert_eq!(
            MessageKey::new("status.busy").map(MessageKey::as_str),
            Some("status.busy")
        );
        assert_eq!(key("status.busy").to_string(), "status.busy");
        assert_eq!(MessageKey::new("Status.busy"), None);
        assert_eq!(keys().next(), Some(key("toolbar.newSession")));
        assert_eq!(keys().last(), Some(key("notify.waitingBody")));
    }

    #[test]
    fn has_no_text_for_a_name_that_is_no_key() {
        assert_eq!(t(Locale::En, "status.busy", &[]).as_deref(), Some("Working"));
        assert_eq!(
            t(Locale::De, &format!("tile.error.{}", "folderMissing"), &[]).as_deref(),
            Some("Der Ordner der Session existiert nicht mehr.")
        );
        assert_eq!(t(Locale::En, "status.unknown", &[]), None);
        assert_eq!(t(Locale::De, "status.unknown", &[("n", "1")]), None);
    }

    #[test]
    fn resolves_german_system_locales_to_german_and_all_others_to_english() {
        assert_eq!(resolve_locale("de"), Locale::De);
        assert_eq!(resolve_locale("de-AT"), Locale::De);
        assert_eq!(resolve_locale("DE_de"), Locale::De);
        assert_eq!(resolve_locale("en-US"), Locale::En);
        assert_eq!(resolve_locale("fr"), Locale::En);
        assert_eq!(resolve_locale(""), Locale::En);
        assert_eq!(resolve_locale("d"), Locale::En);
        assert_eq!(resolve_locale(" de"), Locale::En);
        // Only the start is looked at, as in the original, so any other language code starting with "de" is German.
        assert_eq!(resolve_locale("del"), Locale::De);
    }

    #[test]
    fn the_language_setting_goes_before_the_system_locale() {
        assert_eq!(locale_of(Language::System, Locale::De), Locale::De);
        assert_eq!(locale_of(Language::System, Locale::En), Locale::En);
        assert_eq!(locale_of(Language::En, Locale::De), Locale::En);
        assert_eq!(locale_of(Language::De, Locale::En), Locale::De);
    }

    #[test]
    fn locales_are_written_as_in_the_original() {
        assert_eq!(Locale::En.as_str(), "en");
        assert_eq!(Locale::De.as_str(), "de");
        assert_eq!(serde_json::to_string(&Locale::De).ok().as_deref(), Some("\"de\""));
        assert_eq!(serde_json::from_str::<Locale>("\"en\"").ok(), Some(Locale::En));
    }
}
