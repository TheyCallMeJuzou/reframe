import { createContext, useContext } from 'react';
import { translate, type Locale, type MessageKey } from '@shared/i18n';
import type { SessionView } from '@shared/ipc';
import type { TerminalRegistry } from './terminals';

export type Translate = (key: MessageKey, params?: Record<string, string | number>) => string;

/** A tile drag over this window; sessionId is null when the tile comes from another window. */
export interface DragState {
  sessionId: string | null;
}

export interface AppContextValue {
  t: Translate;
  sessions: Record<string, SessionView>;
  terminals: TerminalRegistry;
  dragging: DragState | null;
  setDragging: (drag: DragState | null) => void;
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
