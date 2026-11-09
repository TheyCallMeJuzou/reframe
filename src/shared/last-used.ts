// When a session was last used, for the session list: a prompt sent, or Claude done or asking something. Starting
// or resuming one (as every start of reframe does) is not using it.
import type { SessionStatus } from './ipc';

/** Whether a session's status going from `before` to `after` is a use of it. */
export function countsAsUse(before: SessionStatus, after: SessionStatus): boolean {
  if (before === after) return false;
  // A prompt (busy), Claude done with it (busy → anything), or Claude asking the user (waiting).
  return after === 'busy' || after === 'waiting' || before === 'busy';
}
