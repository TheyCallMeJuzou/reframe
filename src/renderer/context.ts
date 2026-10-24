import { createContext, useContext } from 'react';
import { translate, type Locale, type MessageKey } from '@shared/i18n';
import type { SessionView } from '@shared/ipc';
import type { DragKind } from './dnd';
import type { TerminalRegistry } from './terminals';

export type Translate = (key: MessageKey, params?: Record<string, string | number>) => string;

/** A drag over this window; key is null when the tile or group comes from another window. */
export interface DragState {
  key: string | null;
  kind: DragKind;
}

export interface AppContextValue {
  t: Translate;
  locale: Locale;
  windowId: string;
  sessions: Record<string, SessionView>;
  terminals: TerminalRegistry;
  dragging: DragState | null;
  setDragging: (drag: DragState | null) => void;
  /** Group whose name is being edited (started by the group menu's "Rename"). */
  renaming: string | null;
  setRenaming: (groupId: string | null) => void;
}

export const AppContext = createContext<AppContextValue | null>(null);

export function useApp(): AppContextValue {
  const value = useContext(AppContext);
  if (!value) throw new Error('AppContext missing');
  return value;
}

export function makeTranslate(locale: Locale): Translate {
  return (key, params) => translate(locale, key, params);
}
