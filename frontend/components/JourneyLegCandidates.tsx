'use client';

import { useEffect, useState } from 'react';
import { Alert, Button, Group, Stack, Text } from '@mantine/core';
import { TextLink } from './TextLink';

/** Wire shape of `GET /Journeys/{id}/legs/{id}/candidates` -- the same
 * envelope `GET /public/trains/search` returns
 * (`crates/api/src/render.rs::calling_point_departure_json`), reused
 * verbatim per design doc §2.2. */
interface CandidateRow {
  uid: string;
  scheduled: string;
  destinationCrs: string | null;
  trueOriginCrs: string | null;
  destinationArrival: string | null;
}

interface CandidatesResponse {
  results: CandidateRow[];
  nextCursor: string | null;
}

/** The open-leg candidate list + pick action -- design doc §2.2/§2.3/§4.
 * `onPicked` is called after a successful commit; the caller (a
 * `JourneyLegCard`, `frontend/components/JourneyLegCard.tsx`) decides what
 * to do next (typically `router.refresh()`). Pagination (`nextCursor`) is
 * deliberately not implemented in this first pass -- the backend route
 * supports it (same shape `TrainSearchForm.tsx`'s own "Load more" already
 * consumes), but a journey leg's candidate list is expected to be short
 * (a bounded time window, not a whole day's unfiltered search); add a
 * `LoadMoreControl` here, mirroring `TrainSearchForm.tsx`'s own, if that
 * assumption proves wrong in practice. */
export function JourneyLegCandidates({
  journeyId,
  legId,
  serviceDate,
  onPicked,
}: {
  journeyId: number;
  legId: number;
  serviceDate: string;
  onPicked: () => void;
}) {
  const [results, setResults] = useState<CandidateRow[] | 'loading' | 'error' | null>(null);
  const [picking, setPicking] = useState<string | null>(null);
  const [pickError, setPickError] = useState<string | null>(null);

  useEffect(() => {
    let cancelled = false;
    setResults('loading');
    fetch(`/api/Journeys/${journeyId}/legs/${legId}/candidates`)
      .then((res) => (res.ok ? res.json() : Promise.reject(res)))
      .then((body: CandidatesResponse) => {
        if (!cancelled) setResults(body.results);
      })
      .catch(() => {
        if (!cancelled) setResults('error');
      });
    return () => {
      cancelled = true;
    };
  }, [journeyId, legId]);

  async function pick(uid: string) {
    setPicking(uid);
    setPickError(null);
    try {
      const response = await fetch(`/api/Journeys/${journeyId}/legs/${legId}/train`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ trainUid: uid, serviceDate }),
      });
      if (!response.ok) {
        setPickError("Couldn't track that train. Try again.");
        return;
      }
      onPicked();
    } catch {
      setPickError("Couldn't track that train. Try again.");
    } finally {
      setPicking(null);
    }
  }

  if (results === null || results === 'loading') {
    return (
      <Text size="sm" c="dimmed">
        Searching for candidate trains…
      </Text>
    );
  }
  if (results === 'error') {
    return (
      <Alert color="red" title="Search failed">
        Couldn&apos;t load candidate trains right now. Try again.
      </Alert>
    );
  }
  if (results.length === 0) {
    return (
      <Text size="sm" c="dimmed">
        No scheduled trains match this window.{' '}
        <TextLink href="/track" inline underline="always">
          Search manually
        </TextLink>{' '}
        instead.
      </Text>
    );
  }
  return (
    <Stack gap="xs">
      {pickError && <Alert color="red">{pickError}</Alert>}
      {results.map((row) => (
        <Group key={row.uid} justify="space-between" wrap="wrap">
          <Text size="sm">
            {row.scheduled} · {row.trueOriginCrs ?? '?'} → {row.destinationCrs ?? '?'}
          </Text>
          <Button size="xs" loading={picking === row.uid} disabled={picking !== null} onClick={() => pick(row.uid)}>
            Track this train
          </Button>
        </Group>
      ))}
    </Stack>
  );
}
