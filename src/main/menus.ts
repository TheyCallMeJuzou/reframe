// Native context menus for tiles, groups and the session list. They only describe the choices; the actions live in the app.
import { Menu, type MenuItemConstructorOptions } from 'electron';
import { GROUP_COLORS } from '@shared/groups';
import type { MessageKey } from '@shared/i18n';
import type { GroupNode } from '@shared/layout';
import { swatch } from './images';

type Translate = (key: MessageKey, params?: Record<string, string | number>) => string;

export interface TileMenuOptions {
  rename: () => void;
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
    { label: t('session.rename'), click: o.rename },
    { type: 'separator' },
    { label: t('menu.newGroup'), enabled: !o.group, click: o.newGroup },
    {
      label: t('menu.addToGroup'),
      enabled: o.otherGroups.length > 0,
      submenu: o.otherGroups.map((g) => ({ label: g.name, icon: swatch(g.color), click: () => o.addToGroup(g.id) })),
    },
    { label: t('menu.removeFromGroup'), enabled: !!o.group, click: o.removeFromGroup },
  ]);
}

export interface ListMenuOptions {
  /** How many entries are selected. */
  count: number;
  open: () => void;
  group: () => void;
  groupInNewWindow: () => void;
  /** For a single session reframe manages. */
  rename?: () => void;
}

export function listMenu(t: Translate, o: ListMenuOptions): Menu {
  return Menu.buildFromTemplate([
    { label: o.count > 1 ? t('listMenu.openMany', { count: o.count }) : t('listMenu.open'), click: o.open },
    ...(o.rename ? [{ label: t('session.rename'), click: o.rename }] : []),
    { type: 'separator' },
    { label: t('listMenu.group'), click: o.group },
    { label: t('listMenu.groupInNewWindow'), click: o.groupInNewWindow },
  ]);
}

export interface WorkspaceMenuOptions {
  /** The workspace this window was saved as or opened from. */
  current: { name: string } | null;
  workspaces: { id: string; name: string; open: boolean }[];
  save: () => void;
  update: () => void;
  open: (workspaceId: string) => void;
}

export function workspaceMenu(t: Translate, o: WorkspaceMenuOptions): Menu {
  return Menu.buildFromTemplate([
    { label: t('workspace.save'), click: o.save },
    ...(o.current ? [{ label: t('workspace.update', { name: o.current.name }), click: o.update }] : []),
    { type: 'separator' },
    ...workspaceItems(t, o.workspaces, o.open),
  ]);
}

/** Saved workspaces to open (checked: its window is open). */
export function workspaceItems(
  t: Translate,
  workspaces: { id: string; name: string; open: boolean }[],
  open: (workspaceId: string) => void,
): MenuItemConstructorOptions[] {
  if (workspaces.length === 0) return [{ label: t('workspace.none'), enabled: false }];
  return workspaces.map((w) => ({ label: workspaceLabel(t, w), click: () => open(w.id) }));
}

/** A workspace in a menu: its name, and whether its window is open. */
export function workspaceLabel(t: Translate, w: { name: string; open: boolean }): string {
  return w.open ? `${w.name} ${t('workspace.openMark')}` : w.name;
}

export function workspaceItemMenu(t: Translate, o: { open: () => void; rename: () => void; remove: () => void }): Menu {
  return Menu.buildFromTemplate([
    { label: t('workspace.open'), click: o.open },
    { label: t('workspace.rename'), click: o.rename },
    { type: 'separator' },
    { label: t('workspace.delete'), click: o.remove },
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
