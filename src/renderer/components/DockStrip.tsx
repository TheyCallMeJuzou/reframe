import type { DockEntry } from '@shared/state';
import { useApp } from '../context';
import { StatusDot } from './StatusDot';

/** Collapsed sessions; a click puts the tile back where it was. */
export function DockStrip({ entries }: { entries: DockEntry[] }) {
  const { t, sessions } = useApp();
  if (entries.length === 0) return null;
  return (
    <nav className="dock" aria-label={t('dock.label')}>
      {entries.map(({ sessionId }) => {
        const session = sessions[sessionId];
        if (!session) return null;
        const label = t('dock.expand', { label: session.label });
        return (
          <button
            key={sessionId}
            className="dock-item"
            title={label}
            aria-label={label}
            data-session={sessionId}
            onClick={() => window.reframe.dispatch({ type: 'tile/expand', sessionId })}
          >
            <StatusDot status={session.status} />
            <span className="dock-label">{session.label}</span>
          </button>
        );
      })}
    </nav>
  );
}
