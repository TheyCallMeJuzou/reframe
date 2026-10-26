// Command line for starting or resuming a session.
import type { SessionRecord } from '@shared/state';

export type LaunchMode = 'new' | 'resume';

/** `settingsFile` brings reframe's hooks; without one (WSL, out of the hooks' reach) the status comes from the registry. */
export function claudeArgs(session: SessionRecord, mode: LaunchMode, settingsFile?: string): string[] {
  const args = mode === 'new' ? ['--session-id', session.claudeSessionId] : ['--resume', session.claudeSessionId];
  const name = session.claudeName === undefined ? session.name : session.claudeName;
  if (name) args.push('--name', name);
  if (session.launch.model) args.push('--model', session.launch.model);
  if (session.launch.permissionMode) args.push('--permission-mode', session.launch.permissionMode);
  // A worktree session that was started once runs in its worktree folder; -w again would make another one in it.
  if (mode === 'new' && session.launch.worktree && !session.launch.worktreeCreated) {
    args.push('--worktree');
    if (session.launch.worktreeName) args.push(session.launch.worktreeName);
  }
  if (settingsFile) args.push('--settings', settingsFile);
  return args;
}
