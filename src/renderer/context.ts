import { createContext, useContext } from 'react';
import { translate, type Locale, type MessageKey } from '@shared/i18n';
import type { SessionView } from '@shared/ipc';
import type { TerminalRegistry } from './terminals';

export type Translate = (key: MessageKey, params?: Record<string, string | number>) => string;

export interface AppContextValue {
  t: Translate;
  sessions: Record<string, SessionView>;
  terminals: TerminalRegistry;
  /** Session currently being dragged inside this window, if any. */
  dragging: string | null;
  setDragging: (sessionId: string | null) => void;
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
