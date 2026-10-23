// Command hook for events that do not support HTTP hooks (SessionStart): append the payload to a log.
const fs = require('node:fs');
const path = require('node:path');

let body = '';
process.stdin.on('data', (c) => (body += c));
process.stdin.on('end', () => {
  let p = {};
  try { p = JSON.parse(body); } catch {}
  const out = process.env.REFRAME_SPIKE_OUT || __dirname;
  const entry = { at: new Date().toISOString(), event: p.hook_event_name, source: p.source, session_id: p.session_id, cwd: p.cwd, tile: process.env.REFRAME_TILE };
  fs.appendFileSync(path.join(out, 'hooks-cmd.log'), JSON.stringify(entry) + '\n');
});
