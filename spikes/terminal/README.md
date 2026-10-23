# Spike: Terminal, Fensterwechsel, DnD, WinEvents

Wegwerf-Code aus Phase 0, Ergebnisse siehe `docs/PHASE0-FINDINGS.md`.

```sh
npm install
npm start -- --cwd=<ordner>        # manuell: eine claude-Session (Haiku) im Ordner
npm run auto -- --cwd=<ordner>     # automatischer Durchlauf mit Screenshots
```

Logs und Screenshots landen in `output/` (`spike.log`).

**Manuelle Tests**
1. Den Kopf "claude session (drag me)" in einen leeren Desktop-Bereich ziehen → es entsteht ein neues Fenster mit der Session.
2. Den Kopf zurück auf das erste Fenster ziehen → die Session wandert zurück.
3. Ein Windows-Terminal-Fenster mit laufendem `claude` an der Titelleiste auf ein Spike-Fenster ziehen und loslassen → `spike.log` enthält `foreign drag end` und `takeover probe`.
