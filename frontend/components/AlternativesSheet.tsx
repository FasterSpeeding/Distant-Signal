'use client';

import { useCallback, useEffect, useRef, useState } from 'react';
import { useRouter } from 'next/navigation';
import {
  Accordion,
  AccordionControl,
  AccordionItem,
  AccordionPanel,
  Alert,
  Box,
  Button,
  Card,
  Drawer,
  Group,
  Modal,
  Popover,
  PopoverDropdown,
  PopoverTarget,
  Stack,
  Text,
  UnstyledButton,
  VisuallyHidden,
  useMantineTheme,
} from '@mantine/core';
import { useMediaQuery } from '@mantine/hooks';
import { ButtonSpinner, LoadingText } from './LoadingText';
import { StateBadge } from './StateBadge';
import { instantLateDepartureTimes, LateDepartureNote, trainToSubject } from './LateDepartureNote';
import { asSentence, describeFailure, isUserFacingApiMessage } from '@/lib/failure';
import { formatTime } from '@/lib/dateFormat';
import {
  BOOKED_PLAN_SAVED_NOTE,
  LATE_INBOUND_NOTE,
  STALE_SUGGESTION_MESSAGE,
  UNDO_WINDOW_MS,
  breakTitle,
  suggestionBadges,
  suggestionDelay,
  suggestionLegBody,
  suggestionLegLines,
  suggestionSetBody,
  suggestionSetTitleFacts,
  suggestionSummary,
  type SuggestionBadge,
} from '@/lib/journeyBreak';
import type {
  AlternativesCause,
  BreakCause,
  BreakResponse,
  ReplanResult,
  ReplanSuggestion,
  SuggestionSet,
} from '@/lib/types';

/** What the sheet shows suggestions for:
 *
 * - `break`: the journey's open break, its stored suggestions
 *   (`GET .../breaks/current`), refreshed when stale;
 * - `missed`: "I missed this train": the other ways from that leg
 *   (`GET .../legs/{id}/alternatives?cause=missed`). Looking writes
 *   nothing; only taking one (`POST .../legs/{id}/replace`, `cause:
 *   missed`) records the train as missed. `inboundLate`: the leg's
 *   connection state says the train before it arrived too late, so the
 *   miss is the railway's, not the traveller's;
 * - `alternatives`: other ways from one leg with no break
 *   (`GET .../legs/{id}/alternatives?cause=`), taken with `POST
 *   .../legs/{id}/replace`. */
export type AlternativesMode =
  | { kind: 'break'; breakId: number; fromPush?: boolean }
  | { kind: 'missed'; legId: number; inboundLate?: boolean }
  | { kind: 'alternatives'; legId: number; cause: AlternativesCause };

/** The same disclosure as `WorkingTimetable`'s: a flush `Accordion`. */
const DISCLOSURE_STYLES = {
  item: { border: 'none', backgroundColor: 'transparent' },
  control: { padding: 0, paddingBlock: 'var(--mantine-spacing-xs)' },
  chevron: { marginInlineEnd: 'var(--mantine-spacing-xs)' },
  panel: { paddingInlineStart: 0 },
  content: { paddingInline: 0 },
} as const;

/** Every failure here reads "Couldn't load other trains." or "Couldn't
 * update the journey." (spec §8.3). */
const LOAD_FAILURE = ['load', 'other trains'] as const;
const UPDATE_FAILURE = ['update', 'the journey'] as const;

/** A refusal's message: the API's own sentence when its body (plain text,
 * or JSON with a `message`) is one written for people and the status is
 * one a person can act on, else `describeFailure`. */
async function failureMessage([verb, noun]: readonly [string, string], response: Response): Promise<string> {
  const body = await response.text().catch(() => '');
  if ([400, 403, 409, 422].includes(response.status)) {
    let message = body;
    try {
      const parsed: unknown = JSON.parse(body);
      if (parsed && typeof parsed === 'object' && 'message' in parsed && typeof parsed.message === 'string') {
        message = parsed.message;
      }
    } catch {
      // Plain text: the body is the message.
    }
    if (isUserFacingApiMessage(message)) return asSentence(message);
  }
  return describeFailure(verb, noun, response.status);
}

type Fetched<T> = { ok: true; value: T } | { ok: false; message: string };

async function fetchJson<T>(url: string, init: RequestInit, failure: readonly [string, string]): Promise<Fetched<T>> {
  try {
    const response = await fetch(url, init);
    if (!response.ok) return { ok: false, message: await failureMessage(failure, response) };
    return { ok: true, value: (await response.json()) as T };
  } catch {
    return { ok: false, message: describeFailure(...failure) };
  }
}

const JSON_HEADERS = { 'Content-Type': 'application/json' };

/** Fresh suggestions for a break (`POST .../replan/preview`). */
function previewBreak(journeyId: number, breakId: number): Promise<Fetched<BreakResponse>> {
  return fetchJson<BreakResponse>(
    `/api/Journeys/${journeyId}/replan/preview`,
    { method: 'POST', headers: JSON_HEADERS, body: JSON.stringify({ breakId }) },
    ['update', 'these trains'],
  );
}

/** The request that loads the sheet, by mode. */
function loadRequest(journeyId: number, mode: AlternativesMode): { url: string; method: 'GET' | 'POST' } {
  const base = `/api/Journeys/${journeyId}`;
  switch (mode.kind) {
    case 'break':
      return { url: `${base}/breaks/current${mode.fromPush ? '?src=push' : ''}`, method: 'GET' };
    case 'missed':
      return { url: `${base}/legs/${mode.legId}/alternatives?cause=missed`, method: 'GET' };
    case 'alternatives':
      return { url: `${base}/legs/${mode.legId}/alternatives?cause=${mode.cause}`, method: 'GET' };
  }
}

/** The cause a mode stands for before the break (if any) says. "I
 * missed this train" stands for none: the sheet is titled "Other ways to
 * ..." while the traveller only looks, never "You missed the ...". */
function modeCause(mode: AlternativesMode): BreakCause | null {
  if (mode.kind === 'alternatives') {
    if (mode.cause === 'cancelled') return 'cancelled';
    if (mode.cause === 'connection') return 'connection_missed';
  }
  return null;
}

/** The suggestions sheet (spec §8.3, §17.6): a bottom `Drawer` below `sm`
 * and a `Modal` above. Up to three cards (Recommended, then the other
 * picks), "More options" for the rest, and "Take this one" on each, which
 * is the whole accept: no confirm, because Undo follows for 30 seconds.
 * The page refreshes once Undo is gone or the sheet is closed. Only ever
 * opened for the journey's owner. */
export function AlternativesSheet({
  journeyId,
  mode,
  opened,
  onClose,
  fallbackTitle,
  stationName = null,
  canMute = false,
}: {
  journeyId: number;
  mode: AlternativesMode;
  opened: boolean;
  onClose: () => void;
  /** The title until the suggestions say more (the banner's own). */
  fallbackTitle: string;
  /** The break's station, for "Connection at York missed". */
  stationName?: string | null;
  /** Offer "Stop suggestions for this journey". */
  canMute?: boolean;
}) {
  const router = useRouter();
  const theme = useMantineTheme();
  const isPhone = useMediaQuery(`(max-width: calc(${theme.breakpoints.sm} - 0.0625em))`);
  const [title, setTitle] = useState(fallbackTitle);
  // Whether the journey changed while the sheet was open, so closing it
  // shows the new plan.
  const changed = useRef(false);
  const markChanged = useCallback((isChanged: boolean) => {
    changed.current = isChanged;
  }, []);

  function close() {
    onClose();
    if (changed.current) {
      changed.current = false;
      router.refresh();
    }
  }

  const body = opened && (
    <AlternativesSheetBody
      journeyId={journeyId}
      mode={mode}
      stationName={stationName}
      canMute={canMute}
      fallbackTitle={fallbackTitle}
      onTitle={setTitle}
      onChanged={markChanged}
      onDone={close}
    />
  );

  return isPhone ? (
    <Drawer
      opened={opened}
      onClose={close}
      position="bottom"
      title={title}
      classNames={{ content: 'listFilterSheet', title: 'listFilterSheetTitle' }}
    >
      {body}
    </Drawer>
  ) : (
    <Modal opened={opened} onClose={close} title={title} size="lg">
      {body}
    </Modal>
  );
}

/** The sheet's contents, mounted each time it opens so every opening
 * starts from a fresh load. */
function AlternativesSheetBody({
  journeyId,
  mode,
  stationName,
  canMute,
  fallbackTitle,
  onTitle,
  onChanged,
  onDone,
}: {
  journeyId: number;
  mode: AlternativesMode;
  stationName: string | null;
  canMute: boolean;
  fallbackTitle: string;
  onTitle: (title: string) => void;
  onChanged: (changed: boolean) => void;
  onDone: () => void;
}) {
  const router = useRouter();
  const [status, setStatus] = useState<'loading' | 'ready' | 'error'>('loading');
  const [loadError, setLoadError] = useState<string | null>(null);
  const [breakInfo, setBreakInfo] = useState<BreakResponse | null>(null);
  const [set, setSet] = useState<SuggestionSet | null>(null);
  const [updating, setUpdating] = useState(false);
  const [updateError, setUpdateError] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const [takingId, setTakingId] = useState<string | null>(null);
  const [actionError, setActionError] = useState<string | null>(null);
  const [accepted, setAccepted] = useState<ReplanResult | null>(null);
  const [undoOpen, setUndoOpen] = useState(false);
  const [undoing, setUndoing] = useState(false);
  const [undone, setUndone] = useState(false);
  const [muting, setMuting] = useState(false);
  const [muted, setMuted] = useState(false);
  const [muteError, setMuteError] = useState<string | null>(null);

  const { url, method } = loadRequest(journeyId, mode);
  // Neither "I missed this train" nor other ways from a leg has a break:
  // both load a suggestion set and write nothing until one is taken.
  const isAlternatives = mode.kind === 'alternatives' || mode.kind === 'missed';
  const cause = breakInfo?.cause ?? modeCause(mode);

  useEffect(() => {
    let cancelled = false;
    // A function, not the flag itself: TypeScript would keep the flag
    // narrowed to `false` across the awaits below.
    const isCancelled = () => cancelled;
    void (async () => {
      if (isAlternatives) {
        const result = await fetchJson<SuggestionSet>(url, { method }, LOAD_FAILURE);
        if (isCancelled()) return;
        if (!result.ok) {
          setLoadError(result.message);
          setStatus('error');
          return;
        }
        setSet(result.value);
        setStatus('ready');
        return;
      }
      const result = await fetchJson<BreakResponse>(url, { method }, LOAD_FAILURE);
      if (isCancelled()) return;
      if (!result.ok) {
        setLoadError(result.message);
        setStatus('error');
        return;
      }
      const loaded = result.value;
      setBreakInfo(loaded);
      setSet(loaded.suggestions);
      setStatus('ready');
      // Stored suggestions show at once, dimmed, while fresh ones are
      // planned (spec §8.3).
      if (!loaded.stale && loaded.suggestions !== null && !loaded.suggestions.stale) return;
      setUpdating(true);
      const fresh = await previewBreak(journeyId, loaded.id);
      if (isCancelled()) return;
      setUpdating(false);
      if (!fresh.ok) {
        setUpdateError(fresh.message);
        return;
      }
      setBreakInfo(fresh.value);
      setSet(fresh.value.suggestions);
    })();
    return () => {
      cancelled = true;
    };
  }, [journeyId, url, method, isAlternatives]);

  // The title follows the suggestions, which know the trigger train.
  const title = set ? breakTitle(suggestionSetTitleFacts(set, cause, stationName)) : fallbackTitle;
  useEffect(() => {
    onTitle(title);
  }, [onTitle, title]);

  // Undo is offered for 30 seconds; then the page shows the new plan.
  useEffect(() => {
    if (!undoOpen) return;
    const timer = setTimeout(() => {
      setUndoOpen(false);
      onChanged(false);
      router.refresh();
    }, UNDO_WINDOW_MS);
    return () => clearTimeout(timer);
  }, [undoOpen, onChanged, router]);

  async function take(suggestion: ReplanSuggestion) {
    if (set === null) return;
    setTakingId(suggestion.id);
    setActionError(null);
    setNotice(null);
    const headers = { ...JSON_HEADERS, 'Idempotency-Key': crypto.randomUUID() };
    const request =
      breakInfo !== null
        ? fetch(`/api/Journeys/${journeyId}/replan/accept`, {
            method: 'POST',
            headers,
            body: JSON.stringify({ breakId: breakInfo.id, suggestionId: suggestion.id, fingerprint: set.fingerprint }),
          })
        : fetch(`/api/Journeys/${journeyId}/legs/${set.fromLegId}/replace`, {
            method: 'POST',
            headers,
            body: JSON.stringify({
              cause: mode.kind === 'alternatives' ? mode.cause : 'missed',
              legs: suggestion.legs.map(suggestionLegBody),
              throughLegId: suggestion.throughLegId,
            }),
          });
    try {
      const response = await request;
      if (response.status === 409) {
        const stale = await staleBreak(response.clone());
        if (stale !== null) {
          setBreakInfo(stale);
          setSet(stale.suggestions);
          setNotice(STALE_SUGGESTION_MESSAGE);
          return;
        }
      }
      if (!response.ok) {
        setActionError(await failureMessage(UPDATE_FAILURE, response));
        return;
      }
      const result = (await response.json()) as ReplanResult;
      setAccepted(result);
      setUndoOpen(result.undoRevisionNo !== null);
      onChanged(true);
    } catch {
      setActionError(describeFailure(...UPDATE_FAILURE));
    } finally {
      setTakingId(null);
    }
  }

  async function undo() {
    if (accepted?.undoRevisionNo == null) return;
    setUndoing(true);
    setActionError(null);
    try {
      const response = await fetch(`/api/Journeys/${journeyId}/revisions/${accepted.undoRevisionNo}/revert`, {
        method: 'POST',
      });
      if (!response.ok) {
        setActionError(await failureMessage(['undo', 'this change'], response));
        return;
      }
      setUndoOpen(false);
      setUndone(true);
      onChanged(true);
    } catch {
      setActionError(describeFailure('undo', 'this change'));
    } finally {
      setUndoing(false);
    }
  }

  async function mute() {
    setMuting(true);
    setMuteError(null);
    try {
      const response = await fetch(`/api/Journeys/${journeyId}/suggestions-muted`, {
        method: 'PUT',
        headers: JSON_HEADERS,
        body: JSON.stringify({ muted: true }),
      });
      if (!response.ok) {
        setMuteError(await failureMessage(['stop', 'suggestions'], response));
        return;
      }
      setMuted(true);
    } catch {
      setMuteError(describeFailure('stop', 'suggestions'));
    } finally {
      setMuting(false);
    }
  }

  if (status === 'loading') {
    return (
      <Box role="status" aria-busy="true">
        <LoadingText>Loading trains</LoadingText>
      </Box>
    );
  }

  if (status === 'error') {
    return (
      <Alert color="red" variant="light" role="alert" title="Couldn't load other trains">
        {loadError}
      </Alert>
    );
  }

  if (accepted !== null) {
    return (
      <Stack gap="sm">
        {undone ? (
          <Alert color="green" variant="light" role="status" title="Your earlier plan is back." />
        ) : (
          <Alert color="green" variant="light" role="status" title="Journey updated. Booked plan saved.">
            {undoOpen && (
              <Button
                size="sm"
                variant="default"
                mt="xs"
                onClick={() => void undo()}
                disabled={undoing}
                {...(undoing && { leftSection: <ButtonSpinner /> })}
              >
                {undoing ? 'Undoing' : 'Undo'}
              </Button>
            )}
          </Alert>
        )}
        {actionError && (
          <Text c="var(--ds-color-error-text)" role="alert">
            {actionError}
          </Text>
        )}
        <Group justify="flex-end">
          <Button size="sm" onClick={onDone}>
            Done
          </Button>
        </Group>
      </Stack>
    );
  }

  const suggestions = set?.suggestions ?? [];
  const cards = suggestions.filter((suggestion) => suggestion.card);
  const rest = suggestions.filter((suggestion) => !suggestion.card);
  // Why a miss counts as the railway's: the break's note, else (nothing
  // recorded yet) the leg's connection state, as the api decides it (§4.2).
  const inboundLate = mode.kind === 'missed' && mode.inboundLate === true;
  const faultNote = breakInfo !== null ? breakInfo.faultNote : inboundLate ? LATE_INBOUND_NOTE : null;

  const card = (suggestion: ReplanSuggestion) => (
    <SuggestionCard
      key={suggestion.id}
      suggestion={suggestion}
      set={set}
      busy={takingId === suggestion.id}
      disabled={takingId !== null || updating}
      onTake={() => void take(suggestion)}
    />
  );

  return (
    <Stack gap="sm">
      {set && <Text size="sm">{suggestionSetBody(set)}</Text>}
      {notice && (
        <Alert color="yellow" variant="light" role="status">
          {notice}
        </Alert>
      )}
      {updating && (
        <Box role="status">
          <LoadingText>Updating</LoadingText>
        </Box>
      )}
      {updateError && (
        <Text size="sm" c="var(--ds-color-error-text)" role="alert">
          {updateError}
        </Text>
      )}
      {actionError && (
        <Text c="var(--ds-color-error-text)" role="alert">
          {actionError}
        </Text>
      )}
      <Stack
        gap="sm"
        aria-busy={updating}
        opacity={updating ? 0.55 : 1}
        data-suggestions
        data-updating={updating || undefined}
      >
        {set === null && !updating && (
          <Text size="sm" c="dimmed">
            No other trains found yet.
          </Text>
        )}
        {cards.map(card)}
        {rest.length > 0 && (
          <Accordion chevronPosition="left" keepMounted={false} styles={DISCLOSURE_STYLES}>
            <AccordionItem value="more-options">
              <AccordionControl>More options ({rest.length})</AccordionControl>
              <AccordionPanel>
                <Stack gap="sm">{rest.map(card)}</Stack>
              </AccordionPanel>
            </AccordionItem>
          </Accordion>
        )}
      </Stack>
      {faultNote && <Text size="sm">{faultNote}</Text>}
      {/* The api's ticket advice (§17.5): station staff first unless the
          ticket is flexible, worded by who broke the journey. */}
      {set?.hint && (
        <Text size="sm" data-ticket-advice>
          {set.hint}
        </Text>
      )}
      <Text size="sm" c="dimmed">
        {BOOKED_PLAN_SAVED_NOTE}
      </Text>
      {canMute &&
        (muted ? (
          <Text size="sm" role="status">
            Suggestions muted for this journey.
          </Text>
        ) : (
          <Group>
            <Button
              size="sm"
              variant="subtle"
              onClick={() => void mute()}
              disabled={muting}
              {...(muting && { leftSection: <ButtonSpinner /> })}
            >
              Stop suggestions for this journey
            </Button>
          </Group>
        ))}
      {muteError && (
        <Text size="sm" c="var(--ds-color-error-text)" role="alert">
          {muteError}
        </Text>
      )}
    </Stack>
  );
}

/** The fresh break in a `409 suggestion_stale`, or `null` for any other
 * conflict. */
async function staleBreak(response: Response): Promise<BreakResponse | null> {
  try {
    const body: unknown = await response.json();
    if (
      body &&
      typeof body === 'object' &&
      'error' in body &&
      body.error === 'suggestion_stale' &&
      'break' in body &&
      body.break &&
      typeof body.break === 'object'
    ) {
      return body.break as BreakResponse;
    }
  } catch {
    // Not JSON: an ordinary conflict.
  }
  return null;
}

/** A pick or fit badge. A fit with a reason opens it on tap. */
function SuggestionBadgeView({ badge }: { badge: SuggestionBadge }) {
  const view = <StateBadge tone={badge.tone}>{badge.label}</StateBadge>;
  if (!badge.reason) return view;
  return (
    <Popover width={280} withArrow shadow="md" position="bottom-start">
      <PopoverTarget>
        <UnstyledButton className="ds-clickable" aria-label={`${badge.label}: why`}>
          {view}
        </UnstyledButton>
      </PopoverTarget>
      <PopoverDropdown>
        <Text size="sm">{badge.reason}</Text>
      </PopoverDropdown>
    </Popover>
  );
}

/** One way to carry on: its badges, the summary ("10:04 York to
 * Edinburgh, direct"), its times and delay against the booked arrival, its
 * legs when it has changes, and "Take this one". */
function SuggestionCard({
  suggestion,
  set,
  busy,
  disabled,
  onTake,
}: {
  suggestion: ReplanSuggestion;
  set: SuggestionSet | null;
  busy: boolean;
  disabled: boolean;
  onTake: () => void;
}) {
  const badges = suggestionBadges(suggestion);
  const delay = suggestionDelay(suggestion.delayMinutes);
  const summary = suggestionSummary(suggestion, set ?? undefined);
  const legLines = suggestion.legs.length > 1 ? suggestionLegLines(suggestion) : [];
  return (
    <Card withBorder padding="sm" data-suggestion={suggestion.id}>
      <Stack gap="xs">
        {badges.length > 0 && (
          <Group gap={6} wrap="wrap" data-badge-row>
            {badges.map((badge) => (
              <SuggestionBadgeView key={badge.label} badge={badge} />
            ))}
          </Group>
        )}
        <Text fw={600}>{summary}</Text>
        <Group gap="xs" wrap="wrap">
          <Text size="sm" c="dimmed">
            Departs {formatTime(suggestion.departure)}, arrives {formatTime(suggestion.arrival)}
          </Text>
          {delay && (
            <StateBadge tone={delay.tone} data-suggestion-delay={delay.tone}>
              <span aria-hidden="true">{delay.text}</span>
              <VisuallyHidden>{delay.spoken}</VisuallyHidden>
            </StateBadge>
          )}
        </Group>
        {legLines.length > 0 && (
          <Stack gap={0} component="ul" className="ds-plain-list">
            {legLines.map((line, index) => (
              <Text key={index} component="li" size="xs" c="dimmed">
                {line}
              </Text>
            ))}
          </Stack>
        )}
        {suggestion.legs.map((leg, index) =>
          leg.kind === 'train' && leg.reliesOnLateDeparture ? (
            <LateDepartureNote
              key={`late-${index}`}
              {...instantLateDepartureTimes(leg.reliesOnLateDeparture)}
              subject={trainToSubject(leg.destinationName)}
            />
          ) : null,
        )}
        <Group justify="flex-end">
          <Button
            size="sm"
            onClick={onTake}
            disabled={disabled}
            aria-label={`${busy ? 'Saving' : 'Take this one'}: ${summary}`}
            {...(busy && { leftSection: <ButtonSpinner /> })}
          >
            {busy ? 'Saving' : 'Take this one'}
          </Button>
        </Group>
      </Stack>
    </Card>
  );
}
