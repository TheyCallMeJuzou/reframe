import { describe, expect, it } from 'vitest';
import { tile, type LayoutNode } from './layout';
import { emptyWindow, type SessionRecord, type WindowState } from './state';
import { snapshotWindow, windowFromWorkspace } from './workspaces';

const record = (id: string): SessionRecord => ({
  id,
  claudeSessionId: `claude-${id}`,
  cwd: '/work',
  launch: {},
  lifecycle: 'open',
  createdAt: 0,
  lastActiveAt: 0,
});

const grouped: LayoutNode = {
  kind: 'split',
  id: 's1',
  dir: 'row',
  sizes: [50, 50],
  children: [tile('a'), { kind: 'group', id: 'g1', name: 'Group 1', color: '#d97757', child: tile('b') }],
};

const saved = (): WindowState => ({
  ...emptyWindow('w1'),
  bounds: { x: 10, y: 20, width: 900, height: 600 },
  layout: grouped,
  dock: [{ kind: 'tile', sessionId: 'c', hint: { anchor: 'g1', side: 'right', ratio: 0.5 } }],
});

describe('workspaces', () => {
  it('saves a window with its sessions, collapsed ones included', () => {
    const sessions = { a: record('a'), b: record('b'), c: record('c'), other: record('other') };
    const ws = snapshotWindow(saved(), sessions, { id: 'ws', name: 'Frontend', savedAt: 5 });
    expect(ws.window.layout).toEqual(grouped);
    expect(ws.window.bounds).toEqual({ x: 10, y: 20, width: 900, height: 600 });
    expect(ws.sessions.map((s) => s.id).sort()).toEqual(['a', 'b', 'c']);
  });

  it('opens as a window of its own, linked to it, with its sessions as reframe has them now', () => {
    const ws = snapshotWindow(
      saved(),
      { a: record('a'), b: record('b'), c: record('c') },
      { id: 'ws', name: 'F', savedAt: 0 },
    );
    const win = windowFromWorkspace(ws, 'w2', new Map([['b', 'b2']]), new Set(), () => 'new');
    expect(win.id).toBe('w2');
    expect(win.workspaceId).toBe('ws');
    expect(win.hidden).toBe(false);
    expect(JSON.stringify(win.layout)).toContain('"sessionId":"b2"');
    expect(JSON.stringify(win.layout)).toContain('"id":"g1"');
  });

  it('gives a group a new id when that one is in use elsewhere, and follows it in restore hints', () => {
    const ws = snapshotWindow(
      saved(),
      { a: record('a'), b: record('b'), c: record('c') },
      { id: 'ws', name: 'F', savedAt: 0 },
    );
    const win = windowFromWorkspace(ws, 'w2', new Map([['c', 'c2']]), new Set(['g1']), () => 'g9');
    expect(JSON.stringify(win.layout)).toContain('"id":"g9"');
    expect(win.dock[0]).toEqual({ kind: 'tile', sessionId: 'c2', hint: { anchor: 'g9', side: 'right', ratio: 0.5 } });
  });
});
