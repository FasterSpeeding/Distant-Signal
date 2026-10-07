import Link from 'next/link';
import type { ComponentPropsWithoutRef, ReactNode } from 'react';
import { VisuallyHidden } from '@mantine/core';
import { ServiceModeBadge } from '@/components/ServiceModeBadge';
import { isTimetableOnly, serviceNoun } from '@/lib/serviceMode';
import type { LineCatalogueStation, LineTrainSummary } from '@/lib/types';
import { formatClock, lineTimeMinute, stopStrip } from '@/lib/lineTrains';
import { dayOffsetMarker, liveStatusLabel, type LiveTone } from '@/lib/serviceStatus';
import classes from './ServiceRow.module.css';

const TONE_CLASS: Record<LiveTone, string | undefined> = {
  cancelled: classes.cancelled,
  late: classes.late,
  early: classes.early,
  onTime: classes.onTime,
  timetable: classes.muted,
  none: classes.muted,
};

/** The list a {@link ServiceRow} goes in: bordered, one rule between rows. */
export function ServiceRowList(props: Omit<ComponentPropsWithoutRef<'ul'>, 'className'>) {
  return <ul className={classes.list} {...props} />;
}

/** The "+1" after a time on a later day than the service date, with the
 * words a screen reader hears instead. */
function NextDay({ dayOffset, spoken = true }: { dayOffset: number | undefined; spoken?: boolean }) {
  const marker = dayOffsetMarker(dayOffset);
  if (!marker) return null;
  return (
    <>
      <span className={classes.nextDay} aria-hidden="true">
        {marker.short}
      </span>
      {spoken && <VisuallyHidden> ({marker.spoken})</VisuallyHidden>}
    </>
  );
}

export interface ServiceRowProps {
  train: LineTrainSummary;
  /** The service date (`YYYY-MM-DD`) the train's page is linked for. */
  date: string;
  /** The line's catalogue, for the stop strip (its `role` picks the key
   * stations). Without it there is no strip. */
  stations?: LineCatalogueStation[] | undefined;
  /** A row past the phone window: hidden below the `sm` breakpoint by CSS,
   * so the page needs no JavaScript to show a shorter window on a phone. */
  beyondPhone?: boolean | undefined;
  /** A time other than `lineDue`, e.g. the departure from a picked station. */
  timeOverride?: string | undefined;
  /** Days after the service date the listed time falls on; a positive
   * value shows a "+1" marker ("next day" to a screen reader). */
  dayOffset?: number | undefined;
  /** `false` for a row with no on-line stops of its own (a station-pair
   * search row). */
  showStrip?: boolean | undefined;
  /** The arrival at a picked destination station (`HH:MM`), shown after
   * the destination. */
  arrival?: string | undefined;
  /** Days after the service date `arrival` falls on, as `dayOffset`. */
  arrivalDayOffset?: number | undefined;
  /** A dimmed second line, e.g. "From Reading · South Western Railway". */
  details?: string | undefined;
  /** Controls of the row's own (a "Track this train" button). With
   * actions, the destination is the link rather than the whole row, so no
   * control is nested in a link. */
  actions?: ReactNode;
}

/** One service: the time (at the line, at a picked station, or a leg's
 * departure), destination, live status -- one line on a phone -- and, on
 * wider screens, the compact strip of key stations it calls at next.
 *
 * Without `actions` the whole row is the link to the train's page (a 44px
 * tap target); with them, the destination is the link and the actions sit
 * at the end of the row (below it on a phone). */
export function ServiceRow({
  train,
  date,
  stations,
  beyondPhone = false,
  timeOverride,
  dayOffset,
  showStrip = true,
  arrival,
  arrivalDayOffset,
  details,
  actions,
}: ServiceRowProps) {
  const minute = lineTimeMinute(train.lineDue);
  const time = timeOverride ?? (minute === null ? '--:--' : formatClock(minute));
  const destination =
    train.destination?.name ?? train.destination?.crs ?? `${serviceNoun(train.serviceMode)} ${train.uid}`;
  const status = liveStatusLabel(train.live, train.serviceMode);
  const timetableOnly = isTimetableOnly(train);
  const strip = stations && showStrip ? stopStrip(train, stations) : null;
  const href = `/train/${encodeURIComponent(train.uid)}/${date}`;
  const hasActions = actions !== undefined && actions !== null && actions !== false;
  const nextDay = dayOffsetMarker(dayOffset);

  const timeCell = (
    <span
      className={`${classes.time} ${status.tone === 'cancelled' ? classes.struck : ''}`}
      aria-hidden={hasActions || undefined}
    >
      {time}
      <NextDay dayOffset={dayOffset} spoken={!hasActions} />
    </span>
  );
  const afterDestination = (
    <>
      {arrival && (
        <span className={classes.arrival}>
          {' '}
          <span aria-hidden="true">· arr</span>
          <VisuallyHidden>, arriving at</VisuallyHidden> {arrival}
          <NextDay dayOffset={arrivalDayOffset} />
        </span>
      )}
      {timetableOnly && (
        <>
          {' '}
          <ServiceModeBadge mode={train.serviceMode} />
        </>
      )}
    </>
  );
  const rest = (
    <>
      <span className={`${classes.status} ${TONE_CLASS[status.tone] ?? ''}`}>{status.text}</span>{' '}
      {details && (
        <>
          <VisuallyHidden>. </VisuallyHidden>
          <span className={classes.details}>{details}</span>{' '}
        </>
      )}
      {strip && strip.shown.length > 0 && (
        <span className={classes.strip} aria-hidden="true">
          {strip.shown.join(' · ')}
          {strip.hidden > 0 && ` · +${strip.hidden} stop${strip.hidden === 1 ? '' : 's'}`}
        </span>
      )}
      {strip && <VisuallyHidden>. {strip.text}.</VisuallyHidden>}
    </>
  );

  return (
    <li
      className={beyondPhone ? classes.beyondPhone : undefined}
      data-uid={train.uid}
      data-beyond-phone={beyondPhone || undefined}
    >
      {hasActions ? (
        <div className={classes.row} data-details={details ? true : undefined} data-actions>
          {timeCell}{' '}
          <span className={classes.dest}>
            <Link href={href} className={classes.destLink}>
              <VisuallyHidden>
                {time}
                {nextDay && ` (${nextDay.spoken})`}{' '}
              </VisuallyHidden>
              <VisuallyHidden>to </VisuallyHidden>
              {destination}
            </Link>
            {afterDestination}
          </span>{' '}
          {rest}
          <span className={classes.actions}>{actions}</span>
        </div>
      ) : (
        <Link href={href} className={classes.row} data-details={details ? true : undefined}>
          {timeCell}{' '}
          <span className={classes.dest}>
            <VisuallyHidden>to </VisuallyHidden>
            {destination}
            {afterDestination}
          </span>{' '}
          {rest}
        </Link>
      )}
    </li>
  );
}
