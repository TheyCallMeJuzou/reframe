// When a session was last used, as the session list shows it: short ("5m", "2h", "3d", then the date), and exact
// for its tooltip.
import type { Locale } from '@shared/i18n';
import type { Translate } from './context';

const MINUTE = 60_000;
const HOUR = 60 * MINUTE;
const DAY = 24 * HOUR;

export function shortTime(time: number, now: number, t: Translate, locale: Locale): string {
  const past = Math.max(0, now - time);
  if (past < MINUTE) return t('list.usedNow');
  if (past < HOUR) return t('list.usedMinutes', { n: Math.floor(past / MINUTE) });
  if (past < DAY) return t('list.usedHours', { n: Math.floor(past / HOUR) });
  if (past < 7 * DAY) return t('list.usedDays', { n: Math.floor(past / DAY) });
  const date = new Date(time);
  const thisYear = date.getFullYear() === new Date(now).getFullYear();
  return new Intl.DateTimeFormat(
    locale,
    thisYear ? { day: 'numeric', month: 'short' } : { month: 'short', year: 'numeric' },
  ).format(date);
}

export function exactTime(time: number, locale: Locale): string {
  return new Intl.DateTimeFormat(locale, { dateStyle: 'medium', timeStyle: 'short' }).format(new Date(time));
}
