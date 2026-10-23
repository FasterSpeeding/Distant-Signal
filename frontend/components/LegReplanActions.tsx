'use client';

import { useState } from 'react';
import { Button } from '@mantine/core';
import { AlternativesSheet, type AlternativesMode } from './AlternativesSheet';
import { breakTitle } from '@/lib/journeyBreak';
import { legDepartureInstant } from '@/lib/journeyLegChaining';
import type { JourneyLegDetail } from '@/lib/types';

/** What the journey page knows about a leg for these actions. */
export interface LegReplanContext {
  /** The first train leg not yet travelled: it can be missed. */
  missable: boolean;
  /** The journey's open break, if any. */
  openBreakId: number | null;
  /** This leg is the open break's trigger. */
  isBreakTrigger: boolean;
  /** The owner muted suggestions for this journey. */
  suggestionsMuted: boolean;
}

/** A train leg's mid-journey actions, for the owner (spec §8.2):
 *
 * - "I missed this train" on the first train leg not yet travelled (never
 *   a travelled or cancelled one): no confirm, as nothing is written; it
 *   opens the sheet with the other ways from here, and only taking one
 *   records the train as missed. Closing the sheet leaves the journey as
 *   it was: no break, so no "You missed the ..." banner;
 * - "Find another way" on a leg whose train is cancelled or which broke
 *   the plan: the open break's suggestions, or other trains for a
 *   cancelled leg when no break is open yet.
 *
 * "Change train" (phase 0) stays beside these in `JourneyLegCard`. */
export function LegReplanActions({
  journeyId,
  leg,
  context,
}: {
  journeyId: number;
  leg: JourneyLegDetail;
  context: LegReplanContext;
}) {
  const [mode, setMode] = useState<AlternativesMode | null>(null);
  const cancelled = leg.trackedTrainState?.status === 'cancelled';
  const showMissed = context.missable && !cancelled;
  const showFindAnother = cancelled || context.isBreakTrigger;
  if (!showMissed && !showFindAnother) return null;

  const facts = {
    departure: legDepartureInstant(leg),
    trainDestinationName: leg.trackedTrainState?.scheduleDestinationName ?? null,
    legDestinationName: leg.destinationName,
    stationName: null,
  };
  const findAnother: AlternativesMode =
    context.openBreakId !== null
      ? { kind: 'break', breakId: context.openBreakId }
      : { kind: 'alternatives', legId: leg.id, cause: 'cancelled' };
  const fallbackTitle =
    mode?.kind === 'missed'
      ? breakTitle({ ...facts, cause: 'user_missed' })
      : breakTitle({ ...facts, cause: cancelled ? 'cancelled' : null });

  return (
    <>
      {showMissed && (
        <Button
          size="xs"
          variant="default"
          onClick={() => setMode({ kind: 'missed', legId: leg.id, inboundLate: leg.connection?.state === 'missed' })}
        >
          I missed this train
        </Button>
      )}
      {showFindAnother && (
        <Button size="xs" variant="light" onClick={() => setMode(findAnother)}>
          Find another way
        </Button>
      )}
      {mode !== null && (
        <AlternativesSheet
          journeyId={journeyId}
          mode={mode}
          opened
          onClose={() => setMode(null)}
          fallbackTitle={fallbackTitle}
          canMute={mode.kind !== 'alternatives' && !context.suggestionsMuted}
        />
      )}
    </>
  );
}
