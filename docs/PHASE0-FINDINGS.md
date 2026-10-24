# Phase 0 – Ergebnisse der Spikes

Stand: 2026-10-23 · Windows 11 · Claude Code 2.1.309 · Electron 44.7.0 · node-pty 1.1.0 · xterm 6.0.0 · koffi 3.3.2

Spike-Code: `spikes/terminal/` (Electron-App mit `npm start` bzw. `npm run auto`).

## Zusammenfassung

| # | Frage | Ergebnis |
|---|---|---|
| 1 | `claude` in Electron + node-pty (ConPTY) + xterm.js | ✅ Farben, Unicode (●, ⎿, ✻), Eingabe, Slash-Commands, Permission-Dialog, Ctrl+C: alles korrekt |
| 2 | Headless-Spiegel → serialize → anderes Fenster | ✅ Die Session wurde in ein neues Fenster umgehängt, das TUI ist vollständig und korrekt |
| 3 | HTML5-DnD zwischen Fenstern / Rausziehen ins Leere | ✅ Manuell getestet: Rausziehen ins Leere erzeugt ein neues Fenster an der Cursor-Position, Zurückziehen hängt die Session wieder um |
| 4 | CLI-Verhalten (Hooks, IDs, Worktree, Resume) | ✅ Alles geklärt, Details siehe unten |
| 5 | koffi + `SetWinEventHook` / fremdes Fenster draufziehen | ✅ Drag eines Windows-Terminal-Fensters über reframe wird erkannt. ✅ Session → Fenster-Zuordnung über den Owner der Pseudo-Konsole |

## Details

### PTY-Host
- node-pty 1.1.0 bringt N-API-Prebuilds mit (win32-x64/arm64, darwin). Es läuft ohne `electron-rebuild` in Electron 44, auch im `utilityProcess`.
- Die PID, die node-pty meldet, ist **direkt die PID von `claude.exe`**, sofern wir das Binary direkt starten (siehe unten).
- Snapshot-Konsistenz: `mirror.write('', cb)` dient als Marker. Daten, die nach dem Attach ankommen, werden gepuffert und nach dem Snapshot nachgeschickt.
- 2× Ctrl+C (`\x03`) beendet Claude in ~2 s mit Exit-Code 0. Die Quit-Strategie aus dem Plan funktioniert also.
- npm 11 blockiert Install-Skripte: `koffi` und `node-pty` müssen per `allowScripts` freigegeben werden.

### Claude-Binary auflösen
- Auf diesem Rechner gibt es zwei Installationen: npm (`claude.cmd` → `…\@anthropic-ai\claude-code\bin\claude.exe`, 2.1.309) und WinGet (`claude.exe`, 2.1.224).
- reframe muss so auflösen wie die Shell (PATH + PATHEXT). Ein npm-cmd-Shim wird **zur echten `.exe` aufgelöst**, damit PTY-PID = Claude-PID gilt (wichtig für die Registry). Fallback ist `cmd.exe /d /s /c`.

### ⚠️ Umgebungsvariablen der Eltern-Session
Startet man reframe aus einer laufenden Claude-Session heraus, erbt der gestartete `claude` deren Marker. Er wird dann zur "Child Session": **keine Transcripts, kein Registry-Eintrag** (Hinweis im TUI: *"Transcript saving is off – inherited CLAUDE_CODE_CHILD_SESSION marker"*).
→ Beim Spawn gezielt entfernen: `CLAUDECODE`, `CLAUDE_PID`, `CLAUDE_CODE_{CHILD_SESSION, SESSION_ID, SESSION_ATTENDED, BRIDGE_SESSION_ID, MESSAGING_SOCKET, MESSAGING_TOKEN, ENTRYPOINT}`. **Nicht** pauschal alle `CLAUDE_CODE_*` entfernen, denn darunter sind auch echte User-Konfigs.

### Hooks
- Hooks aus `--settings <datei>` werden mit Projekt-Hooks **zusammengeführt**, beide feuern. Eine Datei pro Session vermeidet Quoting-Probleme mit JSON auf der Kommandozeile.
- **HTTP-Hooks funktionieren** für `UserPromptSubmit`, `Stop` und `Notification`, mit eigenem Header (`X-Reframe-Token`). Die Tile-ID steckt im URL-Pfad (`/hook/<tileId>/<event>`).
- **`SessionStart` unterstützt keine HTTP-Hooks**, nur Command-Hooks.
- `Notification` liefert `notification_type: "permission_prompt"` sofort, wenn der Permission-Dialog erscheint.
- Umgebungsvariablen aus dem Spawn (`REFRAME_TILE`) kommen in Command-Hooks an. Command-Hooks laufen hier über **Git Bash**.

### Session-Registry `~/.claude/sessions/<pid>.json` (undokumentiert)
- Felder: `pid, sessionId, cwd, name, status, kind, entrypoint, procStart, startedAt, updatedAt, …`
- `status`: `idle`, `busy` und `waiting` (während des Permission-Prompts).
- **`/clear` aktualisiert `sessionId` sofort**, noch vor dem nächsten Prompt.
- Ohne `--name` bekommt jeder Prozess einen automatischen Namen (`repo-e1`, nach Resume `repo-12`).

### Session-IDs, Resume, Worktrees
- `--session-id <uuid>` funktioniert, auch zusammen mit `-w <name>`.
- `/clear` erzeugt eine **neue Session-ID** (bestätigt über die Registry, den `SessionStart`-Command-Hook mit `source: "clear"` und die nächsten HTTP-Hooks).
- `--resume <id>` findet die Session **aus jedem Verzeichnis**, übernimmt aber das neue `cwd`. → Immer im gespeicherten `cwd` spawnen.
- `-w <name>` legt den Worktree unter `<repo>/.claude/worktrees/<name>` an. Das `cwd` der Session ist der Worktree, und das Transcript liegt im Projektordner des Worktrees. → Beim Restore `--resume` im Worktree-`cwd` starten, **ohne** erneutes `-w`.
- Resume zeigt den vollständigen vorherigen Verlauf.

### koffi 3 / WinEvents
- `koffi.register` + `SetWinEventHook(WINEVENT_OUTOFCONTEXT)` im Electron-Main-Prozess: Die Callbacks kommen über die Message-Loop des UI-Threads an (getestet mit `EVENT_OBJECT_NAMECHANGE`).
- Zeiger kommen in koffi 3 als BigInt. Out-Parameter (`GetWindowThreadProcessId`) gehen über Ein-Element-Arrays, Strings über einen `Buffer`.
- Packaging-Hinweis: koffi 3 ist auf Plattform-Pakete aufgeteilt (`@koromix/koffi-win32-x64` …). electron-builder muss diese mitnehmen (`asarUnpack`).

### Fremdes Fenster → Claude-Session (Windows)
- `EVENT_SYSTEM_MOVESIZESTART/END` liefert das gezogene Fenster (HWND, PID, Titel). `screen.getCursorScreenPoint()` beim Loslassen sagt, ob der Drop über reframe passiert ist.
- **Der Prozessbaum ist unbrauchbar:** Ein einziger `WindowsTerminal.exe` (hier PID 2152) hostet *alle* Terminal-Fenster. Unter ihm hängen alle Claude-Sessions aller Fenster, sogar Prozesse, die aus einem dieser Terminals gestartet wurden (z.B. reframe selbst samt seinen Sessions).
- **Titel sind unbrauchbar:** Ohne Namen heißt jede Session "✳ Claude Code".
- **Lösung, die funktioniert:** Für jede laufende Claude-Session (`~/.claude/sessions/*.json`, PID lebt) gilt: `AttachConsole(pid)` → `GetConsoleWindow()` → `GetWindow(GW_OWNER)`. Windows Terminal setzt den Owner der ConPTY-Pseudo-Konsole auf das Fenster, das den Tab hostet. Getestet mit 7 Sessions in 7 Fenstern, alle eindeutig zugeordnet. Bei conhost ist das Konsolenfenster selbst das sichtbare Fenster. Treffer: `consoleWnd`, `owner` oder `rootOwner` == gedropptes HWND. Probe: `spikes/terminal/probe-console-owner.js`.
- `AttachConsole` gehört in einen kurzlebigen Hilfsprozess, nicht in den Electron-Main (wegen dessen eigener Konsole/stdio).
- Claude Code startet sein eingebautes ripgrep als `claude.exe` (Kommandozeile `rg …`). Kandidaten deshalb nur über die Registry bestimmen, nicht über den Prozessnamen.
- Zu prüfen in Phase 5: Wird der Owner aktualisiert, wenn ein Tab in ein anderes WT-Fenster gezogen wird?

### ⚠️ Verwaiste Claude-Prozesse
Beim Schließen des Spikes (sofortiges `app.quit()`) lief der Claude-Prozess samt Kindprozessen weiter. Stirbt der PTY-Host, reißt das unter Windows den Prozess nicht zuverlässig mit.
→ Beim Beenden **erst** 2× Ctrl+C, auf Exit warten, dann `taskkill /T /F` auf den Prozessbaum, und **erst danach** die App beenden. Beim Start verwaiste Sessions des letzten Laufs (PID + `procStart` im State) erkennen und beenden, **bevor** resumed wird. Sonst laufen zwei Prozesse auf derselben Session.

### Nachträge aus Phase 1
- **Ein Transcript gibt es erst ab der ersten Nachricht.** Eine Session ohne Prompt (frisch gestartet oder direkt nach `/clear`) lässt sich nicht mit `--resume` fortsetzen. reframe merkt sich deshalb pro aktueller Session-ID, ob schon ein Prompt lief (`used`). Ohne Transcript und ohne Prompt startet die Session frisch unter **derselben ID** (`--session-id`). Fehlt das Transcript trotz Prompt, zeigt die Kachel "Verlauf fehlt" und bietet "Neue Session hier starten" an.
- **Lokale Slash-Commands (`/clear`) lösen keinen `UserPromptSubmit`-Hook aus.** Den ID-Wechsel liefert nur die Registry.
- **`electron.exe` als Node (`ELECTRON_RUN_AS_NODE`) bekommt unter ConPTY keine Ein- und Ausgabe**, weil es unter Windows ein GUI-Programm ist. Ein `.js`-Ersatz für `claude` (Tests) läuft deshalb über einen echten Node (`REFRAME_NODE`).
- **Beenden mit `app.quit()` statt `app.exit()`**, sonst halten Chromiums Hilfsprozesse das Profil noch sekundenlang fest.

### Nachträge aus Phase 2
- **`react-resizable-panels` kann die Menge der Panels und das Layout nicht in einem Schritt ändern.** `setLayout` mit drei Panels, solange die Gruppe erst zwei kennt, wirft eine Exception ("Invalid 2 panel layout"), und die reißt den ganzen React-Baum mit. Deshalb wird eine Split-Gruppe neu aufgebaut, sobald sich ihre Kinder ändern (React-`key` aus den Kinder-Keys). Die Terminals überleben das, weil sie im `TerminalRegistry` leben und nur umgehängt werden.
- **`window.close()` aus der Seite schließt kein `BrowserWindow`**, das nicht per Skript geöffnet wurde (Chromium-Regel). In Tests geht das über `app.browserWindow(page)`.
- Wird ein Drag ins Leere mit Esc abgebrochen, meldet Chromium dasselbe (`dropEffect: 'none'`) wie ein Loslassen ins Leere. Liegt der Zeiger dann außerhalb aller Fenster, wird die Kachel herausgelöst.

### Nachträge aus Phase 3
- **Ziehen innerhalb eines Fensters war seit Phase 1 kaputt.** Ein `drop`-Listener in der *Capture*-Phase hat den Drag-Zustand zurückgesetzt. Im Microtask direkt nach diesem Listener rendert React neu und entfernt die Drop-Ebene, bevor Reacts eigener `onDrop` läuft. Zurückgesetzt wird jetzt in der Bubble-Phase. Gefunden hat das erst ein Test mit echtem HTML5-Drag-and-drop.
- **Die Drop-Seite kommt aus dem `drop`-Ereignis selbst**, nicht aus einem per `dragover` gesetzten State. Bei einem schnellen Drop ist der noch nicht gerendert.
- **Playwrights `dragTo` erzeugt in Electron echte HTML5-Drag-Ereignisse inklusive eigener MIME-Typen und Daten**, allerdings nur ein einziges `dragover`. Ziehen zwischen Fenstern lässt sich so weiterhin nicht testen.
- Native Menüs (`Menu.popup`) kann Playwright nicht anklicken. Deshalb rufen die Tests dieselben öffentlichen App-Methoden auf, die auch die Menüs benutzen.

### Nachträge aus Phase 4
- **Transcript-Format:** `ai-title` (`aiTitle`, der neueste gilt), `agent-name` (`agentName`), `last-prompt` (`lastPrompt`), das `cwd` steht in jedem `user`-Eintrag. Transcripts werden mehrere MB groß, gelesen werden nur die ersten 64 KB und die letzten 256 KB.
- **Registry `nameSource`:** `user` (selbst vergeben), `auto` (von Claude erzeugt), `derived` (aus dem Ordner). Nur `user`-Namen werden bei der Übernahme mitgenommen.
- **Übernahme unter Windows funktioniert über `AttachConsole` + `WriteConsoleInputW`:** Zwei Strg+C-Tastenereignisse (VK 0x43, Scan 0x2E, Zeichen 0x03, LEFT_CTRL_PRESSED) kommen bei Claude als echte Eingabe an ("Press Ctrl-C again to exit", dann Exit 0). Getestet mit dem echten `claude` in einer ConPTY, also wie in einem Windows-Terminal-Tab. Das läuft in einem kurzlebigen `utilityProcess`, damit der Main-Prozess nie die Konsole wechselt. Unter macOS und Linux ist der Weg `SIGINT` ×2.
- **Race beim Neustarten einer Session:** Beenden (×) läuft asynchron. Ein sofortiges Wiederöffnen startete den neuen Prozess, und das Aufräumen des alten traf danach den neuen. Deshalb hat jetzt jeder Start eine Instanznummer, und PTY-Ereignisse, Aufräumen und die Strg+C-Schleife gelten nur für ihre eigene Instanz.
- Das "AttachConsole failed" auf stderr kommt aus node-pty selbst, wenn man `kill()` auf einen schon beendeten Prozess aufruft. Es ist harmlos und wird in Tests vermieden.

### Nachträge aus Phase 5
- **Fenster → Sessions:** Mit dem Hilfsprozess (`owners`) lassen sich alle extern laufenden Sessions auf einmal ihrem Fenster zuordnen. Bei Windows Terminal ist das der Owner der Pseudo-Konsole, bei conhost das Konsolenfenster selbst. Getestet mit einem cmd-Fenster, das per `start` geöffnet wurde.
- **Was in einem Tab läuft:** `GetConsoleProcessList` nach `AttachConsole`, die Namen kommen über `QueryFullProcessImageNameW`. Alle Prozesse eines Tabs teilen sich dieselbe Konsoleneingabe, und eingetipptes `exit` liest die vorderste Shell.
- **Tab aus Windows Terminal ziehen:** Das ist ein Drag & Drop der Tab-Leiste, kein Fenster-Verschieben, es kommt also kein `EVENT_SYSTEM_MOVESIZESTART`. Wird der Tab außerhalb jeder Tab-Leiste losgelassen (und das Ziel nimmt den Drop nicht an), öffnet Windows Terminal dort ein neues Fenster. Erkannt wird das über `EVENT_OBJECT_CREATE` und `EVENT_OBJECT_SHOW` für Top-Level-Fenster eines Terminal-Programms. Die Konsole des Tabs zeigt eventuell erst nach einem Moment auf das neue Fenster, deshalb wird mehrmals nachgesehen (bis 2,5 s). Ob Chromium dabei HTML5-Drag-Events bekommt, schreibt das Log mit ("something from another app is dragged over reframe").
- **Testfallen:** `spawn(…, { detached: true })` gibt cmd unter Windows *keine* Konsole, cmd beendet sich sofort, und das Kindprogramm bekommt eine eigene. Und cmd beantwortet Strg+C in Batch-Dateien mit "Batchvorgang abbrechen?". Realistisch ist `start "Titel" cmd /s /k "<befehl>"`.

## Konsequenzen für den Plan
1. **Status und Session-ID-Tracking:** Primär über HTTP-Hooks (`UserPromptSubmit`, `Stop`, `Notification`; dokumentiert). Für sofortige ID-Wechsel (`/clear`) und den Status zusätzlich die Registry-Datei der bekannten PID beobachten. Einen `SessionStart`-Command-Hook gibt es vorerst nicht, weil er eine Runtime (node) bei den Usern voraussetzen würde.
2. **Spawn-Umgebung bereinigen** (siehe oben). Gilt auch für die Dev-Umgebung.
3. **Binary-Auflösung** mit Unwrapping der npm-cmd-Shims.
4. **Restore immer im gespeicherten `cwd`**, bei Worktrees im Worktree-Pfad, ohne `-w`.
5. **Name:** Ist ein Name gesetzt, geben wir beim Resume erneut `--name` mit, sonst vergibt Claude einen neuen Auto-Namen.
6. **Übernahme unter Windows:** gedropptes HWND → Owner der Pseudo-Konsole → Session (statt Prozessbaum und Titel).
7. **Prozess-Lebenszyklus:** Exit abwarten und den Prozessbaum killen, bevor die App endet. Beim Start Waisen aufräumen.
8. **Session-Liste:** "Gefunden" zeigt Sessions **aus allen Projekten** und startet sie im richtigen Ordner. Wird eine Session gewählt, die gerade woanders läuft (Registry), bietet die App "Übernehmen?" an, statt einen zweiten Prozess auf dieselbe Session zu setzen.
