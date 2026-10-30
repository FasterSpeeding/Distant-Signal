'use client';

import { useId, useState, type ComponentProps } from 'react';
import { SegmentedControl, Stack, Text, VisuallyHidden } from '@mantine/core';
import { TrainSearchForm } from './TrainSearchForm';
import { TrainServiceLookup } from './TrainServiceLookup';
import { TRAIN_URL_PARAMS, type TrainsView } from '@/lib/trainSearch';
import { replaceUrlParams } from '@/lib/useUrlFilterSync';

const VIEW_LABELS: Record<TrainsView, string> = {
  station: 'Station',
  between: 'Between stations',
  service: 'Headcode or UID',
};

/** The `/trains` page's three timetable views, behind one switch: a
 * station's departures or arrivals, the trains between two stations
 * (`TrainSearchForm`), or one train by its headcode or UID
 * (`TrainServiceLookup`). `initialView` is the one the URL names
 * (`trainsViewFor`). Switching clears the URL's timetable params, so the
 * address bar never describes a view that isn't on screen, and the new
 * view starts empty. `ticketId` (the Journeys page's ticket flow) is kept.
 *
 * Three options don't fit side by side on a phone, so the switch stacks
 * below `xs` (`.ds-entry-mode-control`, as New journey's does). */
export function TrainsLookup({
  initialView,
  initialCode,
  searchForm,
}: {
  initialView: TrainsView;
  /** `?headcode=` (or `?uid=`), for the headcode view. */
  initialCode?: string | undefined;
  /** Everything `TrainSearchForm` takes but the view, for the station and
   * between-stations views; the headcode view reads its own share. */
  searchForm: Omit<ComponentProps<typeof TrainSearchForm>, 'view' | 'seedFromUrl'>;
}) {
  const [view, setView] = useState<TrainsView>(initialView);
  // Whether the visitor has switched view on the page: from then on the
  // views seed nothing from the (cleared) address bar.
  const [switched, setSwitched] = useState(false);
  const labelId = useId();

  function switchTo(next: TrainsView) {
    if (next === view) return;
    try {
      window.history.replaceState(null, '', replaceUrlParams(window.location.href, TRAIN_URL_PARAMS, {}));
    } catch {
      // Safari throttles replaceState; the view still switches.
    }
    setSwitched(true);
    setView(next);
  }

  return (
    <Stack gap="md">
      <Stack gap={4}>
        <Text id={labelId} size="xs" fw={600} c="dimmed">
          Look up trains by
        </Text>
        <SegmentedControl
          className="ds-entry-mode-control"
          aria-labelledby={labelId}
          value={view}
          onChange={switchTo}
          withItemsBorders={false}
          data={(Object.keys(VIEW_LABELS) as TrainsView[]).map((value) => ({ value, label: VIEW_LABELS[value] }))}
        />
      </Stack>
      {/* Sighted visitors see the form change; a screen reader hears it. */}
      <VisuallyHidden role="status" aria-live="polite">
        {switched ? `Showing ${VIEW_LABELS[view].toLowerCase()}.` : ''}
      </VisuallyHidden>
      {view === 'service' ? (
        <TrainServiceLookup
          key="service"
          seedFromUrl={!switched}
          initialCode={switched ? '' : initialCode}
          initialDate={switched ? '' : searchForm.initialDate}
          operatorNames={searchForm.operatorNames}
          searchDates={searchForm.searchDates}
        />
      ) : (
        <TrainSearchForm
          key={view}
          view={view}
          seedFromUrl={!switched}
          {...(switched
            ? {
                attachTicketId: searchForm.attachTicketId,
                operatorNames: searchForm.operatorNames,
                unnamedOperatorCodes: searchForm.unnamedOperatorCodes,
                searchDates: searchForm.searchDates,
              }
            : searchForm)}
        />
      )}
    </Stack>
  );
}
