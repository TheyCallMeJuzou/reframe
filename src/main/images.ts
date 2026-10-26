// Small images drawn in code: color swatches for menus, and the badge on a window's taskbar button.
import { nativeImage, type NativeImage } from 'electron';

const swatches = new Map<string, NativeImage>();

/** A small filled circle in the given color, drawn once per color. */
export function swatch(hex: string): NativeImage {
  const cached = swatches.get(hex);
  if (cached) return cached;
  const size = 16;
  const [r, g, b] = [1, 3, 5].map((i) => parseInt(hex.slice(i, i + 2), 16)) as [number, number, number];
  const pixels = Buffer.alloc(size * size * 4);
  for (let y = 0; y < size; y++) {
    for (let x = 0; x < size; x++) {
      const distance = Math.hypot(x + 0.5 - size / 2, y + 0.5 - size / 2);
      const alpha = Math.max(0, Math.min(1, 6.5 - distance)); // radius 6 with a soft edge
      const i = (y * size + x) * 4;
      // Bitmaps for nativeImage are BGRA with premultiplied alpha.
      pixels[i] = Math.round(b * alpha);
      pixels[i + 1] = Math.round(g * alpha);
      pixels[i + 2] = Math.round(r * alpha);
      pixels[i + 3] = Math.round(255 * alpha);
    }
  }
  const image = nativeImage.createFromBitmap(pixels, { width: size, height: size });
  swatches.set(hex, image);
  return image;
}

/** The badge on the taskbar button of a window with a session that needs the user: the color of its status dot. */
export const attentionBadge = (): NativeImage => swatch('#e0b04a');
