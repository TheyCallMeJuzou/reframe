import { useEffect, useState } from 'react';
import type { PreferencesPatch } from '@shared/ipc';
import { MODEL_SUGGESTIONS, PERMISSION_MODES, type PermissionMode } from '@shared/launch';
import { FONT_SIZE, type CloseWindow, type Language } from '@shared/state';
import { useApp } from '../context';
import { fontSizeAfter } from '../shortcuts';

/**
 * Settings: the language, the terminals' text size, snapping separators to a grid, what closing a window with
 * sessions does, which claude runs and what new sessions start with, and update checks (where this copy checks at
 * all). Each applies at once.
 */
export function SettingsDialog({
  language,
  systemLocale,
  fontSize,
  snapToGrid,
  closeWindow,
  checkForUpdates,
  claudeBin,
  defaultModel,
  defaultPermissionMode,
  onClose,
}: {
  language: Language;
  systemLocale: string;
  fontSize: number;
  snapToGrid: boolean;
  closeWindow: CloseWindow;
  checkForUpdates: boolean | null;
  claudeBin: string | null;
  defaultModel: string | null;
  defaultPermissionMode: PermissionMode | null;
  onClose: () => void;
}) {
  const { t, keys, locale } = useApp();
  const set = (patch: PreferencesPatch) => window.reframe.setPreferences(patch);
  // Where the claude that runs is (looked up again when another one is chosen).
  const [claude, setClaude] = useState<string | null | undefined>(undefined);
  useEffect(() => {
    let alive = true;
    void window.reframe.getDialogDefaults().then((defaults) => alive && setClaude(defaults.claude));
    return () => {
      alive = false;
    };
  }, [claudeBin]);
  // Typed here, kept when the field is left.
  const [model, setModel] = useState(defaultModel ?? '');
  const chooseClaude = async () => {
    const file = await window.reframe.pickClaude();
    if (file) set({ claudeBin: file });
  };
  // What "System" stands for right now, named in the current language.
  const systemName = new Intl.DisplayNames([locale], { type: 'language' }).of(systemLocale);
  const ctrl = window.reframe.platform === 'darwin' ? '⌘' : t('keys.ctrl');

  return (
    <div className="modal-backdrop" onMouseDown={(e) => e.target === e.currentTarget && onClose()}>
      <div
        className="modal settings"
        role="dialog"
        aria-label={t('settings.title')}
        onKeyDown={(e) => e.key === 'Escape' && onClose()}
      >
        <h2>{t('settings.title')}</h2>

        <label className="setting">
          <span>{t('settings.language')}</span>
          <select value={language} onChange={(e) => set({ language: e.target.value as Language })}>
            <option value="system">{t('settings.languageSystem', { language: systemName ?? '' })}</option>
            <option value="en">English</option>
            <option value="de">Deutsch</option>
          </select>
        </label>

        <div className="setting">
          <span>{t('settings.textSize')}</span>
          <span className="setting-stepper">
            <button
              className="button"
              aria-label={t('settings.smaller')}
              disabled={fontSize <= FONT_SIZE.min}
              onClick={() => set({ fontSize: fontSizeAfter(fontSize, 'smaller') })}
            >
              −
            </button>
            <output className="setting-value">{fontSize}</output>
            <button
              className="button"
              aria-label={t('settings.bigger')}
              disabled={fontSize >= FONT_SIZE.max}
              onClick={() => set({ fontSize: fontSizeAfter(fontSize, 'bigger') })}
            >
              +
            </button>
            <button
              className="button"
              disabled={fontSize === FONT_SIZE.normal}
              onClick={() => set({ fontSize: FONT_SIZE.normal })}
            >
              {t('settings.normal')}
            </button>
          </span>
        </div>
        <p className="setting-hint">
          {t('settings.textSizeHint', {
            bigger: keys('=', false),
            smaller: keys('-', false),
            normal: keys('0', false),
            ctrl,
          })}
        </p>

        <label className="checkbox">
          {/* Turns at once; the saved value comes back with the next view (and from other windows too). */}
          <input
            key={String(snapToGrid)}
            type="checkbox"
            defaultChecked={snapToGrid}
            onChange={(e) => set({ snapToGrid: e.target.checked })}
          />
          {t('settings.snap')}
        </label>
        <p className="setting-hint">{t('settings.snapHint')}</p>

        <label className="setting">
          <span>{t('settings.closeWindow')}</span>
          <select value={closeWindow} onChange={(e) => set({ closeWindow: e.target.value as CloseWindow })}>
            <option value="ask">{t('settings.closeWindowAsk')}</option>
            <option value="end">{t('settings.closeWindowEnd')}</option>
            <option value="keep">{t('settings.closeWindowKeep')}</option>
          </select>
        </label>
        <p className="setting-hint">{t('settings.closeWindowHint')}</p>

        <div className="setting">
          <span>{t('settings.claude')}</span>
          <span className="setting-stepper">
            {claudeBin && (
              <button className="button" onClick={() => set({ claudeBin: '' })}>
                {t('settings.claudeUsePath')}
              </button>
            )}
            <button className="button" onClick={() => void chooseClaude()}>
              {t('settings.claudeChoose')}
            </button>
          </span>
        </div>
        {claude !== undefined && (
          <p className="setting-hint setting-claude">
            {claudeBin ? (claude ? t('settings.claudeChosen') : t('settings.claudeMissing')) : null}
            {!claudeBin && (claude ? t('settings.claudeFound') : t('settings.claudeNotFound'))}
            {(claudeBin ?? claude) && (
              // Cut off at its start, so the file stays in sight, but read left to right.
              <span className="setting-path" title={claudeBin ?? claude ?? ''}>
                <bdi>{claudeBin ?? claude}</bdi>
              </span>
            )}
          </p>
        )}
        <p className="setting-hint">{t('settings.claudeHint')}</p>

        <label className="setting">
          <span>{t('settings.defaultModel')}</span>
          <input
            className="setting-input"
            value={model}
            list="settings-models"
            placeholder={t('dialog.modelPlaceholder')}
            spellCheck={false}
            onChange={(e) => setModel(e.target.value)}
            onBlur={() => model.trim() !== (defaultModel ?? '') && set({ defaultModel: model })}
            onKeyDown={(e) => e.key === 'Enter' && e.currentTarget.blur()}
          />
          <datalist id="settings-models">
            {MODEL_SUGGESTIONS.map((m) => (
              <option key={m} value={m} />
            ))}
          </datalist>
        </label>
        <label className="setting">
          <span>{t('settings.defaultPermissionMode')}</span>
          <select
            value={defaultPermissionMode ?? ''}
            onChange={(e) => set({ defaultPermissionMode: e.target.value as PermissionMode | '' })}
          >
            <option value="">{t('dialog.default')}</option>
            {PERMISSION_MODES.map((mode) => (
              <option key={mode} value={mode}>
                {mode}
              </option>
            ))}
          </select>
        </label>
        <p className="setting-hint">{t('settings.launchHint')}</p>

        {checkForUpdates !== null && (
          <>
            <label className="checkbox">
              <input
                key={String(checkForUpdates)}
                type="checkbox"
                defaultChecked={checkForUpdates}
                onChange={(e) => set({ checkForUpdates: e.target.checked })}
              />
              {t('settings.updates')}
            </label>
            <p className="setting-hint">{t('settings.updatesHint')}</p>
          </>
        )}

        <div className="button-row end">
          <button className="button primary" autoFocus onClick={onClose}>
            {t('settings.close')}
          </button>
        </div>
      </div>
    </div>
  );
}
