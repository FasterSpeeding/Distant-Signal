'use client';

import { useState } from 'react';
import { useRouter } from 'next/navigation';
import { Box, Button, Group, Select, Text } from '@mantine/core';
import { formatDate } from '@/lib/dateFormat';
import { routeLabel } from '@/lib/stationLabel';
import type { TrackedTrainListItem } from '@/lib/types';

/** Same gap, same fix, as `components/IncidentSearchForm.tsx`'s own
 * `noOptionsFound` (see that file's comment): Mantine's `Combobox.Empty` --
 * what `nothingFoundMessage` renders into -- is an unstyled `<Box>` with no
 * ARIA role, so a plain string leaves an open listbox with a child that is
 * neither an `option` nor a `group`, failing axe's `aria-required-children`.
 * This `Select` is only ever rendered once `trains` is non-empty (see the
 * early return below), but it is `searchable` -- a typed search narrowing
 * the visible options to zero hits the exact same gap, not just an
 * empty-catalogue fixture. Duplicated locally rather than imported -- same
 * precedent as every other call site that needed this fix. */
function noOptionsFound(label: string) {
  return (
    <Box role="option" aria-disabled="true">
      {label}
    </Box>
  );
}

/** Attaches a standalone ticket (Part A of the upload-first plan --
 * `trackedTrainId: null`) to one of the caller's own already-tracked
 * trains, per Part B's "a clearly separated section for tickets that
 * exist but aren't attached to any train yet, each with a way to attach
 * them to a train." `trains` is the SAME list the merged `/track/mine`
 * page already fetched for its own trains section -- no separate fetch
 * here, just a `<Select>` over data the page already has.
 *
 * Renders nothing when `trains` is empty: there's nothing to attach to
 * yet, and the page's own "track a new train for this ticket" link (next
 * to this component) already covers that case. */
export function AttachTicketAction({ ticketId, trains }: { ticketId: number; trains: TrackedTrainListItem[] }) {
  const router = useRouter();
  const [selected, setSelected] = useState<string | null>(null);
  const [attaching, setAttaching] = useState(false);
  const [error, setError] = useState<string | null>(null);

  if (trains.length === 0) {
    return null;
  }

  async function handleAttach() {
    if (!selected) return;
    setAttaching(true);
    setError(null);
    try {
      const response = await fetch(`/api/Train/tickets/${ticketId}/attach`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ trackingId: Number(selected) }),
      });
      if (response.ok) {
        router.refresh();
        return;
      }
      if (response.status === 409) {
        setError('This ticket has already been attached to a train.');
        return;
      }
      if (response.status === 401) {
        setError('Log in to attach this ticket.');
        return;
      }
      if (response.status === 404) {
        setError("That tracked train couldn't be found.");
        return;
      }
      setError("Couldn't attach this ticket. Try again.");
    } catch {
      setError("Couldn't attach this ticket. Try again.");
    } finally {
      setAttaching(false);
    }
  }

  const options = trains.map((train) => ({
    value: String(train.id),
    label: `${routeLabel(train.pinOriginCrs, train.pinOriginName, train.pinDestinationCrs, train.pinDestinationName)} (${formatDate(train.serviceDate)})`,
  }));

  return (
    <Group gap="xs" align="flex-end" wrap="wrap">
      <Select
        label="Attach to one of your tracked trains"
        placeholder="Pick a tracked train"
        data={options}
        value={selected}
        onChange={setSelected}
        size="xs"
        style={{ minWidth: 240 }}
        searchable
        nothingFoundMessage={noOptionsFound('No matching tracked trains')}
      />
      <Button size="xs" onClick={handleAttach} disabled={!selected || attaching}>
        {attaching ? 'Attaching…' : 'Attach'}
      </Button>
      {error && (
        <Text size="xs" c="var(--ds-color-error-text)">
          {error}
        </Text>
      )}
    </Group>
  );
}
