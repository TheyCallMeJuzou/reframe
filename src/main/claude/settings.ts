// Per-session settings file passed via `claude --settings`. Claude Code merges its hooks with the
// user's and the project's own hooks. HTTP hooks report to reframe's local hook server.
import fs from 'node:fs';
import path from 'node:path';

export const HOOK_EVENTS = ['UserPromptSubmit', 'Stop', 'Notification'] as const;
export type HookEvent = (typeof HOOK_EVENTS)[number];
export const HOOK_TOKEN_HEADER = 'X-Reframe-Token';

export function hookSettings(port: number, token: string, sessionId: string): object {
  const hooks: Record<string, unknown> = {};
  for (const event of HOOK_EVENTS) {
    hooks[event] = [
      {
        hooks: [
          {
            type: 'http',
            url: `http://127.0.0.1:${port}/hook/${encodeURIComponent(sessionId)}/${event}`,
            headers: { [HOOK_TOKEN_HEADER]: token },
          },
        ],
      },
    ];
  }
  return { hooks };
}

export function writeHookSettings(dir: string, port: number, token: string, sessionId: string): string {
  fs.mkdirSync(dir, { recursive: true });
  const file = path.join(dir, `${sessionId}.json`);
  fs.writeFileSync(file, JSON.stringify(hookSettings(port, token, sessionId), null, 2));
  return file;
}
