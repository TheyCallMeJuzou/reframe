import { FONT_SIZE, type CloseWindow, type Language, type Preferences } from '@shared/state';
import { useApp } from '../context';
import { fontSizeAfter } from '../shortcuts';

/**
 * Settings: the language, the terminals' text size, snapping separators to a grid, what closing a window with
 * sessions does, and update checks (where this copy checks at all). Each applies at once.
 */
export function SettingsDialog({
  language,
  systemLocale,
  fontSize,
  snapToGrid,
  closeWindow,
  checkForUpdates,
  onClose,
}: {
  language: Language;
  systemLocale: string;
  fontSize: number;
  snapToGrid: boolean;
  closeWindow: CloseWindow;
  checkForUpdates: boolean | null;
  onClose: () => void;
}) {
  const { t, keys, locale } = useApp();
  const set = (patch: Partial<Preferences>) => window.reframe.setPreferences(patch);
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
