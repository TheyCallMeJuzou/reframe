// Environment for spawned claude processes.

// Markers a running Claude Code session sets for its children. If reframe was started from inside a
// Claude session, a spawned claude inheriting them becomes a "child session" that writes no transcript
// and no registry entry. Only these markers are removed; other CLAUDE_CODE_* variables are user config.
const PARENT_SESSION_MARKERS =
  /^(CLAUDECODE|CLAUDE_PID|CLAUDE_CODE_(CHILD_SESSION|SESSION_ID|SESSION_ATTENDED|BRIDGE_SESSION_ID|MESSAGING_SOCKET|MESSAGING_TOKEN|ENTRYPOINT))$/i;

export function claudeEnv(base: NodeJS.ProcessEnv, extra: Record<string, string> = {}): Record<string, string> {
  const env: Record<string, string> = {};
  for (const [key, value] of Object.entries(base)) {
    if (value !== undefined && !PARENT_SESSION_MARKERS.test(key)) env[key] = value;
  }
  return { ...env, ...extra };
}
