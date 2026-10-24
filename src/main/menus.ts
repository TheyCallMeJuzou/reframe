// Native context menus for tiles and groups. They only describe the choices; the actions live in the app.
import { Menu, nativeImage, type MenuItemConstructorOptions, type NativeImage } from 'electron';
import { GROUP_COLORS } from '@shared/groups';
import type { MessageKey } from '@shared/i18n';
import type { GroupNode } from '@shared/layout';

type Translate = (key: MessageKey, params?: Record<string, string | number>) => string;

export interface TileMenuOptions {
  /** The group the tile is in, if any. */
  group: GroupNode | null;
  /** Visible groups in the same window the tile could join. */
  otherGroups: GroupNode[];
  newGroup: () => void;
  addToGroup: (groupId: string) => void;
  removeFromGroup: () => void;
}

export function tileMenu(t: Translate, o: TileMenuOptions): Menu {
  return Menu.buildFromTemplate([
    { label: t('menu.newGroup'), enabled: !o.group, click: o.newGroup },
    {
      label: t('menu.addToGroup'),
      enabled: o.otherGroups.length > 0,
      submenu: o.otherGroups.map((g) => ({ label: g.name, icon: swatch(g.color), click: () => o.addToGroup(g.id) })),
    },
    { label: t('menu.removeFromGroup'), enabled: !!o.group, click: o.removeFromGroup },
  ]);
}

export interface GroupMenuOptions {
  rename: () => void;
  recolor: (hex: string) => void;
  dissolve: () => void;
  endAll: () => void;
}

export function groupMenu(t: Translate, group: GroupNode, o: GroupMenuOptions): Menu {
  return Menu.buildFromTemplate([
    { label: t('group.rename'), click: o.rename },
    { label: t('group.color'), submenu: colorItems(t, group, o.recolor) },
    { type: 'separator' },
    { label: t('group.dissolve'), click: o.dissolve },
    { label: t('group.endAll'), click: o.endAll },
  ]);
}

export function colorMenu(t: Translate, group: GroupNode, recolor: (hex: string) => void): Menu {
  return Menu.buildFromTemplate(colorItems(t, group, recolor));
}

function colorItems(t: Translate, group: GroupNode, recolor: (hex: string) => void): MenuItemConstructorOptions[] {
  return GROUP_COLORS.map((c) => ({
    label: t(c.label),
    icon: swatch(c.hex),
    type: 'checkbox',
    checked: c.hex === group.color,
    click: () => recolor(c.hex),
  }));
}

const swatches = new Map<string, NativeImage>();

/** A small filled circle in the given color, drawn once per color. */
function swatch(hex: string): NativeImage {
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
