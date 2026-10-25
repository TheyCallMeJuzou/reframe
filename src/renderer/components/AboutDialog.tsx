import type { MouseEvent } from 'react';
import license from '../../../LICENSE?raw';
import logo from '../../../docs/logo/reframe-icon.svg';
import { AUTHOR_URL, REPO_URL } from '@shared/links';
import { useApp } from '../context';

/** The license's paragraphs, without the file's line breaks inside them (the dialog wraps them itself). */
const LICENSE_TEXT = license.trim().replace(/([^\n])\n(?=[^\n])/g, '$1 ');

/** About reframe: version, where it comes from, and its license. */
export function AboutDialog({ version, onClose }: { version: string; onClose: () => void }) {
  const { t } = useApp();
  const link = (url: string) => (event: MouseEvent) => {
    event.preventDefault();
    window.reframe.openExternal(url);
  };

  return (
    <div className="modal-backdrop" onMouseDown={(e) => e.target === e.currentTarget && onClose()}>
      <div
        className="modal about"
        role="dialog"
        aria-label={t('about.title')}
        onKeyDown={(e) => e.key === 'Escape' && onClose()}
      >
        <img className="about-logo" src={logo} alt="" width={72} height={72} />
        <h2 className="about-name">reframe</h2>
        <p className="about-version">{t('about.version', { version })}</p>
        <p className="about-tagline">{t('about.tagline')}</p>

        <div className="about-links">
          <a href={REPO_URL} onClick={link(REPO_URL)}>
            {t('about.github')}
          </a>
          <a href={REPO_URL} onClick={link(REPO_URL)}>
            ⭐ {t('about.star')}
          </a>
        </div>

        <p className="about-credit">
          {t('about.madeWith')}{' '}
          <a href={AUTHOR_URL} onClick={link(AUTHOR_URL)}>
            TheyCallMeJuzou
          </a>
        </p>

        <details className="about-license">
          <summary>{t('about.license')}</summary>
          <pre>{LICENSE_TEXT}</pre>
        </details>

        <div className="button-row">
          <button className="button primary" autoFocus onClick={onClose}>
            {t('about.close')}
          </button>
        </div>
      </div>
    </div>
  );
}
