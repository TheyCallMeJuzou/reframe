# reframe – Plan: Desktop-App für Claude-Code-Sessions

## Context

Wer mehrere Claude-Code-Sessions parallel nutzt, hat viele einzelne Terminal-Fenster offen, und nach einem Neustart ist alles weg. **reframe** ist eine Desktop-App (Windows zuerst, dann macOS und Linux, später Open Source unter MIT), die das löst:

- Claude-Code-Sessions laufen **gekachelt und gleichzeitig sichtbar** in einem App-Fenster (keine Tabs).
- Sessions lassen sich **gruppieren**, **einklappen**, als eigenes Fenster **rausziehen** und **wieder reinziehen**.
- Externe Terminal-Fenster, in denen `claude` läuft, kann man reinziehen. Die App **übernimmt** die Session dann (Hybrid).
- Jede Änderung wird **automatisch gespeichert**, nach einem Neustart wird alles wiederhergestellt (`claude --resume`).

---

## Festgelegte Entscheidungen

| Thema | Entscheidung |
|---|---|
| Was ist eine Session | Nur Claude Code CLI. Eine Kachel startet `claude` direkt, ohne Shell. |
| Reinziehen | Hybrid: Externes Fenster draufziehen, die App liest die Session-ID aus, beendet die alte Session sauber und läuft mit `claude --resume <id>` im eigenen Terminal weiter. |
| Reinholen | Beides: Session-Liste (überall) **und** Fenster draufziehen (zuerst Windows, dann macOS und X11, unter Wayland nur die Liste). |
| Multi-Tab-Fenster | Laufen mehrere Claude-Sessions im Fenster, gibt es eine Auswahl. Das Fenster schließt sich nur, wenn danach nichts anderes mehr darin läuft. |
| Layout | Freie Splits (wie VS Code/tmux): Beim Drop wählst du über Drop-Zonen die Seite (links, rechts, oben, unten), die Trenner sind verschiebbar. |
| Einklappen | Pfeil oben links: Die Kachel wandert in eine Leiste am Rand, ein Klick holt sie an ihren alten Platz zurück. |
| Gruppen | Farbiger Rahmen mit Namen um benachbarte Kacheln. Verschieben, Einklappen und Rausziehen gehen gemeinsam. |
| Rausziehen | Es entsteht ein eigenes App-Fenster, das man wieder reinziehen kann und das beim Restore mitkommt. |
| X am Fenster | Versteckt nur dieses Fenster, die Sessions laufen weiter. Im Tray-Menü lässt es sich wieder einblenden. |
| Tray → Beenden | Alle Sessions werden sauber beendet, beim nächsten Start resumed. |
| Kachel × | Beendet den Claude-Prozess, die Session bleibt in der Liste. "Aus Liste entfernen" löscht sie nur aus reframe, das Transcript in `~/.claude` bleibt. |
| Benachrichtigungen | OS-Notification bei "fertig" und "wartet auf Antwort", nur wenn Fenster versteckt oder unfokussiert ist oder die Kachel eingeklappt ist. |
| Session-Liste | Zwei Bereiche: **In der App** (verwaltet, offen oder beendet) und **Gefunden** (extern laufend und die letzten Sessions aus `~/.claude`). |
| Neue Session | Ordner (Pflicht, mit den zuletzt benutzten), Name optional (`--name`), Schalter "Git-Worktree" (`-w`). Eingeklappt unter "Erweitert": Modell (`--model`), Permission-Mode (`--permission-mode`). |
| Stack | Electron + TypeScript + React, xterm.js, node-pty |
| Plattformen | Entwicklung auf Windows, von Anfang an portabel, CI für alle 3 OS |
| Hosting | Entwicklung auf Gitea, Push-Spiegel nach GitHub (`TheyCallMeJuzou/reframe`), dort CI und Releases für alle 3 OS |
| Lizenz / Name | MIT (Copyright "reframe Team") / **reframe** |

---

## Architektur

```
┌──────────────── Electron Main Process (Single Source of Truth) ───────────────┐
│ App-Lifecycle · Single-Instance-Lock · Tray · WindowManager · StateStore      │
│ LayoutReducer (pure) · HookServer (127.0.0.1 + Token) · Notifications         │
│ Discovery (~/.claude) · Takeover (natives Modul, Phase 5)                     │
└───────┬───────────────────────────────┬───────────────────────────────────────┘
        │ MessagePort (Steuerung)       │ IPC (Intents rein, Layout raus)
┌───────▼──────────────────┐   ┌────────▼────────────┐  ┌─────────────────────┐
│ PTY-Host (utilityProcess)│◄─►│ Renderer Fenster A  │  │ Renderer Fenster B  │
│ pro Session:             │   │ React · Split-Tree  │  │ (rausgezogen)       │
│  node-pty (ConPTY)       │   │ xterm.js-Views      │  │                     │
│  @xterm/headless-Spiegel │◄──┴─ direkte MessagePorts für Terminal-Daten ───┘
└──────────────────────────┘
```

**Kernprinzipien**
1. **Sessions leben im PTY-Host, nicht im Fenster.** Ein Renderer ist nur eine Ansicht. Wird eine Kachel verschoben, schließt sich die Ansicht im Quellfenster, und das Zielfenster hängt sich an denselben PTY. Der aktuelle Bildschirminhalt kommt per `@xterm/addon-serialize` aus dem Headless-Spiegel. Das ist das bewährte Muster aus dem VS-Code-PTY-Host.
2. **Der Main-Prozess besitzt den Zustand.** Renderer schicken nur Intents (`moveTile`, `split`, `collapse`, `group` …). Main wendet den reinen `LayoutReducer` an, speichert und sendet das neue Layout an die betroffenen Fenster. So gibt es keine Race-Conditions zwischen Fenstern.
3. **Terminal-Daten laufen am Main vorbei** über direkte MessagePorts (`MessageChannelMain`) zwischen PTY-Host und Renderer.
4. **Nichts Globales anfassen:** Hooks werden pro Session über `claude --settings <json>` mitgegeben. Die `~/.claude/settings.json` des Users bleibt unverändert.

### Claude-Integration
- **Neue Session:** `claude --session-id <uuid> [--name] [--model] [--permission-mode] [-w] --settings <hooks>`. Die UUID erzeugt reframe selbst, deshalb ist die ID von Anfang an bekannt.
- **Restore:** `claude --resume <claudeSessionId> --settings <hooks> [--name]` im gespeicherten `cwd` (bei Worktrees der Worktree-Pfad, **ohne** erneutes `-w`). `--resume` übernimmt sonst das Verzeichnis, in dem gespawnt wird.
- **Hooks** (pro Session als Datei über `--settings` injiziert, werden mit User- und Projekt-Hooks zusammengeführt) melden als **HTTP-Hooks** an den HookServer (`/hook/<tileId>/<event>`, Header `X-Reframe-Token`):
  - `UserPromptSubmit`: Status "läuft", aktualisiert `claudeSessionId` und `cwd`.
  - `Stop`: Status "fertig", plus Notification.
  - `Notification` (`permission_prompt` u.a.): Status "wartet", plus Notification.
  - `SessionStart` kann kein HTTP (Phase 0). Sofortige ID-Wechsel (`/clear`) und den Status liefert stattdessen die beobachtete Registry-Datei `~/.claude/sessions/<pid>.json` der bekannten PID.
- **Spawn-Umgebung bereinigen:** Marker einer Eltern-Claude-Session entfernen (`CLAUDECODE`, `CLAUDE_PID`, `CLAUDE_CODE_{CHILD_SESSION, SESSION_ID, …}`, siehe `docs/PHASE0-FINDINGS.md`), sonst speichert die Session keine Transcripts.
- **Claude-Binary:** über PATH + PATHEXT auflösen wie die Shell. npm-cmd-Shims werden zur echten `.exe` aufgelöst (dann gilt PTY-PID = Claude-PID). Überschreibbar in den Einstellungen und per `REFRAME_CLAUDE_BIN` (für Tests).
- **Discovery ("Gefunden")** in `src/main/discovery/`, defensiv geparst und mit Fixtures getestet:
  - Laufende externe Sessions aus `~/.claude/sessions/<pid>.json`. Die Felder sind `pid`, `sessionId`, `cwd`, `name`, `status`, `procStart` … Das Format ist **undokumentiert**, deshalb in einem Modul gekapselt, mit `procStart` gegen PID-Wiederverwendung, Fallback über die Kommandozeile des Prozesses.
  - Letzte Sessions aus `~/.claude/projects/*/*.jsonl`, nach mtime sortiert. Als Titel dienen `ai-title` bzw. der Name, angezeigt werden die Top 20 plus "Alle anzeigen" mit Suche.

### Hybrid-Übernahme (Fenster draufziehen) – Phase 5
1. Windows: `SetWinEventHook(EVENT_SYSTEM_MOVESIZESTART/END)` erkennt das Ziehen eines fremden Fensters. Liegt der Cursor über reframe, erscheinen die Drop-Zonen.
2. Beim Loslassen: Für jede laufende Session aus der Registry gilt `AttachConsole(pid)` → `GetConsoleWindow()` → Owner. Sessions, deren Owner das gedroppte HWND ist, gehören zum Fenster. Das passiert in einem kurzlebigen Hilfsprozess, siehe Phase-0-Ergebnisse; Prozessbaum und Titel sind dafür unbrauchbar. Bei mehreren Sessions (Tabs) erscheint die Auswahl.
3. Die alte Session wird sauber beendet (Konsole anhängen, 2× Ctrl+C, mit Timeout und Kill als Fallback). Danach startet `claude --resume` im Ziel-Split.
4. Das Fenster schließt sich (`WM_CLOSE`) nur, wenn darin nichts anderes mehr läuft.
5. Technik: zuerst `koffi` (FFI, keine Rust-Toolchain nötig), Fallback `napi-rs`. macOS über AXObserver (Bedienungshilfen-Berechtigung), Linux nur X11.

### Datenmodell & Persistenz (`userData/state.json`)
```ts
State = {
  version: 1,
  sessions: Record<AppSessionId, { claudeSessionId, cwd, name?, launch: {model?, permissionMode?, worktree?},
                                   lifecycle: 'open' | 'ended', createdAt, lastActiveAt }>,
  windows: Array<{ id, bounds, displayId, maximized, hidden,
                   layout: LayoutNode | null,
                   dock: Array<{ ref: TileRef | GroupRef, restoreHint: { siblingId, direction, ratio } }> }>,
}
LayoutNode = { kind: 'split', dir: 'row'|'col', children: LayoutNode[], sizes: number[] }
           | { kind: 'group', id, name, color, child: LayoutNode }   // Gruppe = Teilbaum, v1 ohne Verschachtelung
           | { kind: 'tile', sessionId }
```
- **Speichern** nach jeder Layout-Mutation (sofort) und bei Fenster-Move/Resize (500 ms entprellt). Geschrieben wird atomar: Temp-Datei, fsync, rename, dazu eine `state.json.bak`. Beim Laden prüft ein `zod`-Schema die Datei, Migrationen laufen über `version`.
- **Restore:** Fenster entstehen mit ihren Bounds (liegt ein Monitor nicht mehr vor, kommt das Fenster auf den Primärmonitor), versteckte Fenster bleiben versteckt. Jede `open`-Session wird resumed. Schlägt ein Resume fehl, zeigt die Kachel einen Fehler mit "Neu starten im selben Ordner".

### UI (React)
- **Split-Tree-Rendering** mit `react-resizable-panels` (verschachtelte PanelGroups), dazu eine eigene Drop-Zonen-Overlay-Komponente (Quadranten der Zielkachel).
- **Kachel-Header:** ▾ Einklappen · Name · Status-Punkt (läuft / wartet / fertig / beendet) · ×.
- **Gruppenrahmen:** farbige Umrandung mit Kopfleiste (Name, ▾, Drag-Griff). Gruppieren geht per Mehrfachauswahl und "Gruppieren" oder durch Drop auf den Gruppenrahmen.
- **Dock-Leiste** links mit eingeklappten Kacheln und Gruppen inklusive Status-Punkt.
- **Seitenleiste** mit der Session-Liste (ein- und ausblendbar), Dialog "Neue Session".
- **Drag zwischen Fenstern:** HTML5-DnD mit eigenem MIME-Typ (`application/x-reframe-tile`). Landet ein `dragend` ohne Ziel und liegt der Cursor außerhalb aller reframe-Fenster (`screen.getCursorScreenPoint()`), entsteht ein neues Fenster an der Cursor-Position.
- **Terminal:** `@xterm/xterm` + `addon-webgl` + `addon-fit` + `addon-unicode11` + `addon-web-links`.

### Tray & Lifecycle
- Das X versteckt das Fenster (`close` wird abgefangen). Das Tray-Menü enthält: Fenster (anzeigen/verstecken), Neue Session, Beenden.
- **Beenden:** Status speichern (Sessions bleiben `open`), an alle Sessions 2× `\x03` schicken, auf Exit warten (höchstens 5 s), dann den **Prozessbaum** killen (`taskkill /T /F` bzw. Prozessgruppe), und erst danach die App beenden. Beim nächsten Start werden verwaiste Prozesse des letzten Laufs (PID + `procStart` im State) beendet, danach wird alles resumed.
- Ein Klick auf eine Notification zeigt das Fenster, klappt die Kachel aus und fokussiert sie.
- `app.setAppUserModelId` (nötig für Windows-Toasts), `requestSingleInstanceLock`.

---

## Projektstruktur

```
reframe/
  package.json · electron.vite.config.ts · electron-builder.yml · tsconfig*.json · LICENSE (MIT)
  src/
    shared/      layout.ts (reine Baum-Operationen), state-schema.ts (zod), ipc-contract.ts
    main/        index.ts, tray.ts, windows.ts, state/store.ts, hooks/server.ts,
                 claude/launch.ts, discovery/{registry,transcripts}.ts, notifications.ts, takeover/
    ptyhost/     index.ts, session.ts (node-pty + headless + serialize)
    preload/     index.ts (contextBridge, typisierte API)
    renderer/    App.tsx, layout/{SplitView,Tile,DropZones,GroupFrame,DockStrip}.tsx,
                 terminal/TerminalView.tsx, sidebar/SessionList.tsx, dialogs/NewSessionDialog.tsx
  native/        (Phase 5) Window-Hooks
  test/          fixtures (~/.claude-Beispiele), fake-claude (Stub-CLI)
  .gitea/workflows/ci.yml   (später GitHub Actions im Spiegel)
```

---

## Phasen

**Phase 0: Spikes (Risiken früh klären)** ✅: Ergebnisse in `docs/PHASE0-FINDINGS.md`
1. Läuft `claude` in Electron + node-pty (ConPTY) + xterm.js korrekt? Zu prüfen: Farben, Unicode, Resize, Eingabe, Ctrl+C.
2. Headless-Spiegel → serialize → neues Fenster: sieht Claudes TUI danach korrekt aus?
3. HTML5-DnD zwischen zwei BrowserWindows und Erkennen von "rausgezogen ins Leere".
4. CLI-Verhalten: `--settings` mit Hooks (wird es mit den User-Hooks zusammengeführt?), HTTP-Hook möglich?, `--session-id` zusammen mit `-w`, `--resume` mit IDs aus Worktrees, Session-ID-Wechsel bei `/clear`.
5. Erkennt `koffi` + `SetWinEventHook` das Ziehen fremder Fenster?

**Phase 1: Kern (Windows)** ✅ (manuell noch offen: Tray-Menü, Benachrichtigungen): Scaffold (electron-vite, React, TS, ESLint/Prettier, Vitest), PTY-Host, ein Fenster mit Splits und Drop-Zonen, Dialog "Neue Session", Kachel-Header, Dock-Leiste, StateStore mit Restore, Tray (X versteckt, Beenden resumed), Hooks mit Status und Notifications.

**Phase 2: Mehrere Fenster** ✅ (manuell noch offen: echtes Ziehen zwischen OS-Fenstern): Rausziehen in ein neues Fenster, Drag zwischen Fenstern, versteckte Fenster im Tray, Bounds und Monitor beim Restore. Entschieden: Ein Nebenfenster ohne Kacheln (auch ohne eingeklappte) schließt sich automatisch, das erste Fenster bleibt immer.

**Phase 3: Gruppen** ✅: Rahmen, Gruppieren und Auflösen, Gruppe verschieben, einklappen und rausziehen. Entschieden:
- Anlegen per Rechtsklick auf den Kachelkopf (native Menüs: "Neue Gruppe", "Zu Gruppe hinzufügen", "Aus Gruppe entfernen"). Eine Kachel in den Rahmen ziehen fügt sie hinzu, auf den Rahmenrand ziehen nimmt sie heraus.
- Name und Farbe automatisch ("Gruppe n", nächste freie von 8 Farben). Umbenennen per Doppelklick, die Farbe über den Farbpunkt.
- Die Gruppenleiste kann einklappen (ein Eintrag in der Seitenleiste), ziehen (auch in andere oder neue Fenster), auflösen und alle Sessions beenden (ohne Rückfrage, wie das × an einer Kachel).
- Keine verschachtelten Gruppen. Klappt man die letzte Kachel einer Gruppe ein, klappt die ganze Gruppe ein.

**Phase 4: Session-Liste** ✅: Linke Seitenleiste (Knopf "Sessions", Zustand pro Fenster), Klick öffnet am Ende des Fensters, Ziehen legt gezielt ab, "Gefunden" nach Zeit mit Suche und "Mehr anzeigen". Die Übernahme extern laufender Sessions ist schon hier eingebaut: Nach einer Rückfrage wird zweimal Strg+C in deren Konsole getippt, danach geht es hier weiter. Ein selbst vergebener Name bleibt erhalten. Dazu kommt ein Log in `%APPDATA%eframelogsmain.log`. Bereiche "In der App" und "Gefunden" (Discovery über alle Projekte, Start immer im richtigen Ordner), Drag aus der Liste (beendet → resume, extern laufend → "Übernehmen?" statt eines zweiten Prozesses), "Aus Liste entfernen".

**Phase 5: Fenster draufziehen (Hybrid)**: zuerst Windows mit Multi-Tab-Auswahl, danach macOS (AX) und Linux X11. **Windows ✅** (manuell noch offen: echtes Ziehen mit der Maus), macOS und Linux offen. Entschieden:
- Vor der Übernahme kommt immer eine kurze Bestätigung in reframe. Bei mehreren Tabs gibt es eine Auswahl, vorausgewählt ist der Tab, dessen Titel zum Fenster passt.
- Was danach im Terminal übrig bleibt: Läuft in einem übernommenen Tab nur noch eine Shell, tippt reframe dort `exit`. So schließt sich nur dieser Tab, und als letzter Tab nimmt er das Fenster mit. Andere Tabs werden nie angefasst. Bewusst kein `WM_CLOSE` an das ganze Fenster: Wer in Windows Terminal die Rückfrage für mehrere Tabs abgeschaltet hat, würde sonst alle Tabs verlieren.
- Geprüft wird nur, wenn ein bekanntes Terminal-Programm gezogen wird und irgendwo ein externes Claude läuft.
- Einen Tab aus Windows Terminal über reframe ziehen: Das ist kein Fenster-Verschieben, Windows Terminal öffnet erst nach dem Loslassen ein neues Fenster für den Tab. Erscheint ein neues Terminal-Fenster über reframe, in dem ein schon vorher laufendes Claude steckt, gilt das als Drop an dieser Stelle. Während des Ziehens zeigt reframe die Drop-Vorschau, wenn das Terminal-Fenster, aus dem der Tab kommt, ein Claude enthält. Manuell bestätigt (2026-10-24), Vorschau noch offen.

**Phase 6: Open-Source-Reife**: README, CONTRIBUTING, CI-Matrix (Win/macOS/Linux) über einen GitHub-Spiegel, electron-builder (NSIS/dmg/AppImage+deb), Hinweise zum Code-Signing, optional Auto-Update und Autostart. **In Arbeit**: GitHub-Spiegel steht, CI auf allen drei Systemen grün (2026-10-24), auch die End-to-End-Tests gegen die gepackte App. Offen: manueller Test des Installers, erstes Release. Entschieden:
- CI auf GitHub über den Push-Spiegel (`.github/workflows`). `.gitea/workflows` existiert nur, damit Gitea diese Workflows nicht selbst ausführt.
- Autostart (Tray: "Mit dem System starten"): reframe startet dann versteckt im Tray, die Sessions laufen im Hintergrund weiter.
- Auto-Update über GitHub Releases (electron-updater): Windows-Installer und AppImage. macOS erst mit signiertem Build, deb über die Paketverwaltung. Installiert beim Beenden oder über "Neu starten und aktualisieren".
- Doku für Mitwirkende auf Englisch (README, CONTRIBUTING, `docs/RELEASING.md`). Die Planungsdokumente bleiben auf Deutsch.
- Windows-Installer ohne Rückfragen und ohne Adminrechte, pro Benutzer (NSIS one-click). Vorerst unsigniert, Hinweise dazu in `docs/RELEASING.md`.
- Die End-to-End-Tests laufen in CI gegen die gepackte App (`REFRAME_E2E_APP`), unter macOS und Linux vorerst nur informativ.

---

## Risiken / offene Punkte
- `~/.claude/sessions/*.json` und die Transcript-Struktur sind **undokumentiert** und können sich mit Claude-Code-Updates ändern. Deshalb gekapselt, mit Fixture-Tests und Fallbacks.
- Eine Windows-Terminal-Instanz besitzt mehrere Fenster in **einem** Prozess. Die Zuordnung Fenster → Session läuft daher über den Titel und, wenn nötig, über die Auswahl.
- Unter Wayland kann man fremde Fenster nicht beobachten, dort gibt es nur die Liste.
- Die Liste der Eltern-Session-Marker in der Umgebung kann mit neuen Claude-Code-Versionen wachsen. Deshalb gekapselt, mit einem Test, der einen frischen Spawn auf Registry-Eintrag und Transcript prüft.

## Verifikation
- **Unit (Vitest):** `shared/layout.ts` (Split, Move, Collapse/Restore-Hint, Gruppen, Rausziehen), StateStore (atomares Schreiben, kaputte Datei → `.bak`, Migration), Discovery-Parser mit Fixtures, CLI-Argument-Builder.
- **E2E (Playwright `_electron`)** mit `REFRAME_CLAUDE_BIN=test/fake-claude`. Der Stub gibt seine Argumente aus und feuert Hook-Events. Geprüft werden: 4 Sessions anlegen, splitten, gruppieren, einklappen, rausziehen, beenden, neu starten → Layout und `--resume <id>`-Aufrufe sind identisch.
- **Manuell (Windows, echtes `claude`):** 4 Sessions mit Gruppe und einem rausgezogenen Fenster → Tray → Beenden → Neustart: alles steht wieder da, die Konversationen sind fortgesetzt. Fenster per X verstecken, Claude eine Frage stellen → Toast bei "fertig". Windows-Terminal-Fenster mit laufendem `claude` reinziehen → Session wird übernommen, der Verlauf bleibt.
