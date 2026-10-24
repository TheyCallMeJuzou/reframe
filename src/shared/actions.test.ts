import { describe, expect, it } from 'vitest';
import {
  addTile,
  applyLayoutAction,
  dropSession,
  isDropTarget,
  isLayoutAction,
  isWindowEmpty,
  windowHasSession,
} from './actions';
import { leaves, tileKey } from './layout';
import { emptyWindow } from './state';

const withTiles = (...ids: string[]) => ids.reduce((w, id) => addTile(w, id), emptyWindow('w1'));

describe('collapse and expand', () => {
  it('moves a tile into the dock and back to its place', () => {
    const win = withTiles('a', 'b', 'c');
    const collapsed = applyLayoutAction(win, { type: 'tile/collapse', sessionId: 'b' });
    expect(leaves(collapsed.layout)).toEqual(['a', 'c']);
    expect(collapsed.dock.map((d) => d.sessionId)).toEqual(['b']);
    expect(windowHasSession(collapsed, 'b')).toBe(true);

    const expanded = applyLayoutAction(collapsed, { type: 'tile/expand', sessionId: 'b' });
    expect(leaves(expanded.layout)).toEqual(['a', 'b', 'c']);
    expect(expanded.dock).toEqual([]);
  });

  it('ignores unknown sessions', () => {
    const win = withTiles('a');
    expect(applyLayoutAction(win, { type: 'tile/collapse', sessionId: 'x' })).toBe(win);
    expect(applyLayoutAction(win, { type: 'tile/expand', sessionId: 'x' })).toBe(win);
  });
});

describe('dropSession', () => {
  it('removes a session from layout and dock', () => {
    let win = withTiles('a', 'b');
    win = applyLayoutAction(win, { type: 'tile/collapse', sessionId: 'b' });
    win = dropSession(dropSession(win, 'b'), 'a');
    expect(win.layout).toBeNull();
    expect(win.dock).toEqual([]);
  });
});

describe('isWindowEmpty', () => {
  it('counts collapsed tiles as content', () => {
    expect(isWindowEmpty(emptyWindow('w'))).toBe(true);
    const collapsed = applyLayoutAction(withTiles('a'), { type: 'tile/collapse', sessionId: 'a' });
    expect(collapsed.layout).toBeNull();
    expect(isWindowEmpty(collapsed)).toBe(false);
    expect(isWindowEmpty(dropSession(collapsed, 'a'))).toBe(true);
  });
});

describe('isDropTarget', () => {
  it('accepts a node key with a side', () => {
    expect(isDropTarget({ key: tileKey('a'), side: 'bottom' })).toBe(true);
    expect(isDropTarget({ key: 1, side: 'bottom' })).toBe(false);
    expect(isDropTarget(null)).toBe(false);
  });
});

describe('isLayoutAction', () => {
  it('accepts well-formed actions only', () => {
    expect(isLayoutAction({ type: 'tile/move', sessionId: 'a', target: { key: tileKey('b'), side: 'left' } })).toBe(
      true,
    );
    expect(isLayoutAction({ type: 'tile/move', sessionId: 'a', target: { key: 'x', side: 'middle' } })).toBe(false);
    expect(isLayoutAction({ type: 'split/resize', splitId: 's', sizes: [50, '50'] })).toBe(false);
    expect(isLayoutAction({ type: 'tile/delete', sessionId: 'a' })).toBe(false);
    expect(isLayoutAction(null)).toBe(false);
  });
});
