import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { afterEach, describe, expect, it } from 'vitest';
import { listRunning, listRunningAsync, readTranscriptInfo, TranscriptIndex } from './discovery';

const temps: string[] = [];
function home(): string {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'reframe-discovery-'));
  temps.push(dir);
  return dir;
}
afterEach(() => {
  for (const dir of temps.splice(0)) fs.rmSync(dir, { recursive: true, force: true });
});

const line = (o: object) => JSON.stringify(o) + '\n';
const user = (content: string, cwd = 'C:\\work\\api') => line({ type: 'user', cwd, message: { content } });

function transcript(h: string, project: string, id: string, body: string, ageMs = 0): string {
  const dir = path.join(h, 'projects', project);
  fs.mkdirSync(dir, { recursive: true });
  const file = path.join(dir, `${id}.jsonl`);
  fs.writeFileSync(file, body);
  const t = new Date(Date.now() - ageMs);
  fs.utimesSync(file, t, t);
  return file;
}

describe('readTranscriptInfo', () => {
  it('prefers the agent name, then the latest AI title, then prompts', async () => {
    const h = home();
    const titled = transcript(
      h,
      'p',
      'a',
      user('fix the login') +
        line({ type: 'ai-title', aiTitle: 'Login fix' }) +
        line({ type: 'ai-title', aiTitle: 'Login and signup fix' }),
    );
    expect(await readTranscriptInfo(titled)).toEqual({ cwd: 'C:\\work\\api', title: 'Login and signup fix' });
    const named = transcript(h, 'p', 'b', user('x') + line({ type: 'agent-name', agentName: 'api-work' }));
    expect((await readTranscriptInfo(named))?.title).toBe('api-work');
    const plain = transcript(h, 'p', 'c', user('<command-name>/clear</command-name>') + user('   explain   this  '));
    expect((await readTranscriptInfo(plain))?.title).toBe('explain this');
  });

  it('skips transcripts without a prompt and survives broken lines', async () => {
    const h = home();
    expect(await readTranscriptInfo(transcript(h, 'p', 'a', line({ type: 'mode' }) + '{broken\n'))).toBeNull();
  });

  it('reads the title from the end of a large transcript', async () => {
    const h = home();
    const filler = line({ type: 'assistant', text: 'x'.repeat(1000) }).repeat(600);
    const file = transcript(h, 'p', 'a', user('start') + filler + line({ type: 'ai-title', aiTitle: 'Late title' }));
    expect(fs.statSync(file).size).toBeGreaterThan(400 * 1024);
    expect(await readTranscriptInfo(file)).toEqual({ cwd: 'C:\\work\\api', title: 'Late title' });
  });
});

describe('TranscriptIndex', () => {
  it('lists newest first, excludes known sessions, searches title and folder, and pages', async () => {
    const h = home();
    transcript(h, 'p1', 'old', user('old work', 'C:\\work\\web'), 60_000);
    transcript(h, 'p1', 'mid', user('middle', 'C:\\work\\api'), 30_000);
    transcript(h, 'p2', 'new', user('newest', 'C:\\work\\api'));
    transcript(h, 'p2', 'mine', user('managed by reframe'));
    const index = new TranscriptIndex(h);
    const ids = async (o: { query?: string; limit?: number }) =>
      (await index.list({ limit: 10, exclude: new Set(['mine']), ...o })).items.map((i) => i.sessionId);

    expect(await ids({})).toEqual(['new', 'mid', 'old']);
    expect(await ids({ query: 'WEB' })).toEqual(['old']);
    expect(await ids({ query: 'middle' })).toEqual(['mid']);
    expect(await index.list({ limit: 2, exclude: new Set() })).toMatchObject({ hasMore: true });
  });
});

describe('listRunning', () => {
  it('lists live registry entries that reframe does not own', () => {
    const h = home();
    fs.mkdirSync(path.join(h, 'sessions'));
    const write = (pid: number, sessionId: string) =>
      fs.writeFileSync(path.join(h, 'sessions', `${pid}.json`), JSON.stringify({ pid, sessionId, cwd: 'C:\\x' }));
    write(process.pid, 'outside');
    write(999_999_999, 'dead');
    expect(listRunning(h, { pids: new Set(), sessionIds: new Set() }).map((r) => r.sessionId)).toEqual(['outside']);
    expect(listRunning(h, { pids: new Set([process.pid]), sessionIds: new Set() })).toEqual([]);
    expect(listRunning(h, { pids: new Set(), sessionIds: new Set(['outside']) })).toEqual([]);
  });

  it('lists them without blocking too, asking whether each runs (in WSL: through its /proc)', async () => {
    const h = home();
    fs.mkdirSync(path.join(h, 'sessions'));
    const write = (pid: number, sessionId: string) =>
      fs.writeFileSync(path.join(h, 'sessions', `${pid}.json`), JSON.stringify({ pid, sessionId, cwd: '/x' }));
    write(10, 'running');
    write(11, 'gone');
    write(12, 'own');
    fs.writeFileSync(path.join(h, 'sessions', '13.json'), '{ half written');
    const alive = async (pid: number) => pid !== 11;
    const none = { pids: new Set<number>(), sessionIds: new Set<string>() };
    expect((await listRunningAsync(h, none, alive)).map((r) => r.sessionId).sort()).toEqual(['own', 'running']);
    const own = { pids: new Set<number>(), sessionIds: new Set(['own']) };
    expect((await listRunningAsync(h, own, alive)).map((r) => r.sessionId)).toEqual(['running']);
    expect(await listRunningAsync(path.join(h, 'missing'), none, alive)).toEqual([]);
  });
});
