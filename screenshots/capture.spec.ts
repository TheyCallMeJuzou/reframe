// Takes the README's screenshots and GIFs into docs/screenshots (npm run screenshots; not part of the tests).
// reframe runs in a throwaway profile and home folder with demo sessions (demo-claude.mjs draws what scenes.mjs
// has for each name), laid out through its saved state. Each window's page is rendered at a fixed size, and
// drags, drops and pastes go through the same events as real ones; a pointer, a dragged file and pressed keys are
// drawn over the page for the GIFs. compose.ts frames the captures.
import { spawn, type ChildProcess } from 'node:child_process';
import crypto from 'node:crypto';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { expect, test, type ElectronApplication, type Locator, type Page } from '@playwright/test';
import type { GroupNode, LayoutNode } from '../src/shared/layout';
import type { AppState, SessionRecord, WindowState, Workspace } from '../src/shared/state';
import { callMain, cleanup, launch, pageOf, quit, root, tileByLabel } from '../e2e/helpers';
import { Composer, encodeGif, windowSize, type Shot } from './compose';

const out = path.join(root, 'docs', 'screenshots');
const HOUR = 3600_000;
const BLUE = '#5b9cf0';
const GREEN = '#4fbf7f';

/** A throwaway home folder with demo projects, Claude Code's files for them, and reframe's profile. */
class Demo {
  readonly dir = fs.mkdtempSync(path.join(os.tmpdir(), 'reframe-shots-'));
  private home = path.join(this.dir, 'home');
  private claudeHome = path.join(this.dir, 'claude-home');
  private userData = path.join(this.dir, 'user-data');
  private inApp: SessionRecord[] = [];
  private processes: ChildProcess[] = [];

  readonly env: Record<string, string> = {
    ...(process.env as Record<string, string>),
    REFRAME_E2E: '1',
    REFRAME_USER_DATA: this.userData,
    REFRAME_CLAUDE_HOME: this.claudeHome,
    REFRAME_CLAUDE_BIN: path.join(root, 'screenshots', 'demo-claude.mjs'),
    REFRAME_NODE: process.execPath,
    REFRAME_LOCALE: 'en',
    REFRAME_DISABLE_NOTIFICATIONS: '1',
    // Folders show from "~", so no real user name turns up.
    USERPROFILE: this.home,
    HOME: this.home,
  };

  private folder(project: string): string {
    const folder = path.join(this.home, 'projects', project);
    fs.mkdirSync(folder, { recursive: true });
    return folder;
  }

  private transcript(claudeSessionId: string, cwd: string, prompt: string, modifiedAt: number): void {
    const file = path.join(this.claudeHome, 'projects', path.basename(cwd), `${claudeSessionId}.jsonl`);
    fs.mkdirSync(path.dirname(file), { recursive: true });
    fs.writeFileSync(file, JSON.stringify({ type: 'user', cwd, message: { content: prompt } }) + '\n');
    fs.utimesSync(file, new Date(modifiedAt), new Date(modifiedAt));
  }

  /** A session (the scene of its name); `inApp`: open in reframe, otherwise only kept in a workspace. */
  session(name: string, project: string, inApp = true): SessionRecord {
    const cwd = this.folder(project);
    const record: SessionRecord = {
      id: crypto.randomUUID(),
      claudeSessionId: crypto.randomUUID(),
      cwd,
      name,
      launch: {},
      lifecycle: 'open',
      createdAt: Date.now() - 5 * HOUR,
      lastActiveAt: Date.now(),
      used: true,
    };
    if (inApp) {
      this.transcript(record.claudeSessionId, cwd, name, Date.now() - HOUR);
      this.inApp.push(record);
    }
    return record;
  }

  /** A past session, listed among the recent ones. */
  past(title: string, project: string, hoursAgo: number): void {
    this.transcript(crypto.randomUUID(), this.folder(project), title, Date.now() - hoursAgo * HOUR);
  }

  /** A session running in another terminal, as Claude Code registers it. */
  external(name: string, project: string): void {
    const child = spawn(process.execPath, ['-e', 'setInterval(() => {}, 1e6)'], { stdio: 'ignore' });
    this.processes.push(child);
    const entry = {
      pid: child.pid,
      sessionId: crypto.randomUUID(),
      cwd: this.folder(project),
      name,
      nameSource: 'user',
      status: 'busy',
    };
    fs.mkdirSync(path.join(this.claudeHome, 'sessions'), { recursive: true });
    fs.writeFileSync(path.join(this.claudeHome, 'sessions', `${child.pid}.json`), JSON.stringify(entry));
  }

  write(windows: WindowState[], workspaces: Workspace[] = []): void {
    const state: AppState = {
      version: 1,
      sessions: Object.fromEntries(this.inApp.map((record) => [record.id, record])),
      windows,
      recentFolders: [],
      preferences: { nameFromFolder: true, language: 'en' },
      workspaces,
    };
    fs.mkdirSync(this.userData, { recursive: true });
    fs.writeFileSync(path.join(this.userData, 'state.json'), JSON.stringify(state, null, 2));
  }

  async close(): Promise<void> {
    for (const child of this.processes) child.kill();
    await cleanup(this.dir);
  }
}

// ---------- layouts ----------

const tile = (record: SessionRecord): LayoutNode => ({ kind: 'tile', sessionId: record.id });
const split = (dir: 'row' | 'col', children: LayoutNode[], sizes?: number[]): LayoutNode => ({
  kind: 'split',
  id: crypto.randomUUID(),
  dir,
  children,
  sizes: sizes ?? children.map(() => 100 / children.length),
});
const row = (children: LayoutNode[], sizes?: number[]) => split('row', children, sizes);
const col = (children: LayoutNode[], sizes?: number[]) => split('col', children, sizes);
const group = (name: string, color: string, child: LayoutNode): GroupNode => ({
  kind: 'group',
  id: crypto.randomUUID(),
  name,
  color,
  child,
});

function windowState(
  layout: LayoutNode,
  options: { dock?: SessionRecord[]; sidebarOpen?: boolean; workspace?: Workspace } = {},
): WindowState {
  return {
    id: crypto.randomUUID(),
    bounds: { x: 60, y: 60, width: 1280, height: 800 },
    maximized: false,
    hidden: false,
    layout,
    dock: (options.dock ?? []).map((record) => ({ kind: 'tile' as const, sessionId: record.id, hint: null })),
    sidebarOpen: options.sidebarOpen,
    workspaceId: options.workspace?.id,
  };
}

/** A workspace saved `hoursAgo` from `win` (its sessions: `sessions`). */
function workspace(name: string, hoursAgo: number, sessions: SessionRecord[], win?: WindowState): Workspace {
  return {
    id: crypto.randomUUID(),
    name,
    savedAt: Date.now() - hoursAgo * HOUR,
    window: {
      bounds: win?.bounds ?? null,
      maximized: false,
      sidebarOpen: win?.sidebarOpen,
      layout: win?.layout ?? row(sessions.map(tile)),
      dock: win?.dock ?? [],
    },
    sessions,
  };
}

// ---------- capturing ----------

type Camera = () => Promise<Shot>;

/** reframe with the demo, its pixels `scale` times the CSS pixels (a real scale: terminals draw wrong at an emulated one). */
const start = (demo: Demo, scale: number) => launch(demo.env, [`--force-device-scale-factor=${scale}`]);

/**
 * Renders the page at `width` × `height` CSS pixels, whatever size its window has on screen, and returns what
 * captures it at the app's scale (Playwright's screenshots come at the screen's).
 */
async function emulate(page: Page, width: number, height: number): Promise<Camera> {
  const cdp = await page.context().newCDPSession(page);
  await cdp.send('Emulation.setDeviceMetricsOverride', { width, height, deviceScaleFactor: 0, mobile: false });
  return async () => {
    const { data } = await cdp.send('Page.captureScreenshot', { format: 'png' });
    return { png: Buffer.from(data, 'base64'), width, height };
  };
}

/** Waits until all tiles are there and running, and the sessions have drawn at their size. */
async function settle(page: Page, tiles: number): Promise<void> {
  await expect(page.locator('.tile')).toHaveCount(tiles);
  await expect(page.locator('.tile .status-starting')).toHaveCount(0, { timeout: 20_000 });
  await page.waitForTimeout(2000);
}

function save(name: string, data: Buffer): void {
  fs.mkdirSync(out, { recursive: true });
  fs.writeFileSync(path.join(out, name), data);
  console.log(`${name}: ${Math.round(data.length / 1024)} KB`);
}

async function center(locator: Locator): Promise<{ x: number; y: number }> {
  const box = (await locator.boundingBox())!;
  return { x: box.x + box.width / 2, y: box.y + box.height / 2 };
}

/** The pointer, a dragged file or tile, and pressed keys, drawn over the page (they take no events). */
async function addOverlay(page: Page): Promise<void> {
  await page.evaluate(() => {
    const style = document.createElement('style');
    style.textContent = `
      .demo-overlay { position: fixed; z-index: 100000; pointer-events: none; }
      #demo-pointer { left: 0; top: 0; width: 22px; height: 26px; display: none; }
      #demo-ghost {
        display: none; align-items: center; gap: 8px; padding: 6px 10px 6px 6px; border-radius: 6px;
        background: rgb(42 45 52 / 92%); border: 1px solid #454a55; color: #d7dae0;
        font: 12px system-ui, 'Segoe UI', sans-serif; box-shadow: 0 6px 18px rgb(0 0 0 / 40%);
      }
      #demo-ghost .thumb {
        width: 34px; height: 42px; border-radius: 3px; border: 1px solid #5a606c;
        background: linear-gradient(#e9ebef 0 22%, #d97757 22% 30%, #f6f7f9 30% 100%);
      }
      #demo-keys {
        display: none; bottom: 26px; transform: translateX(-50%); gap: 6px; align-items: center;
        color: #d7dae0; font: 600 15px system-ui, 'Segoe UI', sans-serif;
      }
      #demo-keys kbd {
        padding: 6px 12px; border-radius: 6px; background: #2a2d34; border: 1px solid #5a606c;
        border-bottom-width: 3px; font: inherit; box-shadow: 0 6px 18px rgb(0 0 0 / 45%);
      }`;
    document.head.appendChild(style);
    const add = (id: string, html: string) => {
      const el = document.createElement('div');
      el.id = id;
      el.className = 'demo-overlay';
      el.innerHTML = html;
      document.body.appendChild(el);
    };
    add(
      'demo-pointer',
      '<svg width="22" height="26" viewBox="0 0 22 26"><path d="M2 1.5v19.5l5.2-5 3.4 7.8 3.4-1.5-3.3-7.6h7.2z" fill="#fff" stroke="#000" stroke-width="1.3" stroke-linejoin="round"/></svg>',
    );
    add('demo-ghost', '');
    add('demo-keys', '');
  });
}

/** Moves the drawn pointer (and what it drags: a tile's name, a file, or nothing); null hides it, as typing does. */
async function pointer(
  page: Page,
  at: { x: number; y: number } | null,
  carrying: { tile: string } | { file: string } | null = null,
): Promise<void> {
  await page.evaluate(
    ({ at, carrying }) => {
      const arrow = document.getElementById('demo-pointer')!;
      const ghost = document.getElementById('demo-ghost')!;
      arrow.style.display = at ? 'block' : 'none';
      if (at) Object.assign(arrow.style, { left: `${at.x}px`, top: `${at.y}px` });
      if (!at || !carrying) return void (ghost.style.display = 'none');
      ghost.innerHTML =
        'file' in carrying
          ? `<span class="thumb"></span><span>${carrying.file}</span>`
          : `<span style="padding-left:4px">${carrying.tile}</span>`;
      Object.assign(ghost.style, { display: 'flex', left: `${at.x + 14}px`, top: `${at.y + 18}px` });
    },
    { at, carrying },
  );
}

/** Shows keys being pressed, centered at `x` near the bottom; null hides them. */
async function keys(page: Page, combo: string[] | null, x = 0): Promise<void> {
  await page.evaluate(
    ({ combo, x }) => {
      const el = document.getElementById('demo-keys')!;
      el.innerHTML = (combo ?? []).map((k) => `<kbd>${k}</kbd>`).join('<span>+</span>');
      Object.assign(el.style, { display: combo ? 'flex' : 'none', left: `${x}px` });
    },
    { combo, x },
  );
}

type DragKind = 'tile' | 'files';

/** Drag events as a real drag sends them, at a point: a tile's drag starts on its header there; files come in. */
async function drag(page: Page, step: 'start' | 'over' | 'drop', kind: DragKind, at: { x: number; y: number }) {
  await page.evaluate(
    ({ step, kind, at }) => {
      type DemoDrag = { data: DataTransfer; source: Element | null; over: Element | null };
      const w = window as unknown as { demoDrag?: DemoDrag };
      const init = (data: DataTransfer) => ({
        bubbles: true,
        cancelable: true,
        dataTransfer: data,
        clientX: at.x,
        clientY: at.y,
      });
      if (step === 'start') {
        const data = new DataTransfer();
        let source: Element | null = null;
        if (kind === 'files') {
          const png = new Uint8Array([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]);
          data.items.add(new File([png], 'checkout-on-phone.png', { type: 'image/png' }));
        } else {
          source = document.elementFromPoint(at.x, at.y)!.closest('.tile-header');
          source!.dispatchEvent(new DragEvent('dragstart', init(data)));
        }
        w.demoDrag = { data, source, over: null };
        return;
      }
      const drag = w.demoDrag!;
      const target = document.elementFromPoint(at.x, at.y);
      if (target !== drag.over) {
        drag.over?.dispatchEvent(new DragEvent('dragleave', { ...init(drag.data), relatedTarget: target }));
        target?.dispatchEvent(new DragEvent('dragenter', init(drag.data)));
        drag.over = target;
      }
      target?.dispatchEvent(new DragEvent(step === 'over' ? 'dragover' : 'drop', init(drag.data)));
      if (step === 'drop') {
        // Landed, so no tearing out. A made-up DataTransfer ignores setting dropEffect, so it is defined over.
        Object.defineProperty(drag.data, 'dropEffect', { value: 'move' });
        drag.source?.dispatchEvent(new DragEvent('dragend', init(drag.data)));
        delete w.demoDrag;
      }
    },
    { step, kind, at },
  );
}

/** Points from `from` to `to` in `steps`, easing in and out. */
function glide(from: { x: number; y: number }, to: { x: number; y: number }, steps: number) {
  return Array.from({ length: steps }, (_, i) => {
    const t = (i + 1) / steps;
    const eased = t < 0.5 ? 2 * t * t : 1 - (-2 * t + 2) ** 2 / 2;
    return { x: from.x + (to.x - from.x) * eased, y: from.y + (to.y - from.y) * eased };
  });
}

/** Collects GIF frames: each capture shows until the next one. */
class Film {
  readonly frames: { shot: Shot; delay: number }[] = [];
  constructor(
    private page: Page,
    private camera: Camera,
  ) {}
  async take(delay: number): Promise<void> {
    this.frames.push({ shot: await this.camera(), delay });
  }
  /** Types `text` a few characters per frame. */
  async type(text: string): Promise<void> {
    for (let i = 0; i < text.length; i += 3) {
      await this.page.keyboard.type(text.slice(i, i + 3));
      await this.page.waitForTimeout(110);
      await this.take(90);
    }
  }
  /** Frames every `every` ms for `ms`, e.g. while Claude answers. */
  async watch(ms: number, every = 250): Promise<void> {
    for (let t = 0; t < ms; t += every) {
      await this.page.waitForTimeout(every - 60);
      await this.take(every);
    }
  }
}

/** The GIF; with SCREENSHOTS_FRAMES set to a folder, its frames also go there as PNGs (to look at them). */
async function composeGif(app: ElectronApplication, name: string, film: Film): Promise<void> {
  const composer = await Composer.open(app);
  const frames = await composer.framedSeries(film.frames, 1);
  const folder = process.env.SCREENSHOTS_FRAMES;
  if (folder) {
    fs.mkdirSync(folder, { recursive: true });
    frames.forEach((frame, i) =>
      fs.writeFileSync(path.join(folder, `${name}-${String(i).padStart(3, '0')}-${frame.delay}ms.png`), frame.png),
    );
  }
  save(name, encodeGif(frames));
}

const GIF_SIZE = { width: 960, height: 580 };

// ---------- the pictures ----------

test('hero: a workspace with a group of four, and the session list', async () => {
  const demo = new Demo();
  try {
    const checkout = demo.session('checkout-fix', 'webshop');
    const limiter = demo.session('rate-limiter', 'api-gateway');
    const migration = demo.session('db-migration', 'webshop');
    const webhooks = demo.session('payment-webhooks', 'api-gateway');
    const docs = demo.session('docs-refresh', 'docs-site');
    const crash = demo.session('ios-crash', 'mobile-app');
    const notes = demo.session('release-notes', 'mobile-app');
    demo.past('Add dark mode to the account settings', 'webshop', 3);
    demo.past('Bump dependencies and fix the deprecation warnings', 'api-gateway', 26);
    demo.past('Why is search slow for long queries?', 'webshop', 50);
    demo.past('Set up preview deployments for pull requests', 'docs-site', 98);
    demo.past('Translate the onboarding screens to German', 'mobile-app', 170);

    const backend = group(
      'Backend',
      BLUE,
      row([col([tile(checkout), tile(limiter)]), col([tile(migration), tile(webhooks)])]),
    );
    const layout = row([backend, col([tile(docs), tile(crash)])], [63, 37]);
    const webshop = workspace('Webshop', 2, [checkout, limiter, migration, webhooks, docs, crash, notes]);
    const main = windowState(layout, { dock: [notes], sidebarOpen: true, workspace: webshop });
    webshop.window = { ...webshop.window, layout, dock: main.dock, sidebarOpen: true };
    const mobile = workspace('Mobile release', 30, [
      demo.session('ios-crash', 'mobile-app', false),
      demo.session('android-push', 'mobile-app', false),
      demo.session('release-notes', 'mobile-app', false),
    ]);
    const website = workspace('Docs & website', 75, [
      demo.session('docs-refresh', 'docs-site', false),
      demo.session('landing-page', 'webshop', false),
    ]);
    demo.write([main], [webshop, mobile, website]);

    const { app, page } = await start(demo, 2);
    const size = { width: 1600, height: 1000 };
    const camera = await emulate(page, size.width, size.height);
    await settle(page, 6);
    const focused = await center(tileByLabel(page, 'checkout-fix').locator('.tile-body'));
    await page.mouse.click(focused.x, focused.y);
    await expect(page.locator('.sidebar-row').first()).toBeVisible();
    await page.waitForTimeout(3500); // the list has loaded once, so what turns up now is new
    demo.external('ci-pipeline', 'webshop');
    await expect(page.locator('.sidebar-row.is-new')).toBeVisible({ timeout: 10_000 });
    await page.waitForTimeout(450); // the highlight at its brightest
    const hero = await camera();

    const composer = await Composer.open(app);
    save('hero.png', await composer.framed(hero, 2));
    save('social-preview.png', await composer.socialPreview(hero));
    await quit(app);
  } finally {
    await demo.close();
  }
});

test('windows: two windows, each a workspace', async () => {
  const demo = new Demo();
  try {
    const checkout = demo.session('checkout-fix', 'webshop');
    const limiter = demo.session('rate-limiter', 'api-gateway');
    const migration = demo.session('db-migration', 'webshop');
    const docs = demo.session('docs-refresh', 'docs-site');
    const crash = demo.session('ios-crash', 'mobile-app');
    const push = demo.session('android-push', 'mobile-app');
    const notes = demo.session('release-notes', 'mobile-app');

    const first = windowState(
      row([group('Backend', BLUE, col([tile(checkout), tile(migration)])), col([tile(limiter), tile(docs)])], [55, 45]),
    );
    const second = windowState(row([group('Mobile', GREEN, col([tile(crash), tile(push)])), tile(notes)], [60, 40]));
    const webshop = workspace('Webshop', 2, [checkout, limiter, migration, docs], first);
    const mobile = workspace('Mobile release', 30, [crash, push, notes], second);
    first.workspaceId = webshop.id;
    second.workspaceId = mobile.id;
    demo.write([first, second], [webshop, mobile]);

    const { app } = await start(demo, 2);
    const pages = [await pageOf(app, first.id), await pageOf(app, second.id)];
    const sizes = [
      { width: 1280, height: 780 },
      { width: 900, height: 540 },
    ];
    const cameras: Camera[] = [];
    for (const [i, page] of pages.entries()) cameras.push(await emulate(page, sizes[i]!.width, sizes[i]!.height));
    await settle(pages[0]!, 4);
    await settle(pages[1]!, 3);
    const shots = [await cameras[0]!(), await cameras[1]!()];

    const margin = 48;
    const a = windowSize(shots[0]!);
    const b = windowSize(shots[1]!);
    const at = { x: margin + a.width - 380, y: margin + a.height - 300 };
    const composer = await Composer.open(app);
    save(
      'windows.png',
      await composer.desktop(
        [
          { shot: shots[0]!, x: margin, y: margin },
          { shot: shots[1]!, ...at },
        ],
        at.x + b.width + margin,
        at.y + b.height + margin,
        2,
      ),
    );
    await quit(app);
  } finally {
    await demo.close();
  }
});

test('drag-tile.gif: a tile dragged to another spot', async () => {
  const demo = new Demo();
  try {
    const checkout = demo.session('checkout-fix', 'webshop');
    const limiter = demo.session('rate-limiter', 'api-gateway');
    const docs = demo.session('docs-refresh', 'docs-site');
    demo.write([windowState(row([tile(checkout), col([tile(limiter), tile(docs)])], [52, 48]))]);

    const { app, page } = await start(demo, 1);
    const { width, height } = GIF_SIZE;
    const camera = await emulate(page, width, height);
    await settle(page, 3);
    await addOverlay(page);
    const film = new Film(page, camera);

    const grip = await center(tileByLabel(page, 'docs-refresh').locator('.tile-grip'));
    const target = (await tileByLabel(page, 'checkout-fix').boundingBox())!;
    let at = { x: grip.x + 90, y: grip.y + 120 };
    await pointer(page, at);
    await film.take(900);
    for (const point of glide(at, grip, 6)) {
      await pointer(page, point);
      await film.take(60);
    }
    at = grip;
    await drag(page, 'start', 'tile', at);
    await page.waitForTimeout(100);
    await film.take(300);
    const stops = [
      { x: target.x + target.width * 0.82, y: target.y + target.height * 0.5, hold: 700 },
      { x: target.x + target.width * 0.5, y: target.y + target.height * 0.86, hold: 1100 },
    ];
    for (const stop of stops) {
      for (const point of glide(at, stop, 10)) {
        await pointer(page, point, { tile: 'docs-refresh' });
        await drag(page, 'over', 'tile', point);
        await page.waitForTimeout(20);
        await film.take(60);
      }
      at = stop;
      await film.take(stop.hold);
    }
    await drag(page, 'drop', 'tile', at);
    await pointer(page, at);
    await page.waitForTimeout(1500); // the tiles take their new size, the sessions draw again
    await film.take(2600);

    await composeGif(app, 'drag-tile.gif', film);
    await quit(app);
  } finally {
    await demo.close();
  }
});

test('files.gif: a screenshot dropped onto a session', async () => {
  const demo = new Demo();
  try {
    const landing = demo.session('landing-page', 'webshop');
    const checkout = demo.session('checkout-fix', 'webshop');
    demo.write([windowState(row([tile(landing), tile(checkout)], [56, 44]))]);

    const { app, page } = await start(demo, 1);
    const { width, height } = GIF_SIZE;
    const camera = await emulate(page, width, height);
    await settle(page, 2);
    await addOverlay(page);
    const film = new Film(page, camera);

    const body = (await tileByLabel(page, 'landing-page').locator('.tile-body').boundingBox())!;
    let at = { x: body.x + 30, y: body.y + body.height - 30 };
    const file = { file: 'checkout-on-phone.png' };
    await pointer(page, at, file);
    await film.take(900);
    await drag(page, 'start', 'files', at);
    const over = { x: body.x + body.width * 0.3, y: body.y + body.height * 0.68 }; // the hint stays readable
    for (const point of glide(at, over, 12)) {
      await pointer(page, point, file);
      await drag(page, 'over', 'files', point);
      await film.take(60);
    }
    at = over;
    await film.take(900);
    await drag(page, 'drop', 'files', at);
    await pointer(page, at);
    await page.waitForTimeout(500);
    await film.take(900);
    await pointer(page, null);
    await film.type('the total slips under the Pay button');
    await film.take(500);
    await page.keyboard.press('Enter');
    await film.watch(3600);
    await expect(tileByLabel(page, 'landing-page').locator('.status-idle')).toBeVisible();
    await film.take(3000);

    await composeGif(app, 'files.gif', film);
    await quit(app);
  } finally {
    await demo.close();
  }
});

test('paste.gif: Ctrl+V pastes a screenshot', async () => {
  const demo = new Demo();
  try {
    const header = demo.session('mobile-header', 'webshop');
    const limiter = demo.session('rate-limiter', 'api-gateway');
    demo.write([windowState(row([tile(header), tile(limiter)], [56, 44]))]);

    const { app, page } = await start(demo, 1);
    const { width, height } = GIF_SIZE;
    const camera = await emulate(page, width, height);
    await settle(page, 2);
    await addOverlay(page);
    const film = new Film(page, camera);

    const body = (await tileByLabel(page, 'mobile-header').locator('.tile-body').boundingBox())!;
    const from = { x: body.x + body.width + 140, y: body.y + 90 };
    const into = { x: body.x + body.width * 0.55, y: body.y + body.height * 0.6 };
    await pointer(page, from);
    await film.take(800);
    for (const point of glide(from, into, 8)) {
      await pointer(page, point);
      await film.take(60);
    }
    await page.mouse.click(into.x, into.y);
    await page.waitForTimeout(150);
    await film.take(600);
    await callMain(app, 'fakeClipboard', { text: '', image: true, files: [] });
    await keys(page, ['Ctrl', 'V'], body.x + body.width / 2);
    await film.take(450);
    await page.keyboard.press('Control+V');
    await page.waitForTimeout(400);
    await film.take(700);
    await keys(page, null);
    await pointer(page, null);
    await film.type('the cart icon gets pushed out');
    await film.take(500);
    await page.keyboard.press('Enter');
    await film.watch(3600);
    await expect(tileByLabel(page, 'mobile-header').locator('.status-idle')).toBeVisible();
    await film.take(3000);
    await callMain(app, 'fakeClipboard', null);

    await composeGif(app, 'paste.gif', film);
    await quit(app);
  } finally {
    await demo.close();
  }
});
