// Turns window captures into the README's pictures: windows with a frame and a shadow on a calm background, the
// social preview card, and GIFs from series of frames. The HTML is rendered in a page of the running app.
import fs from 'node:fs';
import path from 'node:path';
import type { CDPSession, ElectronApplication, Page } from '@playwright/test';
import { applyPalette, GIFEncoder, quantize } from 'gifenc';
import { PNG } from 'pngjs';
import { root } from '../e2e/helpers';

/** A capture of a window's content, `width` × `height` CSS pixels. */
export interface Shot {
  png: Buffer;
  width: number;
  height: number;
}

/** A window on a picture: where it sits, and how wide it is drawn (default: its own width). */
export interface Placed {
  shot: Shot;
  x: number;
  y: number;
  width?: number;
}

export interface Frame {
  png: Buffer;
  /** How long the frame shows, in milliseconds. */
  delay: number;
}

const TITLE_BAR = 32;
const dataUrl = (type: string, data: Buffer) => `data:${type};base64,${data.toString('base64')}`;
const ICON = dataUrl('image/svg+xml', fs.readFileSync(path.join(root, 'docs', 'logo', 'reframe-icon.svg')));

const STYLE = `
* { box-sizing: border-box; }
html, body { margin: 0; width: 100%; height: 100%; overflow: hidden; }
body {
  position: relative;
  font-family: 'Segoe UI Variable Text', 'Segoe UI', system-ui, sans-serif;
  background:
    radial-gradient(80% 110% at 0% 0%, #4a3f6e 0%, transparent 70%),
    radial-gradient(80% 110% at 100% 100%, #8a4a37 0%, transparent 70%),
    #2a2832;
}
.window {
  position: absolute;
  border-radius: 9px;
  overflow: hidden;
  background: #1b1c20;
  box-shadow: 0 0 0 1px rgb(255 255 255 / 12%), 0 24px 64px rgb(0 0 0 / 55%), 0 6px 18px rgb(0 0 0 / 35%);
}
.titlebar {
  display: flex;
  align-items: center;
  gap: 9px;
  height: ${TITLE_BAR}px;
  padding-left: 12px;
  background: #202127;
  color: #c9ccd3;
  font-size: 12px;
}
.titlebar img { width: 16px; height: 16px; }
.controls { display: flex; margin-left: auto; }
.content { display: block; }
.brand {
  position: absolute;
  left: 76px;
  top: 0;
  bottom: 0;
  width: 470px;
  display: flex;
  flex-direction: column;
  justify-content: center;
  color: #f3f4f7;
}
.brand img { width: 92px; height: 92px; }
.brand h1 { margin: 22px 0 10px; font-size: 80px; font-weight: 700; letter-spacing: -0.02em; line-height: 1; }
.brand p { margin: 0; font-size: 26px; line-height: 1.4; color: #d9dbe2; }
`;

// Windows 11's minimize, maximize and close buttons.
const CONTROLS = ['M18 16.5h10', 'M18.5 11.5h9v9h-9z', 'M18 11l10 10M28 11L18 21']
  .map(
    (d) =>
      `<svg width="46" height="${TITLE_BAR}" viewBox="0 0 46 32"><path d="${d}" fill="none" stroke="#c9ccd3" stroke-width="1"/></svg>`,
  )
  .join('');

function windowHtml({ shot, x, y, width = shot.width }: Placed): string {
  const height = (shot.height * width) / shot.width;
  return `<div class="window" style="left:${x}px;top:${y}px;width:${width}px">
    <div class="titlebar"><img src="${ICON}" alt=""><span>reframe</span><span class="controls">${CONTROLS}</span></div>
    <img class="content" src="${dataUrl('image/png', shot.png)}" style="width:${width}px;height:${height}px" alt="">
  </div>`;
}

/** The outer size of a window as windowHtml draws it. */
export const windowSize = (shot: Shot) => ({ width: shot.width, height: shot.height + TITLE_BAR });

export class Composer {
  private constructor(
    private page: Page,
    private cdp: CDPSession,
  ) {}

  /** A hidden page of the app to render on. */
  static async open(app: ElectronApplication): Promise<Composer> {
    const opened = app.waitForEvent('window');
    await app.evaluate(({ BrowserWindow }) => {
      const win = new BrowserWindow({ show: false, width: 800, height: 600 });
      void win.loadURL('about:blank');
    });
    const page = await opened;
    return new Composer(page, await page.context().newCDPSession(page));
  }

  private async render(body: string, width: number, height: number, scale: number): Promise<Buffer> {
    await this.cdp.send('Emulation.setDeviceMetricsOverride', {
      width,
      height,
      deviceScaleFactor: scale,
      mobile: false,
    });
    await this.page.setContent(`<!doctype html><html><head><style>${STYLE}</style></head><body>${body}</body></html>`);
    await this.page.evaluate(() => Promise.all([...document.images].map((img) => img.decode())));
    // At the emulated scale; Playwright's screenshots come at the screen's.
    const { data } = await this.cdp.send('Page.captureScreenshot', { format: 'png' });
    return Buffer.from(data, 'base64');
  }

  /** Windows on the background (later ones on top), the picture `width` × `height` CSS pixels at `scale`. */
  desktop(windows: Placed[], width: number, height: number, scale: number): Promise<Buffer> {
    return this.render(windows.map(windowHtml).join(''), width, height, scale);
  }

  /** One window with `margin` CSS pixels of background around it. */
  framed(shot: Shot, scale: number, margin = 48): Promise<Buffer> {
    const size = windowSize(shot);
    return this.desktop([{ shot, x: margin, y: margin }], size.width + 2 * margin, size.height + 2 * margin, scale);
  }

  /** Each frame framed like framed(), keeping its delay. */
  async framedSeries(frames: { shot: Shot; delay: number }[], scale: number, margin = 32): Promise<Frame[]> {
    const out: Frame[] = [];
    for (const { shot, delay } of frames) out.push({ png: await this.framed(shot, scale, margin), delay });
    return out;
  }

  /** GitHub's social preview (1280 × 640): name and tagline, and part of `hero` beside them. */
  socialPreview(hero: Shot): Promise<Buffer> {
    const brand = `<div class="brand">
      <img src="${ICON}" alt="">
      <h1>reframe</h1>
      <p>All your Claude Code sessions side by side. Group them, tear them out, get them back after a restart.</p>
    </div>`;
    return this.render(brand + windowHtml({ shot: hero, x: 600, y: 96, width: 980 }), 1280, 640, 1);
  }
}

/**
 * A looping GIF. All frames share one palette, so a frame only carries the pixels that changed (the others are
 * transparent and show the frame before), and frames that look like the one before just make it last longer.
 */
export function encodeGif(frames: Frame[]): Buffer {
  const images = frames.map((frame) => PNG.sync.read(frame.png));
  const { width, height } = images[0]!;
  const step = Math.max(1, Math.floor(images.length / 8));
  const sample = images.filter((_, i) => i % step === 0 || i === images.length - 1);
  const pixels = new Uint8Array(sample.reduce((n, image) => n + image.data.length, 0));
  sample.reduce((offset, image) => (pixels.set(image.data, offset), offset + image.data.length), 0);
  const colors = quantize(pixels, 255);
  const TRANSPARENT = 255;
  const palette = [...colors, ...Array.from({ length: 256 - colors.length }, () => [0, 0, 0])];

  const gif = GIFEncoder();
  let previous: Uint8Array | null = null;
  let queued: { index: Uint8Array; delay: number } | null = null;
  const write = ({ index, delay }: { index: Uint8Array; delay: number }, first: boolean) =>
    gif.writeFrame(index, width, height, {
      ...(first ? { palette, repeat: 0 } : { transparent: true, transparentIndex: TRANSPARENT }),
      delay,
      dispose: 1,
    });
  let written = 0;
  for (const [i, image] of images.entries()) {
    const index = applyPalette(image.data, colors);
    const delay = frames[i]!.delay;
    const before = previous;
    if (before && queued && index.every((value, p) => value === before[p])) {
      queued.delay += delay;
      continue;
    }
    if (queued) write(queued, written++ === 0);
    queued = { index: before ? index.map((value, p) => (value === before[p] ? TRANSPARENT : value)) : index, delay };
    previous = index;
  }
  if (queued) write(queued, written === 0);
  gif.finish();
  return Buffer.from(gif.bytes());
}
