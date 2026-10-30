import { useEffect, useRef, useState } from 'react';
import type { ForeignDropRequest } from '@shared/ipc';
import type { DropTarget } from '@shared/layout';
import { useApp } from '../context';

const folderName = (cwd: string) => cwd.split(/[\\/]/).filter(Boolean).pop() ?? cwd;

/** Confirms taking over the session(s) of a terminal window that was dropped onto this window. */
export function TakeoverDialog({
  request,
  target,
  onDone,
}: {
  request: ForeignDropRequest;
  target: DropTarget | null;
  onDone: () => void;
}) {
  const { t } = useApp();
  const [picked, setPicked] = useState(
    () => new Set(request.candidates.filter((c) => c.preselected).map((c) => c.claudeSessionId)),
  );
  const confirmButton = useRef<HTMLButtonElement>(null);
  const single = request.candidates.length === 1;

  useEffect(() => confirmButton.current?.focus(), []);

  const answer = (confirmed: boolean) => {
    window.reframe.answerForeignDrop(request.requestId, confirmed ? { claudeSessionIds: [...picked], target } : null);
    onDone();
  };

  return (
    <div className="modal-backdrop">
      <div
        className="modal"
        role="dialog"
        aria-modal="true"
        aria-labelledby="takeover-title"
        onKeyDown={(event) => event.key === 'Escape' && answer(false)}
      >
        <h2 id="takeover-title">{t('foreign.title')}</h2>
        <p className="modal-text">{single ? t('foreign.single') : t('foreign.multiple')}</p>
        <div className="takeover-list">
          {request.candidates.map((c) => (
            <label key={c.claudeSessionId} className="takeover-item">
              {!single && (
                <input
                  type="checkbox"
                  checked={picked.has(c.claudeSessionId)}
                  onChange={(event) =>
                    setPicked((current) => {
                      const next = new Set(current);
                      if (event.target.checked) next.add(c.claudeSessionId);
                      else next.delete(c.claudeSessionId);
                      return next;
                    })
                  }
                />
              )}
              <span className="sidebar-text">
                <span className="sidebar-title">{c.name ?? folderName(c.cwd)}</span>
                <span className="sidebar-meta" title={c.cwd}>
                  {c.cwd}
                </span>
              </span>
            </label>
          ))}
        </div>
        <p className="modal-hint">{t('foreign.detail')}</p>
        <div className="button-row end">
          <button type="button" className="button" onClick={() => answer(false)}>
            {t('dialog.cancel')}
          </button>
          <button
            ref={confirmButton}
            type="button"
            className="button primary"
            disabled={picked.size === 0}
            onClick={() => answer(true)}
          >
            {t('takeover.confirm')}
          </button>
        </div>
      </div>
    </div>
  );
}
