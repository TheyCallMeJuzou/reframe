// Runs claude processes for sessions and tracks what they are doing. Status comes from two sources:
// HTTP hooks (documented; prompt submitted, response finished, needs input) and Claude Code's
// session registry file for the process (undocumented; immediate session id changes on /clear).
import { EventEmitter } from 'node:events';
import fs from 'node:fs';
import type { SessionStatus, SessionView } from '@shared/ipc';
import type { SessionRecord } from '@shared/state';
import { claudeArgs, type LaunchMode } from './claude/args';
import { claudeEnv } from './claude/env';
import { readRegistryEntry, transcriptExists } from './claude/registry';
import type { ClaudeBinary } from './claude/resolve';
import { writeHookSettings } from './claude/settings';
import type { HookMessage, HookServer } from './hooks/server';
import { isAlive, killTree } from './process';
import type { PtyHostClient } from './ptyhost-client';

export type NotifyKind = 'done' | 'waiting';

interface Runtime {
  status: SessionStatus;
  mode: LaunchMode;
  startedAt: number;
  pid?: number;
  error?: SessionView['error'];
  registryName?: string;
  registryStatus?: string;
  stopping?: boolean;
  exited?: Promise<void>;
  markExited?: () => void;
}

export interface SessionManagerDeps {
  host: PtyHostClient;
  hooks: HookServer;
  claudeHome: string;
  settingsDir: string;
  resolveClaude: () => ClaudeBinary | null;
  env: NodeJS.ProcessEnv;
}

export type IdentityPatch = Partial<Pick<SessionRecord, 'claudeSessionId' | 'cwd' | 'pid' | 'used'>>;

type Events = {
  /** Something visible changed (status, label, error). */
  changed: [sessionId: string];
  /** Persisted identity changed: claude session id (after /clear), folder (worktrees), pid, or first use. */
  identity: [sessionId: string, patch: IdentityPatch];
  notify: [sessionId: string, kind: NotifyKind];
  /** The process ended on its own (not because reframe stopped it). */
  ended: [sessionId: string];
};

const EARLY_EXIT_MS = 5000;
const POLL_MS = 1000;

export class SessionManager extends EventEmitter<Events> {
  private runtimes = new Map<string, Runtime>();
  private poller: NodeJS.Timeout;

  constructor(private deps: SessionManagerDeps) {
    super();
    deps.host.on('event', (event) => {
      if (event.type === 'spawned') this.onSpawned(event.sessionId, event.pid);
      if (event.type === 'spawn-failed') this.fail(event.sessionId, 'spawnFailed');
      if (event.type === 'exit') this.onExit(event.sessionId, event.exitCode);
    });
    deps.host.on('crashed', () => {
      for (const id of this.runtimes.keys()) this.onExit(id, -1);
    });
    deps.hooks.on('hook', (message) => this.onHook(message));
    this.poller = setInterval(() => this.pollRegistry(), POLL_MS);
    this.poller.unref();
  }

  status(sessionId: string): Runtime | undefined {
    return this.runtimes.get(sessionId);
  }

  isRunning(sessionId: string): boolean {
    const status = this.runtimes.get(sessionId)?.status;
    return status === 'starting' || status === 'idle' || status === 'busy' || status === 'waiting';
  }

  /** Display name Claude gave the session (used when the user did not name it). */
  registryName(sessionId: string): string | undefined {
    return this.runtimes.get(sessionId)?.registryName;
  }

  start(session: SessionRecord, requested: LaunchMode): void {
    // Claude Code writes a transcript only from the first message on. Without one there is nothing to
    // resume: an unused session starts fresh under the same id; a used one has lost its history.
    const missing = requested === 'resume' && !transcriptExists(this.deps.claudeHome, session.claudeSessionId);
    const mode: LaunchMode = missing ? 'new' : requested;
    const runtime: Runtime = { status: 'starting', mode, startedAt: Date.now() };
    runtime.exited = new Promise((resolve) => (runtime.markExited = resolve));
    this.runtimes.set(session.id, runtime);

    if (!fs.existsSync(session.cwd)) return this.fail(session.id, 'folderMissing');
    if (missing && session.used) return this.fail(session.id, 'transcriptMissing');
    const claude = this.deps.resolveClaude();
    if (!claude) return this.fail(session.id, 'claudeNotFound');

    const settings = writeHookSettings(this.deps.settingsDir, this.deps.hooks.port, this.deps.hooks.token, session.id);
    this.deps.host.spawn({
      sessionId: session.id,
      file: claude.file,
      args: [...claude.prefixArgs, ...claudeArgs(session, mode, settings)],
      cwd: session.cwd,
      env: claudeEnv(this.deps.env, { ...claude.env, REFRAME_SESSION: session.id }),
      cols: 120,
      rows: 32,
    });
    this.emit('changed', session.id);
  }

  /** Ends a session the way a user would (Ctrl+C), then kills the process tree if it does not exit. */
  async stop(sessionId: string, timeoutMs = 5000): Promise<void> {
    const runtime = this.runtimes.get(sessionId);
    if (!runtime) return;
    runtime.stopping = true;
    if (this.isRunning(sessionId)) {
      const deadline = Date.now() + timeoutMs;
      for (let i = 0; i < 3 && Date.now() < deadline; i++) {
        this.deps.host.write(sessionId, '\x03');
        if (await settle(runtime.exited!, 700)) break;
      }
      const left = deadline - Date.now();
      if (!(await settle(runtime.exited!, Math.max(0, left))) && runtime.pid) {
        await killTree(runtime.pid);
        await settle(runtime.exited!, 1000);
      }
    }
    this.deps.host.dispose(sessionId);
    this.runtimes.delete(sessionId);
  }

  stopAll(timeoutMs = 5000): Promise<void> {
    return Promise.all([...this.runtimes.keys()].map((id) => this.stop(id, timeoutMs))).then(() => undefined);
  }

  /** Kills processes of sessions that survived a crash of a previous run, so they are not resumed twice. */
  async cleanupOrphans(sessions: SessionRecord[]): Promise<void> {
    await Promise.all(
      sessions.map(async (s) => {
        if (!s.pid || !isAlive(s.pid)) return;
        const entry = readRegistryEntry(this.deps.claudeHome, s.pid);
        if (entry && entry.sessionId === s.claudeSessionId) await killTree(s.pid);
      }),
    );
  }

  dispose(): void {
    clearInterval(this.poller);
  }

  private onSpawned(sessionId: string, pid: number): void {
    const runtime = this.runtimes.get(sessionId);
    if (!runtime) return;
    runtime.pid = pid;
    this.emit('identity', sessionId, { pid });
  }

  private onExit(sessionId: string, exitCode: number): void {
    const runtime = this.runtimes.get(sessionId);
    if (!runtime || runtime.status === 'ended' || runtime.status === 'error') {
      runtime?.markExited?.();
      return;
    }
    const early = Date.now() - runtime.startedAt < EARLY_EXIT_MS && exitCode !== 0;
    runtime.status = early ? 'error' : 'ended';
    if (early) runtime.error = runtime.mode === 'resume' ? 'resumeFailed' : 'spawnFailed';
    runtime.markExited?.();
    this.emit('changed', sessionId);
    if (!runtime.stopping) this.emit('ended', sessionId);
  }

  private fail(sessionId: string, error: NonNullable<SessionView['error']>): void {
    const runtime = this.runtimes.get(sessionId);
    if (!runtime) return;
    runtime.status = 'error';
    runtime.error = error;
    runtime.markExited?.();
    this.emit('changed', sessionId);
  }

  private onHook({ sessionId, event, payload }: HookMessage): void {
    const runtime = this.runtimes.get(sessionId);
    if (!runtime || !this.isRunning(sessionId)) return;
    if (payload.session_id || payload.cwd || event === 'UserPromptSubmit') {
      this.emit('identity', sessionId, {
        ...(payload.session_id ? { claudeSessionId: payload.session_id } : {}),
        ...(payload.cwd ? { cwd: payload.cwd } : {}),
        ...(event === 'UserPromptSubmit' ? { used: true } : {}),
      });
    }
    if (event === 'UserPromptSubmit') this.setStatus(sessionId, runtime, 'busy');
    if (event === 'Stop') {
      this.setStatus(sessionId, runtime, 'idle');
      this.emit('notify', sessionId, 'done');
    }
    // idle_prompt repeats "waiting for input" a minute after Stop; that was already announced.
    if (event === 'Notification' && payload.notification_type !== 'idle_prompt') {
      this.setStatus(sessionId, runtime, 'waiting');
      this.emit('notify', sessionId, 'waiting');
    }
  }

  private pollRegistry(): void {
    for (const [sessionId, runtime] of this.runtimes) {
      if (!runtime.pid || !this.isRunning(sessionId)) continue;
      const entry = readRegistryEntry(this.deps.claudeHome, runtime.pid);
      if (!entry) continue;
      this.emit('identity', sessionId, {
        claudeSessionId: entry.sessionId,
        ...(entry.cwd ? { cwd: entry.cwd } : {}),
        ...(entry.status === 'busy' ? { used: true } : {}),
      });
      if (entry.name !== runtime.registryName) {
        runtime.registryName = entry.name;
        this.emit('changed', sessionId);
      }
      // Follow the registry only when it changes, so it does not undo a fresher hook-based status.
      if (entry.status && entry.status !== runtime.registryStatus) {
        runtime.registryStatus = entry.status;
        this.setStatus(sessionId, runtime, entry.status);
      } else if (runtime.status === 'starting') {
        this.setStatus(sessionId, runtime, entry.status ?? 'idle');
      }
    }
  }

  private setStatus(sessionId: string, runtime: Runtime, status: SessionStatus): void {
    if (runtime.status === status) return;
    runtime.status = status;
    this.emit('changed', sessionId);
  }
}

function settle(promise: Promise<void>, ms: number): Promise<boolean> {
  return new Promise((resolve) => {
    const timer = setTimeout(() => resolve(false), ms);
    promise.then(() => {
      clearTimeout(timer);
      resolve(true);
    });
  });
}
