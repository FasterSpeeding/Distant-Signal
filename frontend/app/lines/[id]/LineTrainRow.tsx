import Link from 'next/link';
import { VisuallyHidden } from '@mantine/core';
import { ServiceModeBadge } from '@/components/ServiceModeBadge';
import { isTimetableOnly, serviceNoun } from '@/lib/serviceMode';
import type { LineCatalogueStation, LineTrainSummary } from '@/lib/types';
import { formatClock, lineTimeMinute, liveStatusLabel, stopStrip } from '@/lib/lineTrains';
import classes from './LineTrains.module.css';

/** One train: time at the line (its first public call on the line, not
 * its origin), destination, live status -- one line on a phone -- and, on
 * wider screens, the compact strip of key stations it calls at next. The
 * whole row is the link to the train's page.
 *
 * `beyondPhone` marks a row past the phone window (hidden below the `sm`
 * breakpoint by CSS, so the page needs no JavaScript to show a shorter
 * window on a phone). */
export function LineTrainRow({
  train,
  date,
  stations,
  beyondPhone = false,
  timeOverride,
  showStrip = true,
}: {
  train: LineTrainSummary;
  date: string;
  stations: LineCatalogueStation[];
  beyondPhone?: boolean;
  /** A time other than `lineDue`, e.g. the departure from a picked station. */
  timeOverride?: string;
  /** `false` for a row with no on-line stops of its own (a station-pair
   * search row). */
  showStrip?: boolean;
}) {
  const minute = lineTimeMinute(train.lineDue);
  const time = timeOverride ?? (minute === null ? '--:--' : formatClock(minute));
  const destination =
    train.destination?.name ?? train.destination?.crs ?? `${serviceNoun(train.serviceMode)} ${train.uid}`;
  const status = liveStatusLabel(train.live, train.serviceMode);
  const timetableOnly = isTimetableOnly(train);
  const strip = stopStrip(train, stations);
  const statusClass =
    status.tone === 'cancelled'
      ? classes.cancelled
      : status.tone === 'late'
        ? classes.late
        : status.tone === 'onTime'
          ? classes.onTime
          : classes.muted;
  return (
    <li
      className={beyondPhone ? classes.beyondPhone : undefined}
      data-uid={train.uid}
      data-beyond-phone={beyondPhone || undefined}
    >
      <Link href={`/train/${encodeURIComponent(train.uid)}/${date}`} className={classes.row}>
        <span className={`${classes.time} ${status.tone === 'cancelled' ? classes.struck : ''}`}>{time}</span>{' '}
        <span className={classes.dest}>
          <VisuallyHidden>to </VisuallyHidden>
          {destination}
          {timetableOnly && (
            <>
              {' '}
              <ServiceModeBadge mode={train.serviceMode} />
            </>
          )}
        </span>{' '}
        <span className={`${classes.status} ${statusClass}`}>{status.text}</span>{' '}
        {showStrip && strip.shown.length > 0 && (
          <span className={classes.strip} aria-hidden="true">
            {strip.shown.join(' · ')}
            {strip.hidden > 0 && ` · +${strip.hidden} stop${strip.hidden === 1 ? '' : 's'}`}
          </span>
        )}
        {showStrip && <VisuallyHidden>. {strip.text}.</VisuallyHidden>}
      </Link>
    </li>
  );
}
