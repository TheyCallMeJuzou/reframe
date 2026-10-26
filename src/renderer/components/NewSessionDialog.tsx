import { useEffect, useRef, useState, type FormEvent } from 'react';
import type { NewSessionResult } from '@shared/ipc';
import { MODEL_SUGGESTIONS, PERMISSION_MODES, type PermissionMode } from '@shared/launch';
import { useApp } from '../context';

const folderName = (folder: string): string => folder.trim().split(/[\\/]/).filter(Boolean).pop() ?? '';

export function NewSessionDialog({ onClose }: { onClose: () => void }) {
  const { t } = useApp();
  const [cwd, setCwd] = useState('');
  // null until the user types a name; then the typed name wins over the folder name.
  const [customName, setCustomName] = useState<string | null>(null);
  const [nameFromFolder, setNameFromFolder] = useState(false);
  const [worktree, setWorktree] = useState(false);
  const [worktreeName, setWorktreeName] = useState('');
  const [model, setModel] = useState('');
  const [permissionMode, setPermissionMode] = useState<PermissionMode | ''>('');
  const [recent, setRecent] = useState<string[]>([]);
  const [error, setError] = useState<Extract<NewSessionResult, { ok: false }> | null>(null);
  const [busy, setBusy] = useState(false);
  const folderInput = useRef<HTMLInputElement>(null);

  useEffect(() => {
    void window.reframe.getDialogDefaults().then(({ recentFolders, preferences }) => {
      setRecent(recentFolders);
      setCwd((current) => current || recentFolders[0] || '');
      setNameFromFolder(preferences.nameFromFolder);
      // What Settings says new sessions start with, unless something was picked here already.
      setModel((current) => current || preferences.defaultModel || '');
      setPermissionMode((current) => current || preferences.defaultPermissionMode || '');
    });
    folderInput.current?.focus();
  }, []);

  const name = customName ?? (nameFromFolder ? folderName(cwd) : '');

  const toggleNameFromFolder = (on: boolean) => {
    setNameFromFolder(on);
    window.reframe.setPreferences({ nameFromFolder: on });
    // Back on: follow the folder again. Off: drop the folder name, but keep anything typed.
    if (on) setCustomName(null);
    else setCustomName((typed) => typed ?? '');
  };

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
    else setError(result);
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
                {folderName(folder) || folder}
              </button>
            ))}
          </div>
        )}

        <label className="field">
          <span>{t('dialog.name')}</span>
          <input
            value={name}
            placeholder={t('dialog.namePlaceholder')}
            onChange={(e) => setCustomName(e.target.value)}
          />
        </label>
        <label className="checkbox nested">
          <input type="checkbox" checked={nameFromFolder} onChange={(e) => toggleNameFromFolder(e.target.checked)} />
          <span>{t('dialog.nameFromFolder')}</span>
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

        {error && (
          <p className="form-error">
            {error.distro ? t('error.claudeNotFoundInWsl', { distro: error.distro }) : t(`dialog.error.${error.error}`)}
            {error.error === 'claudeNotFound' &&
              !error.distro &&
              window.reframe.platform === 'win32' &&
              ` ${t('error.claudeOnlyInWsl')}`}
          </p>
        )}

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
