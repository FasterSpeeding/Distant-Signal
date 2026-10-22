'use client';

import { useState } from 'react';
import { Alert, Button, Stack, Text } from '@mantine/core';
import { AlternativesSheet } from './AlternativesSheet';
import { breakBody, breakTitle, openBreakTitleFacts } from '@/lib/journeyBreak';
import type { JourneyLegDetail, OpenBreakSummary } from '@/lib/types';

/** A journey's open break (spec §8.1, §8.2): a grape "you need to act"
 * alert, "Connection at York missed" over "3 other ways to Edinburgh.
 * First arrives 11:12.", on the journey page above the legs and on the
 * home page's current journey. Everyone who can read the journey sees it;
 * only the owner gets "See options", which opens the suggestions sheet in
 * place. `initialOpen` opens it on load (the push deep link), with
 * `fromPush` saying the page was opened from the notification. */
export function JourneyBreakBanner({
  journeyId,
  openBreak,
  legs,
  isOwner,
  suggestionsMuted = false,
  initialOpen = false,
  fromPush = false,
}: {
  journeyId: number;
  openBreak: OpenBreakSummary;
  legs: readonly JourneyLegDetail[];
  isOwner: boolean;
  suggestionsMuted?: boolean;
  initialOpen?: boolean;
  fromPush?: boolean;
}) {
  const [opened, setOpened] = useState(isOwner && initialOpen);
  // Only the opening the push led to counts as opened from the push.
  const [viaPush, setViaPush] = useState(isOwner && initialOpen && fromPush);
  const title = breakTitle(openBreakTitleFacts(openBreak, legs));
  const body = breakBody(
    openBreak.suggestionCount,
    openBreak.destinationName,
    openBreak.firstArrival,
    openBreak.suggestionsReady,
  );
  return (
    <Alert color="grape" variant="light" title={title} role="region" aria-label={title} data-journey-break>
      <Stack gap="xs" align="flex-start">
        <Text size="sm">{body}</Text>
        {isOwner && (
          <>
            <Button size="sm" onClick={() => setOpened(true)}>
              See options
            </Button>
            <AlternativesSheet
              journeyId={journeyId}
              mode={{ kind: 'break', breakId: openBreak.id, fromPush: viaPush }}
              opened={opened}
              onClose={() => {
                setOpened(false);
                setViaPush(false);
              }}
              fallbackTitle={title}
              stationName={openBreak.stationName}
              canMute={!suggestionsMuted}
            />
          </>
        )}
      </Stack>
    </Alert>
  );
}
