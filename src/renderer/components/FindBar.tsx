import { useEffect, useState } from 'react';
import { useApp } from '../context';
import { ChevronDownIcon, ChevronUpIcon, CloseIcon } from './icons';

/**
 * Finding text in a tile's terminal, its scrollback included: every match is marked, Enter and Shift+Enter go to the
 * next and previous one, Esc closes (and leaves no marks behind).
 */
export function FindBar({ sessionId }: { sessionId: string }) {
  const { t, terminals, setFindIn } = useApp();
  const [text, setText] = useState('');
  /** Where the current match is among how many; index -1 when there are too many to count them all. */
  const [result, setResult] = useState<{ index: number; count: number } | null>(null);

  useEffect(() => terminals.onFindResults(sessionId, setResult), [sessionId, terminals]);
  useEffect(() => () => terminals.clearFind(sessionId), [sessionId, terminals]);

  const close = () => {
    setFindIn(null);
    // Back to typing in the session, once the bar is gone.
    requestAnimationFrame(() => terminals.focus(sessionId));
  };
  const go = (direction: 'next' | 'previous') => {
    if (text) terminals.find(sessionId, text, direction);
  };
  const count =
    !text || !result
      ? ''
      : result.count === 0
        ? t('find.none')
        : result.index < 0
          ? t('find.found', { total: result.count })
          : t('find.count', { n: result.index + 1, total: result.count });

  return (
    // Clicks here stay here (a click in the tile otherwise puts the focus in its terminal).
    <div className="tile-find" role="search" onMouseDown={(event) => event.stopPropagation()}>
      <input
        className="tile-find-input"
        value={text}
        placeholder={t('find.placeholder')}
        aria-label={t('find.placeholder')}
        spellCheck={false}
        autoFocus
        onChange={(event) => {
          setText(event.target.value);
          if (!event.target.value) setResult(null);
          terminals.find(sessionId, event.target.value, 'next', true);
        }}
        onKeyDown={(event) => {
          // Keys typed here belong to the field, not to the terminal around it.
          event.stopPropagation();
          if (event.key === 'Escape') {
            event.preventDefault();
            close();
          } else if (event.key === 'Enter') {
            event.preventDefault();
            go(event.shiftKey ? 'previous' : 'next');
          }
        }}
      />
      <span className="tile-find-count" aria-live="polite">
        {count}
      </span>
      <button
        className="icon-button"
        title={t('find.previous')}
        aria-label={t('find.previous')}
        onClick={() => go('previous')}
      >
        <ChevronUpIcon />
      </button>
      <button className="icon-button" title={t('find.next')} aria-label={t('find.next')} onClick={() => go('next')}>
        <ChevronDownIcon />
      </button>
      <button className="icon-button" title={t('find.close')} aria-label={t('find.close')} onClick={close}>
        <CloseIcon />
      </button>
    </div>
  );
}
