# Phase 0 – Ergebnisse der Spikes

Stand: 2026-10-23 · Windows 11 · Claude Code 2.1.309 · Electron 44.7.0 · node-pty 1.1.0 · xterm 6.0.0 · koffi 3.3.2

Spike-Code: `spikes/terminal/` (Electron-App mit `npm start` bzw. `npm run auto`).

## Zusammenfassung

| # | Frage | Ergebnis |
|---|---|---|
| 1 | `claude` in Electron + node-pty (ConPTY) + xterm.js | ✅ Farben, Unicode (●, ⎿, ✻), Eingabe, Slash-Commands, Permission-Dialog, Ctrl+C: alles korrekt |
| 2 | Headless-Spiegel → serialize → anderes Fenster | ✅ Die Session wurde in ein neues Fenster umgehängt, das TUI ist vollständig und korrekt |
| 3 | HTML5-DnD zwischen Fenstern / Rausziehen ins Leere | ⏳ Gebaut, der manuelle Test steht noch aus |
| 4 | CLI-Verhalten (Hooks, IDs, Worktree, Resume) | ✅ Alles geklärt, Details siehe unten |
| 5 | koffi + `SetWinEventHook` | ✅ Callbacks laufen im Electron-Main-Thread. ⏳ Fremdes Fenster draufziehen: manueller Test steht noch aus |

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

## Konsequenzen für den Plan
1. **Status und Session-ID-Tracking:** Primär über HTTP-Hooks (`UserPromptSubmit`, `Stop`, `Notification`; dokumentiert). Für sofortige ID-Wechsel (`/clear`) und den Status zusätzlich die Registry-Datei der bekannten PID beobachten. Einen `SessionStart`-Command-Hook gibt es vorerst nicht, weil er eine Runtime (node) bei den Usern voraussetzen würde.
2. **Spawn-Umgebung bereinigen** (siehe oben). Gilt auch für die Dev-Umgebung.
3. **Binary-Auflösung** mit Unwrapping der npm-cmd-Shims.
4. **Restore immer im gespeicherten `cwd`**, bei Worktrees im Worktree-Pfad, ohne `-w`.
5. **Name:** Ist ein Name gesetzt, geben wir beim Resume erneut `--name` mit, sonst vergibt Claude einen neuen Auto-Namen.

## Noch offen (manuell, braucht dich)
- **Spike 3:** Tile-Header in leeren Desktop-Bereich ziehen (→ neues Fenster) und wieder ins andere Fenster zurück.
- **Spike 5:** Ein Windows-Terminal-Fenster mit laufendem `claude` an der Titelleiste auf das Spike-Fenster ziehen. Im Log sollten dann `foreign drag end … overReframeWindow` und `takeover probe` mit `sessionId` stehen.
