'use client';

import { useEffect, useState } from 'react';
import { TrainOperatorSelect } from './TrainOperatorSelect';
import { TrainsAdvancedOptions } from './TrainsAdvancedOptions';
import { legFilterErrors, operatorChoicesFromList, type LegFilters } from '@/lib/legFilters';
import { useStopFilterLabels } from '@/lib/useStopFilterLabels';
import type { TrainSearchOperator } from '@/lib/types';

/** `GET /public/trains/search/operators` as the Operator field's options,
 * fetched once per mount; none (the field then finds nothing) when it
 * fails. */
function useOperatorChoices(): { value: string; label: string }[] {
  const [choices, setChoices] = useState<{ value: string; label: string }[]>([]);
  useEffect(() => {
    const controller = new AbortController();
    void (async () => {
      try {
        const response = await fetch('/api/trains/search/operators', { signal: controller.signal });
        if (!response.ok) return;
        const body = (await response.json()) as { operators?: TrainSearchOperator[] };
        if (Array.isArray(body.operators)) setChoices(operatorChoicesFromList(body.operators));
      } catch {
        // No options: the field stays empty rather than failing the form.
      }
    })();
    return () => controller.abort();
  }, []);
  return choices;
}

/** A journey window leg's search filters (DS-215): the Trains section's
 * Between stations Operator field and Advanced options, the same
 * components (`TrainOperatorSelect`, `TrainsAdvancedOptions`) with the same
 * labels, help text, caps and checks, scoped to the leg's two stations.
 * The options start collapsed. Used by "Add a leg"'s "Search a time
 * window" (`AddJourneyLegButton`) and a leg's "Edit search"
 * (`LegSearchForm`); the parent holds the filters and checks them with
 * `legFilterErrors` too, so it can refuse to submit. */
export function LegFilterFields({
  filters,
  onChange,
  origin,
  destination,
}: {
  filters: LegFilters;
  onChange: (filters: LegFilters) => void;
  /** The leg's stations as codes, `null` until each is one. */
  origin: string | null;
  destination: string | null;
}) {
  const operatorOptions = useOperatorChoices();
  const { labels, addNames } = useStopFilterLabels(filters.stops);
  const [opened, setOpened] = useState(false);
  const errors = legFilterErrors(filters, { origin, destination, names: labels });
  const from = origin ?? 'the origin';
  const to = destination ?? 'the destination';
  // A saved operator the list doesn't hold (an older code) stays visible
  // and removable.
  const options = [
    ...operatorOptions,
    ...filters.operators
      .filter((code) => !operatorOptions.some((option) => option.value === code))
      .map((code) => ({ value: code, label: code })),
  ];

  return (
    <>
      <TrainOperatorSelect
        data={options}
        value={filters.operators}
        onChange={(operators) => onChange({ ...filters, operators })}
      />
      <TrainsAdvancedOptions
        filters={filters.stops}
        onChange={(stops) => onChange({ ...filters, stops })}
        names={labels}
        onNames={addNames}
        errors={errors}
        opened={opened}
        onOpenedChange={setOpened}
        scope={`These look at the train's route between ${from} and ${to}.`}
        earlierScope={`To find a particular train, these look at where it has been before ${from}.`}
      />
    </>
  );
}
