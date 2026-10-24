import { useState } from 'react';
import { useApp } from '../context';

/** Names this window before it is saved as a workspace. */
export function SaveWorkspaceDialog({ suggestion, onClose }: { suggestion: string; onClose: () => void }) {
  const { t } = useApp();
  const [name, setName] = useState(suggestion);
  const save = () => {
    window.reframe.saveWorkspace(name.trim() || suggestion);
    onClose();
  };
  return (
    <div className="modal-backdrop" onMouseDown={(e) => e.target === e.currentTarget && onClose()}>
      <form
        className="modal"
        role="dialog"
        aria-label={t('workspace.nameTitle')}
        onKeyDown={(e) => e.key === 'Escape' && onClose()}
        onSubmit={(e) => {
          e.preventDefault();
          save();
        }}
      >
        <h2>{t('workspace.nameTitle')}</h2>
        <p className="modal-hint">{t('workspace.nameHint')}</p>
        <label className="field">
          <span>{t('workspace.nameLabel')}</span>
          <input
            value={name}
            autoFocus
            onFocus={(e) => e.currentTarget.select()}
            onChange={(e) => setName(e.target.value)}
          />
        </label>
        <div className="button-row">
          <button type="button" className="button" onClick={onClose}>
            {t('dialog.cancel')}
          </button>
          <button type="submit" className="button primary">
            {t('workspace.saveButton')}
          </button>
        </div>
      </form>
    </div>
  );
}
