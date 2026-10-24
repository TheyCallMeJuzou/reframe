import type { CSSProperties } from 'react';
import type { SessionStatus } from '@shared/ipc';
import { leaves } from '@shared/layout';
import type { DockEntry } from '@shared/state';
import { useApp } from '../context';
import { StatusDot } from './StatusDot';

// What a collapsed group shows: the status that most needs attention among its sessions.
const URGENCY: SessionStatus[] = ['waiting', 'error', 'busy', 'starting', 'idle', 'ended'];

/** Collapsed tiles and groups; a click puts them back where they were. */
export function DockStrip({ entries }: { entries: DockEntry[] }) {
  const { t, sessions } = useApp();
  if (entries.length === 0) return null;
  return (
    <nav className="dock" aria-label={t('dock.label')}>
      {entries.map((entry) => {
        if (entry.kind === 'group') {
          const group = entry.node;
          const statuses = leaves(group).map((id) => sessions[id]?.status ?? 'ended');
          const status = URGENCY.find((s) => statuses.includes(s)) ?? 'ended';
          const label = t('dock.expandGroup', { label: group.name, count: statuses.length });
          return (
            <button
              key={group.id}
              className="dock-item dock-group"
              style={{ '--group-color': group.color } as CSSProperties}
              title={label}
              aria-label={label}
              data-group={group.id}
              onClick={() => window.reframe.dispatch({ type: 'group/expand', groupId: group.id })}
            >
              <StatusDot status={status} />
              <span className="dock-label">{group.name}</span>
              <span className="dock-count">{statuses.length}</span>
            </button>
          );
        }
        const session = sessions[entry.sessionId];
        if (!session) return null;
        const label = t('dock.expand', { label: session.label });
        return (
          <button
            key={entry.sessionId}
            className="dock-item"
            title={label}
            aria-label={label}
            data-session={entry.sessionId}
            onClick={() => window.reframe.dispatch({ type: 'tile/expand', sessionId: entry.sessionId })}
          >
            <StatusDot status={session.status} />
            <span className="dock-label">{session.label}</span>
          </button>
        );
      })}
    </nav>
  );
}
