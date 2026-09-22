import { Stack, Title, Text } from '@mantine/core';
import type { Metadata } from 'next';
import { TrackTrainForm } from '@/components/TrackTrainForm';

/** Per-page Open Graph/Twitter/`<title>` metadata, in the same four-field
 * shape every detail page in this app already emits (see
 * `app/train/[uid]/[date]/page.tsx`'s `generateMetadata` for the canonical
 * version, and `app/page.tsx`'s own static export for why these top-level
 * pages spell it as a plain `export const metadata` instead).
 *
 * Static rather than an async `generateMetadata()` despite this route
 * reading `searchParams`, for exactly the reason `/trains` gives: Next
 * hands `generateMetadata` the same `searchParams` the component below
 * gets, and `?ticketId=` here is one visitor's own saved ticket -- putting
 * it (or their `?origin=`) into a title would leak per-visitor state into
 * a cached, shared unfurl card for no gain whatsoever. The `<h1>`'s
 * subtitle below DOES vary with `attachTicketId`; this deliberately
 * describes the page's ordinary, shareable purpose instead, which is also
 * the branch an unfurler bot (never carrying either param) actually gets.
 *
 * Title matches the page's own `<h1>` ("Track a Train"). This route has no
 * nav label of its own to agree with -- it was dropped from the nav bar in
 * favour of `/trains` (see `app/layout.tsx`'s comment there) and is now
 * reached from `/trains`' manual-fallback link, `/stations/[crs]`, and
 * `TicketEntryForm` -- so the heading is the only name it has.
 *
 * The description's second half is the page's own default subtitle below,
 * near-verbatim, so the preview card and the page a visitor lands on say
 * the same thing. Its first half describes both ways the form can be
 * filled: picking a row from the departures `TrackTrainForm` loads for the
 * entered origin, or typing origin/scheduled departure (required) and
 * destination/operator (optional) by hand. Deliberately "upcoming
 * departures" rather than "the live departure board": that picker falls
 * back to the CIF-derived scheduled timetable at any station LDBWS has no
 * board for, and says so in its own copy. */
const METADATA_TITLE = 'Track a Train — Distant Signal';
const METADATA_DESCRIPTION =
  'Pin a specific train — picked from the upcoming departures at its origin station, or entered by hand — to see its live position, delay and next calling point as Network Rail reports it. Not sure which train yet? Search a time window instead and pick from the matches.';

export const metadata: Metadata = {
  title: METADATA_TITLE,
  description: METADATA_DESCRIPTION,
  openGraph: { title: METADATA_TITLE, description: METADATA_DESCRIPTION, type: 'website' },
  twitter: { card: 'summary', title: METADATA_TITLE, description: METADATA_DESCRIPTION },
};

export default async function TrackPage({
  searchParams,
}: {
  searchParams: Promise<{
    origin?: string | string[];
    ticketId?: string | string[];
    mode?: string | string[];
    destination?: string | string[];
    departAfter?: string | string[];
    departBefore?: string | string[];
    arriveAfter?: string | string[];
    arriveBefore?: string | string[];
  }>;
}) {
  const { origin, ticketId, mode, destination, departAfter, departBefore, arriveAfter, arriveBefore } =
    await searchParams;
  // Next.js supplies a `string[]` for a repeated query param (e.g.
  // `?origin=a&origin=b`) -- fall back to the first value rather than
  // letting `.toUpperCase()` throw on an array.
  const originParam = Array.isArray(origin) ? origin[0] : origin;
  // Set by `TicketEntryForm`'s own "find or track the train this ticket is
  // for" link (Part A of the upload-first plan) -- a standalone ticket's
  // id, carried forward so `TrackTrainForm` can attach it automatically
  // once a pin is created here. A malformed/non-numeric value is treated
  // the same as absent, rather than passing NaN through.
  const ticketIdParam = Array.isArray(ticketId) ? ticketId[0] : ticketId;
  const attachTicketId = ticketIdParam && /^\d+$/.test(ticketIdParam) ? Number(ticketIdParam) : undefined;
  // Review §2.1/I21: previously nothing in the app could link straight to
  // window mode -- it lived only in `TrackTrainForm`'s own `useState`, not
  // URL-addressable at all (unlike `?origin=` just above, which already had
  // this exact pattern). `JourneyLegCard`'s "Edit search" link
  // (`components/JourneyLegCard.tsx`) is the first real caller. Anything
  // other than the literal string falls back to pick mode, the same
  // "malformed means absent" posture `ticketIdParam` takes above.
  const modeParam = Array.isArray(mode) ? mode[0] : mode;
  const initialMode = modeParam === 'window' ? 'window' : 'pick';
  // Same "repeated query param -> first value" unwrapping `origin`/
  // `ticketId`/`mode` already use just above -- applied uniformly to the
  // five new params `TrackJourneyAgainButton`/`trackAgainHref`
  // (docs/superpowers/plans/2026-09-22-reusable-journeys-phaseA-track-again-plan.md)
  // introduce, so a malformed/repeated value degrades the same way a
  // malformed `?origin=` already does rather than throwing.
  const destinationParam = Array.isArray(destination) ? destination[0] : destination;
  const departAfterParam = Array.isArray(departAfter) ? departAfter[0] : departAfter;
  const departBeforeParam = Array.isArray(departBefore) ? departBefore[0] : departBefore;
  const arriveAfterParam = Array.isArray(arriveAfter) ? arriveAfter[0] : arriveAfter;
  const arriveBeforeParam = Array.isArray(arriveBefore) ? arriveBefore[0] : arriveBefore;
  // Malformed means absent, same posture `ticketIdParam` above already
  // takes for its own format check -- a native `<input type="time">`
  // sanitizes an invalid value to an empty DISPLAY while React state would
  // still hold the raw garbage, so an unvalidated value here could reach
  // `TrackTrainForm`'s state (and from there, submit) in a shape no normal
  // user interacting with the form could ever produce by hand.
  const validTimeParam = (v: string | undefined): string | undefined =>
    v && /^\d{2}:\d{2}$/.test(v) ? v : undefined;

  return (
    <Stack p="lg" gap="md">
      <Title order={1}>Track a Train</Title>
      {/* Review §2.1/I21: the mode-aware "Pin a specific train…"/"Not sure
          which train yet?" sentence now lives inside `TrackTrainForm`
          itself, where it can react to the client-side mode toggle -- a
          visitor who switches modes without reloading the page used to
          keep seeing copy that only described pick mode. The ticket-attach
          sentence stays here: it's mode-independent (attaching a ticket
          only ever follows the pin-mode path) and needs no reactivity. */}
      {attachTicketId !== undefined && (
        <Text c="dimmed">
          Find or track the train your saved ticket is for — it&apos;ll be attached automatically once you do.
        </Text>
      )}
      {/* Review §2.16: `TrackTrainForm`'s own "Track this train" button is
          shown to every visitor, logged in or not (the Tier-2 "show the
          control, gate on the real 401" pattern `useNeedsLogin.ts`
          documents), which gives no hint up front that saving a pin needs
          an account -- same complaint the review makes of the anonymous pin
          star (§3.4). Unconditional rather than gated on `getSession()`: it
          stays true for a logged-in visitor too, and this route mounts a
          client form regardless, so there is no static-rendering property
          to preserve the way `/lines/new`'s own comment protects. */}
      <Text size="sm" c="dimmed">
        Tracking a train needs a Distant Signal account — you&apos;ll be sent to log in when you save if you
        aren&apos;t already signed in.
      </Text>
      <TrackTrainForm
        initialOrigin={originParam?.toUpperCase()}
        initialDestination={destinationParam?.toUpperCase()}
        attachTicketId={attachTicketId}
        initialMode={initialMode}
        initialDepartAfter={validTimeParam(departAfterParam)}
        initialDepartBefore={validTimeParam(departBeforeParam)}
        initialArriveAfter={validTimeParam(arriveAfterParam)}
        initialArriveBefore={validTimeParam(arriveBeforeParam)}
      />
    </Stack>
  );
}
