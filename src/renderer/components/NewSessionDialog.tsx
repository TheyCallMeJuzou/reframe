import { useEffect, useRef, useState, type FormEvent } from 'react';
import type { NewSessionResult } from '@shared/ipc';
import { PERMISSION_MODES, type PermissionMode } from '@shared/launch';
import { useApp } from '../context';

const MODEL_SUGGESTIONS = ['fable', 'opus', 'sonnet', 'haiku'];

export function NewSessionDialog({ onClose }: { onClose: () => void }) {
  const { t } = useApp();
  const [cwd, setCwd] = useState('');
  const [name, setName] = useState('');
  const [worktree, setWorktree] = useState(false);
  const [worktreeName, setWorktreeName] = useState('');
  const [model, setModel] = useState('');
  const [permissionMode, setPermissionMode] = useState<PermissionMode | ''>('');
  const [recent, setRecent] = useState<string[]>([]);
  const [error, setError] = useState<Extract<NewSessionResult, { ok: false }>['error'] | null>(null);
  const [busy, setBusy] = useState(false);
  const folderInput = useRef<HTMLInputElement>(null);

  useEffect(() => {
    void window.reframe.getRecentFolders().then((folders) => {
      setRecent(folders);
      setCwd((current) => current || folders[0] || '');
    });
    folderInput.current?.focus();
  }, []);

  const submit = async (event: FormEvent) => {
    event.preventDefault();
    if (!cwd.trim() || busy) return;
    setBusy(true);
    const result = await window.reframe.createSession({
      cwd: cwd.trim(),
      name: name.trim() || undefined,
      worktree,
      worktreeName: worktree ? worktreeName.trim() || undefined : undefined,
      model: model.trim() || undefined,
      permissionMode: permissionMode || undefined,
    });
    setBusy(false);
    if (result.ok) onClose();
    else setError(result.error);
  };

  const browse = async () => {
    const folder = await window.reframe.pickFolder();
    if (folder) {
      setCwd(folder);
      setError(null);
    }
  };

  return (
    <div className="modal-backdrop" onMouseDown={(e) => e.target === e.currentTarget && onClose()}>
      <form
        className="modal"
        role="dialog"
        aria-modal="true"
        aria-labelledby="new-session-title"
        onSubmit={submit}
        onKeyDown={(e) => e.key === 'Escape' && onClose()}
      >
        <h2 id="new-session-title">{t('dialog.title')}</h2>

        <label className="field">
          <span>{t('dialog.folder')}</span>
          <div className="field-row">
            <input
              ref={folderInput}
              value={cwd}
              placeholder={t('dialog.folderPlaceholder')}
              spellCheck={false}
              onChange={(e) => {
                setCwd(e.target.value);
                setError(null);
              }}
            />
            <button type="button" className="button" onClick={browse}>
              {t('dialog.browse')}
            </button>
          </div>
        </label>
        {recent.length > 0 && (
          <div className="recent" aria-label={t('dialog.recent')}>
            {recent.map((folder) => (
              <button
                type="button"
                key={folder}
                className={`chip${folder === cwd ? ' is-selected' : ''}`}
                title={folder}
                onClick={() => {
                  setCwd(folder);
                  setError(null);
                }}
              >
                {folder.split(/[\\/]/).filter(Boolean).pop() ?? folder}
              </button>
            ))}
          </div>
        )}

        <label className="field">
          <span>{t('dialog.name')}</span>
          <input value={name} placeholder={t('dialog.namePlaceholder')} onChange={(e) => setName(e.target.value)} />
        </label>

        <label className="checkbox">
          <input type="checkbox" checked={worktree} onChange={(e) => setWorktree(e.target.checked)} />
          <span>{t('dialog.worktree')}</span>
        </label>
        {worktree && (
          <label className="field nested">
            <span>{t('dialog.worktreeName')}</span>
            <input value={worktreeName} spellCheck={false} onChange={(e) => setWorktreeName(e.target.value)} />
          </label>
        )}

        <details className="advanced">
          <summary>{t('dialog.advanced')}</summary>
          <label className="field">
            <span>{t('dialog.model')}</span>
            <input
              value={model}
              list="model-suggestions"
              placeholder={t('dialog.modelPlaceholder')}
              spellCheck={false}
              onChange={(e) => setModel(e.target.value)}
            />
            <datalist id="model-suggestions">
              {MODEL_SUGGESTIONS.map((m) => (
                <option key={m} value={m} />
              ))}
            </datalist>
          </label>
          <label className="field">
            <span>{t('dialog.permissionMode')}</span>
            <select value={permissionMode} onChange={(e) => setPermissionMode(e.target.value as PermissionMode | '')}>
              <option value="">{t('dialog.default')}</option>
              {PERMISSION_MODES.map((mode) => (
                <option key={mode} value={mode}>
                  {mode}
                </option>
              ))}
            </select>
          </label>
        </details>

        {error && <p className="form-error">{t(`dialog.error.${error}`)}</p>}

        <div className="button-row end">
          <button type="button" className="button" onClick={onClose}>
            {t('dialog.cancel')}
          </button>
          <button type="submit" className="button primary" disabled={!cwd.trim() || busy}>
            {t('dialog.start')}
          </button>
        </div>
      </form>
    </div>
  );
}
