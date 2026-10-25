import { EventEmitter } from 'node:events';
import { describe, expect, it, vi } from 'vitest';
import { Updates, type Updater } from './updates';

/** Stands in for electron-updater's updater: events by hand, checks that find nothing. */
function fakeUpdater() {
  const events = new EventEmitter();
  const updater = Object.assign(events, {
    autoDownload: true,
    autoInstallOnAppQuit: true,
    logger: null,
    checkForUpdates: vi.fn(async () => null),
  });
  return updater as typeof updater & Updater;
}

describe('Updates', () => {
  it('downloads and installs on quit in an installed copy', () => {
    const updater = fakeUpdater();
    const updates = new Updates(() => {});
    updates.watch(updater);
    expect(updater.autoDownload).toBe(true);
    expect(updater.autoInstallOnAppQuit).toBe(true);
    updater.emit('update-available', { version: '0.6.0' });
    expect(updates.available).toBeNull();
    updater.emit('update-downloaded', { version: '0.6.0' });
    expect(updates.ready).toBe('0.6.0');
  });

  it('only tells about a newer version in a portable copy, once', () => {
    const updater = fakeUpdater();
    const updates = new Updates(() => {}, true);
    const told: string[] = [];
    updates.on('available', (version) => told.push(version));
    updates.watch(updater);
    expect(updater.autoDownload).toBe(false);
    expect(updater.autoInstallOnAppQuit).toBe(false);
    updater.emit('update-available', { version: '0.6.0' });
    updater.emit('update-available', { version: '0.6.0' });
    expect(updates.available).toBe('0.6.0');
    expect(told).toEqual(['0.6.0']);
  });
});
