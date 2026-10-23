import type { SessionStatus } from '@shared/ipc';
import { useApp } from '../context';

export function StatusDot({ status }: { status: SessionStatus }) {
  const { t } = useApp();
  const label = t(`status.${status}`);
  return <span className={`status-dot status-${status}`} title={label} aria-label={label} role="img" />;
}
