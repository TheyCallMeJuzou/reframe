// Runs claude processes for sessions and tracks what they are doing. Status comes from two sources:
// HTTP hooks (documented; prompt submitted, response finished, needs input) and Claude Code's
// session registry file for the process (undocumented; immediate session id changes on /clear).
// A session in a WSL folder runs its claude in that distribution, through wsl.exe; the hooks cannot reach reframe
// from there, so its status (and what it announces) comes from the registry alone.
import { EventEmitter } from 'node:events';
import fs from 'node:fs';
import os from 'node:os';
import type { SessionStatus, SessionView } from '@shared/ipc';
import type { SessionRecord } from '@shared/state';
import { parseWslPath, wslWindowsPath, type WslPath } from '@shared/wsl';
import { claudeArgs, type LaunchMode } from './claude/args';
import { claudeEnv } from './claude/env';
import {
  findRegistryEntry,
  forgetRegistryEntry,
  isRunningEntry,
  readRegistryEntry,
  readRegistryEntryAsync,
  transcriptExists,
  transcriptExistsAsync,
  type RegistryEntry,
} from './claude/registry';
import type { ClaudeBinary } from './claude/resolve';
import { writeHookSettings } from './claude/settings';
import type { HookMessage, HookServer } from './hooks/server';
import type { LogFn } from './log';
import { isAlive, killTree, processStart } from './process';
import type { PtyHostClient } from './ptyhost-client';
import { settle, withTimeout } from './wait';
import { SHARE_TIMEOUT_MS, type WslCommand, type WslInfo } from './wsl/distros';

export type NotifyKind = 'done' | 'waiting';

/** How sessions in WSL folders get run (see WslDistros). */
export interface WslLauncher {
  readonly share: string;
  readonly command: WslCommand;
  info(distro: string): Promise<WslInfo | null>;
  /** Ends a claude in the distribution by its Linux process id. */
  stopClaude(distro: string, pid: number): Promise<unknown>;
  /** Whether a Linux process runs in the distribution. */
  alive(distro: string, pid: number): Promise<boolean>;
}

/** A session running in WSL: its distribution, Claude's folder there, and its registry entry once found. */
interface WslRuntime {
  distro: string;
  home: string;
  claudeSessionId: string;
  linuxPid?: number;
  /** Its registry is being read (over the share); no second read starts meanwhile. */
  reading?: boolean;
}

interface Runtime {
  /** Which start of the session this is; events of an older process carry an older number. */
  instance: number;
  status: SessionStatus;
  mode: LaunchMode;
  startedAt: number;
  pid?: number;
  /** The process's start as the system tells it, to tell its registry entry from one left behind under its id. */
  procStart?: string | null;
  /** A registry entry left behind by another process was seen (and logged) for it. */
  staleSeen?: boolean;
  error?: SessionView['error'];
  registryName?: string;
  registryStatus?: string;
  stopping?: Promise<void>;
  exited?: Promise<void>;
  markExited?: () => void;
  wsl?: WslRuntime;
}

export interface SessionManagerDeps {
  host: PtyHostClient;
  hooks: HookServer;
  claudeHome: string;
  settingsDir: string;
  resolveClaude: () => ClaudeBinary | null;
  env: NodeJS.ProcessEnv;
  log?: LogFn;
  /** Windows only: runs sessions in WSL folders. */
  wsl?: WslLauncher;
}

export type IdentityPatch = Partial<Pick<SessionRecord, 'claudeSessionId' | 'cwd' | 'pid' | 'linuxPid' | 'used'>>;

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
/** How long looking for a transcript in a distribution may take; without an answer, a resume is tried. */
const TRANSCRIPT_TIMEOUT_MS = 5_000;

export class SessionManager extends EventEmitter<Events> {
  private runtimes = new Map<string, Runtime>();
  private instances = 0;
  private poller: NodeJS.Timeout;
  /** A pass over the registry is under way (one in WSL waits for a share), so the next tick skips. */
  private polling = false;
  private slowShareLoggedAt = 0;

  private log: LogFn;

  constructor(private deps: SessionManagerDeps) {
    super();
    this.log = deps.log ?? (() => {});
    deps.host.on('event', (event) => {
      if (event.type === 'error') return this.log('warn', 'error in the pty host', { error: event.message });
      const runtime = this.runtimes.get(event.sessionId);
      // A process that is still going away while its session was started again: not this runtime's business.
      if (!runtime || runtime.instance !== event.instance) return;
      if (event.type === 'spawned') this.onSpawned(event.sessionId, event.pid);
      if (event.type === 'spawn-failed') this.fail(event.sessionId, 'spawnFailed');
      if (event.type === 'exit') this.onExit(event.sessionId, event.exitCode);
    });
    deps.host.on('crashed', (code, restarting) => {
      this.log('error', 'pty host stopped unexpectedly, all its sessions are gone', { code, restarting });
      for (const [id, runtime] of this.runtimes) if (this.isRunning(id)) this.lostWithHost(id, runtime);
    });
    deps.hooks.on('hook', (message) => this.onHook(message));
    this.poller = setInterval(() => void this.pollRegistry(), POLL_MS);
    this.poller.unref();
  }

  status(sessionId: string): Runtime | undefined {
    return this.runtimes.get(sessionId);
  }

  isRunning(sessionId: string): boolean {
    const status = this.runtimes.get(sessionId)?.status;
    return status === 'starting' || status === 'idle' || status === 'busy' || status === 'waiting';
  }

  /** Running and staying so: not on its way out after it was ended (then it needs to be started again). */
  keepsRunning(sessionId: string): boolean {
    return this.isRunning(sessionId) && !this.runtimes.get(sessionId)?.stopping;
  }

  /** Display name Claude gave the session (used when the user did not name it). */
  registryName(sessionId: string): string | undefined {
    return this.runtimes.get(sessionId)?.registryName;
  }

  start(session: SessionRecord, requested: LaunchMode): void {
    const previous = this.runtimes.get(session.id);
    if (previous?.pid && previous.status !== 'ended' && previous.status !== 'error') {
      // Started again before the old process was gone (e.g. reopened right after ending it): it goes now, and
      // whoever waits for it to end (its stop) is done; its exit is not reported, being an older instance.
      void this.killOwn(previous);
      previous.markExited?.();
    }
    const runtime: Runtime = { instance: ++this.instances, status: 'starting', mode: requested, startedAt: Date.now() };
    runtime.exited = new Promise((resolve) => (runtime.markExited = resolve));
    this.runtimes.set(session.id, runtime);
    // The host died too often in a row and was left down: nothing can start.
    if (!this.deps.host.running) return this.fail(session.id, 'hostCrashed');

    const where = this.deps.wsl && parseWslPath(session.cwd, this.deps.wsl.share);
    if (where) {
      void this.startInWsl(session, requested, runtime, where);
      this.emit('changed', session.id);
      return;
    }

    if (!fs.existsSync(session.cwd)) return this.fail(session.id, 'folderMissing');
    const mode = launchMode(
      session,
      requested,
      requested !== 'resume' || transcriptExists(this.deps.claudeHome, session.claudeSessionId),
    );
    if (mode === 'lost') return this.fail(session.id, 'transcriptMissing');
    runtime.mode = mode;
    const claude = this.deps.resolveClaude();
    if (!claude) return this.fail(session.id, 'claudeNotFound');

    const settings = writeHookSettings(this.deps.settingsDir, this.deps.hooks.port, this.deps.hooks.token, session.id);
    this.log('info', 'session start', {
      session: session.id,
      claudeSession: session.claudeSessionId,
      mode,
      ...(mode !== requested ? { note: 'no transcript yet, starting fresh under the same id' } : {}),
      cwd: session.cwd,
      claude: claude.source,
    });
    this.deps.host.spawn({
      sessionId: session.id,
      instance: runtime.instance,
      file: claude.file,
      args: [...claude.prefixArgs, ...claudeArgs(session, mode, settings)],
      cwd: session.cwd,
      env: claudeEnv(this.deps.env, { ...claude.env, REFRAME_SESSION: session.id }),
      cols: 120,
      rows: 32,
    });
    this.emit('changed', session.id);
  }

  /**
   * Runs claude in the distribution of the session's WSL folder. Asking the distribution how (which starts it)
   * takes a moment; the session shows as starting meanwhile, and is not started at all if it was ended by then.
   */
  private async startInWsl(session: SessionRecord, requested: LaunchMode, runtime: Runtime, where: WslPath) {
    const wsl = this.deps.wsl!;
    const info = await wsl.info(where.distro);
    const folder = !!info?.claude && (await isFolder(session.cwd));
    // Ended, started again, or its host died while the distribution was asked.
    if (this.runtimes.get(session.id) !== runtime || runtime.stopping || runtime.status !== 'starting') {
      return runtime.markExited?.();
    }
    if (!info?.claude) return this.fail(session.id, 'claudeNotFound');
    if (!folder) return this.fail(session.id, 'folderMissing');
    const home = wslWindowsPath(where.distro, info.claudeHome, wsl.share);
    // Over the share: without an answer in time, the transcript is taken to be there (a resume without one says so).
    const transcript =
      requested !== 'resume' ||
      (await withTimeout(transcriptExistsAsync(home, session.claudeSessionId), TRANSCRIPT_TIMEOUT_MS, true));
    if (this.runtimes.get(session.id) !== runtime || runtime.stopping || runtime.status !== 'starting') {
      return runtime.markExited?.();
    }
    const mode = launchMode(session, requested, transcript);
    if (mode === 'lost') return this.fail(session.id, 'transcriptMissing');
    if (session.linuxPid) {
      await this.endWslOrphan(session, session.linuxPid, where.distro, home);
      if (this.runtimes.get(session.id) !== runtime || runtime.stopping || runtime.status !== 'starting') {
        return runtime.markExited?.();
      }
    }
    runtime.mode = mode;
    runtime.startedAt = Date.now();
    runtime.wsl = { distro: where.distro, home, claudeSessionId: session.claudeSessionId };

    this.log('info', 'session start', {
      session: session.id,
      claudeSession: session.claudeSessionId,
      mode,
      ...(mode !== requested ? { note: 'no transcript yet, starting fresh under the same id' } : {}),
      cwd: session.cwd,
      wsl: where.distro,
      claude: info.claude,
    });
    const { file, prefixArgs, env } = wsl.command;
    this.deps.host.spawn({
      sessionId: session.id,
      instance: runtime.instance,
      file,
      // Without a shell (so nothing the user's startup files print gets in), with the PATH their login shell has.
      args: [
        ...prefixArgs,
        ...['-d', where.distro, '--cd', where.path, '--exec', 'env', `PATH=${info.path}`, info.claude],
        ...claudeArgs(session, mode),
      ],
      cwd: os.homedir(),
      env: claudeEnv(this.deps.env, env),
      cols: 120,
      rows: 32,
    });
    this.emit('changed', session.id);
  }

  /** Shows a session as not started, for a reason (its conversation runs elsewhere), without starting anything. */
  refuse(sessionId: string, error: NonNullable<SessionView['error']>): void {
    if (this.isRunning(sessionId)) return;
    const runtime: Runtime = {
      instance: ++this.instances,
      status: 'error',
      error,
      mode: 'resume',
      startedAt: Date.now(),
    };
    runtime.exited = Promise.resolve();
    this.runtimes.set(sessionId, runtime);
    this.log('info', 'session not started', { session: sessionId, error });
    this.emit('changed', sessionId);
  }

  /** Ends a session the way a user would (Ctrl+C), then kills the process tree if it does not exit. */
  stop(sessionId: string, timeoutMs = 5000): Promise<void> {
    const runtime = this.runtimes.get(sessionId);
    if (!runtime) return Promise.resolve();
    // A second stop (e.g. removing a session that is still ending) joins the first one.
    runtime.stopping ??= this.stopRuntime(sessionId, runtime, timeoutMs);
    return runtime.stopping;
  }

  private async stopRuntime(sessionId: string, runtime: Runtime, timeoutMs: number): Promise<void> {
    if (this.isRunning(sessionId)) {
      const deadline = Date.now() + timeoutMs;
      for (let i = 0; i < 3 && Date.now() < deadline; i++) {
        // Once the session was started again, Ctrl+C would hit the new process; start() kills this one.
        if (this.runtimes.get(sessionId) !== runtime) break;
        this.deps.host.write(sessionId, '\x03');
        if (await settle(runtime.exited!, 700)) break;
      }
      const left = deadline - Date.now();
      if (!(await settle(runtime.exited!, Math.max(0, left))) && runtime.pid) {
        this.log('warn', 'session did not exit on Ctrl+C, killing its process tree', {
          session: sessionId,
          pid: runtime.pid,
        });
        await this.killOwn(runtime);
        await settle(runtime.exited!, 1000);
      }
    }
    this.deps.host.dispose(sessionId, runtime.instance);
    // Started again in the meantime: the new runtime stays.
    if (this.runtimes.get(sessionId) === runtime) this.runtimes.delete(sessionId);
  }

  /** Sessions with a process, running or on their way out. */
  get active(): string[] {
    return [...this.runtimes.keys()];
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
        // Its entry alone is not enough: the process may be gone and its id another program's now.
        if (entry && entry.sessionId === s.claudeSessionId && isRunningEntry(entry)) {
          this.log('warn', 'killing orphaned claude from a previous run', { session: s.id, pid: s.pid });
          await killTree(s.pid);
          forgetRegistryEntry(this.deps.claudeHome, s.pid);
        }
      }),
    );
  }

  dispose(): void {
    clearInterval(this.poller);
  }

  /**
   * The claude reframe last ran for a WSL session may still run in the distribution: reframe crashed, and ending the
   * wsl.exe in between does not reliably end what runs behind it. Resumed next to it, the conversation would run
   * twice, so it is ended first. Only that very process (its Linux id, with this session in its entry): the session
   * running in a terminal of its own is a takeover, which is asked about.
   */
  private async endWslOrphan(session: SessionRecord, linuxPid: number, distro: string, home: string): Promise<void> {
    const wsl = this.deps.wsl!;
    const entry = await withTimeout(readRegistryEntryAsync(home, linuxPid), SHARE_TIMEOUT_MS, null);
    if (entry?.sessionId !== session.claudeSessionId) return;
    if (!(await withTimeout(wsl.alive(distro, linuxPid), SHARE_TIMEOUT_MS, false))) return;
    this.log('warn', 'ending orphaned claude in wsl from a previous run', { session: session.id, distro, linuxPid });
    await wsl.stopClaude(distro, linuxPid);
  }

  /** Kills a session's process tree, and removes the registry entry the claude could not remove itself. */
  private async killOwn(runtime: Runtime): Promise<void> {
    if (!runtime.pid) return;
    await killTree(runtime.pid);
    // In WSL the entry is the Linux process's, in the distribution.
    if (!runtime.wsl) forgetRegistryEntry(this.deps.claudeHome, runtime.pid);
  }

  /**
   * A registry entry under the session's process id that another process left behind: a claude killed before it
   * could remove it, whose id Windows gave to this one. Its session is not this one's. (In WSL entries are found by
   * their session id, and the distribution's clock may differ.)
   */
  private isStale(sessionId: string, runtime: Runtime, entry: RegistryEntry): boolean {
    if (runtime.wsl) return false;
    const stale =
      entry.procStart && runtime.procStart
        ? entry.procStart !== runtime.procStart
        : entry.startedAt !== undefined && entry.startedAt < runtime.startedAt - 2_000;
    if (stale && !runtime.staleSeen) {
      runtime.staleSeen = true;
      this.log('info', 'registry entry left behind by an earlier process, ignored', { session: sessionId });
    }
    return stale;
  }

  private onSpawned(sessionId: string, pid: number): void {
    const runtime = this.runtimes.get(sessionId);
    if (!runtime) return;
    runtime.pid = pid;
    runtime.procStart = processStart(pid);
    this.log('info', 'session spawned', { session: sessionId, pid });
    this.emit('identity', sessionId, { pid });
  }

  private onExit(sessionId: string, exitCode: number): void {
    const runtime = this.runtimes.get(sessionId);
    if (!runtime || runtime.status === 'ended' || runtime.status === 'error') {
      runtime?.markExited?.();
      return;
    }
    // Ended by reframe (e.g. quitting) while still starting up is no failure to start.
    const early = !runtime.stopping && Date.now() - runtime.startedAt < EARLY_EXIT_MS && exitCode !== 0;
    runtime.status = early ? 'error' : 'ended';
    if (early) runtime.error = runtime.mode === 'resume' ? 'resumeFailed' : 'spawnFailed';
    runtime.markExited?.();
    this.log(early ? 'warn' : 'info', 'session exited', {
      session: sessionId,
      exitCode,
      status: runtime.status,
      ...(runtime.error ? { error: runtime.error } : {}),
      byReframe: !!runtime.stopping,
    });
    this.emit('changed', sessionId);
    if (!runtime.stopping) this.emit('ended', sessionId);
  }

  /**
   * A running session whose PTY host died. A dying host does not reliably take its processes along on Windows, and
   * resuming next to a survivor would run the conversation twice, so what is left of it is ended.
   */
  private lostWithHost(sessionId: string, runtime: Runtime): void {
    runtime.status = 'error';
    runtime.error = 'hostCrashed';
    runtime.markExited?.();
    void this.killOwn(runtime);
    const wsl = runtime.wsl;
    if (wsl?.linuxPid) void this.deps.wsl?.stopClaude(wsl.distro, wsl.linuxPid);
    this.log('warn', 'session lost with the pty host', { session: sessionId, pid: runtime.pid });
    this.emit('changed', sessionId);
  }

  private fail(sessionId: string, error: NonNullable<SessionView['error']>): void {
    const runtime = this.runtimes.get(sessionId);
    if (!runtime) return;
    runtime.status = 'error';
    runtime.error = error;
    runtime.markExited?.();
    this.log('warn', 'session could not start', { session: sessionId, error });
    this.emit('changed', sessionId);
  }

  private onHook({ sessionId, event, payload }: HookMessage): void {
    const runtime = this.runtimes.get(sessionId);
    this.log('info', 'hook', {
      session: sessionId,
      event,
      ...(payload.notification_type ? { type: payload.notification_type } : {}),
      ...(runtime ? {} : { ignored: 'unknown session' }),
    });
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

  private async pollRegistry(): Promise<void> {
    if (this.polling) return;
    this.polling = true;
    try {
      for (const [sessionId, runtime] of [...this.runtimes]) {
        if (!runtime.pid || !this.isRunning(sessionId)) continue;
        const entry = runtime.wsl
          ? await this.wslRegistryEntry(runtime.wsl)
          : readRegistryEntry(this.deps.claudeHome, runtime.pid);
        // Nothing (yet), or the session ended or started again while its share was asked.
        if (!entry || this.runtimes.get(sessionId) !== runtime || !this.isRunning(sessionId)) continue;
        if (this.isStale(sessionId, runtime, entry)) continue;
        this.followRegistry(sessionId, runtime, entry);
      }
    } finally {
      this.polling = false;
    }
  }

  private followRegistry(sessionId: string, runtime: Runtime, entry: RegistryEntry): void {
    // In WSL the folder is a Linux path; reframe keeps it as Windows reaches it.
    const cwd =
      entry.cwd && runtime.wsl ? wslWindowsPath(runtime.wsl.distro, entry.cwd, this.deps.wsl!.share) : entry.cwd;
    this.emit('identity', sessionId, {
      claudeSessionId: entry.sessionId,
      ...(cwd ? { cwd } : {}),
      ...(entry.status === 'busy' ? { used: true } : {}),
      ...(runtime.wsl ? { linuxPid: entry.pid } : {}),
    });
    if (entry.name !== runtime.registryName) {
      runtime.registryName = entry.name;
      this.emit('changed', sessionId);
    }
    // Follow the registry only when it changes, so it does not undo a fresher hook-based status.
    if (entry.status && entry.status !== runtime.registryStatus) {
      const before = runtime.registryStatus;
      runtime.registryStatus = entry.status;
      this.setStatus(sessionId, runtime, entry.status);
      // Without hooks (WSL), the registry is also what tells that Claude is done or needs the user.
      if (runtime.wsl && entry.status === 'waiting') this.emit('notify', sessionId, 'waiting');
      if (runtime.wsl && entry.status === 'idle' && before === 'busy') this.emit('notify', sessionId, 'done');
    } else if (runtime.status === 'starting') {
      this.setStatus(sessionId, runtime, entry.status ?? 'idle');
    }
  }

  /**
   * A WSL session's registry entry. It is named after the Linux process id, which only the entry itself tells, so
   * it is found by the session id first (and again should that file go away). Null when there is none, or no answer
   * in time; a read that is still waiting for the share is not started a second time.
   */
  private async wslRegistryEntry(wsl: WslRuntime): Promise<RegistryEntry | null> {
    if (wsl.reading) return null;
    wsl.reading = true;
    const read = (async () => {
      const known = wsl.linuxPid ? await readRegistryEntryAsync(wsl.home, wsl.linuxPid) : null;
      if (known) return known;
      const found = await findRegistryEntry(wsl.home, wsl.claudeSessionId);
      wsl.linuxPid = found?.pid;
      return found;
    })().finally(() => (wsl.reading = false));
    const entry = await withTimeout(read, SHARE_TIMEOUT_MS, undefined);
    if (entry === undefined && Date.now() - this.slowShareLoggedAt > 60_000) {
      this.slowShareLoggedAt = Date.now();
      this.log('warn', 'a wsl distribution did not answer in time', { distro: wsl.distro });
    }
    return entry ?? null;
  }

  private setStatus(sessionId: string, runtime: Runtime, status: SessionStatus): void {
    if (runtime.status === status) return;
    runtime.status = status;
    this.emit('changed', sessionId);
  }
}

/**
 * How to start a session that should resume. Claude Code writes a transcript only from the first message on;
 * without one there is nothing to resume: an unused session starts fresh under the same id, a used one has lost
 * its history.
 */
function launchMode(session: SessionRecord, requested: LaunchMode, transcript: boolean): LaunchMode | 'lost' {
  if (requested !== 'resume' || transcript) return requested;
  return session.used ? 'lost' : 'new';
}

/** Whether a folder is there, without holding up everything else (a WSL folder may wait for its distribution). */
function isFolder(folder: string): Promise<boolean> {
  return fs.promises.stat(folder).then(
    (stats) => stats.isDirectory(),
    () => false,
  );
}
