'use client';

import { useId, useState, useSyncExternalStore } from 'react';
import { Stack, Switch, Text } from '@mantine/core';
import { describeFailure, failureFromResponse } from '@/lib/failure';
import {
  analyticsSession,
  consentVersion,
  privacySignalSet,
  readDeviceOptIn,
  setAccountOptIn,
  subscribeToConsent,
  writeDeviceOptIn,
} from '@/lib/usageAnalytics';

/** What the switch counts, in the switch's own description. */
export const ANALYTICS_OPT_IN_DESCRIPTION =
  'Send counts of which kinds of pages and features are used, to help us improve them. No cookies and nothing that identifies you: we only keep daily totals. Off unless you turn it on, and turning it off stops any more counts.';

/** Shown under the switch when the browser asks not to be tracked (Global
 * Privacy Control or Do Not Track). */
export const PRIVACY_SIGNAL_NOTE = 'Your browser asks sites not to track you, so nothing is sent.';

const serverVersion = () => 0;
const noSubscription = () => () => undefined;
const onClient = () => true;
const onServer = () => false;

/** "Help improve Distant Signal" (DS-190): the opt-in for usage counts,
 * off by default. On /account/preferences and /cookies.
 *
 * - Logged in: the account's choice (`PUT /api/preferences/analytics`). A
 *   failed save puts the switch back and says so.
 * - Logged out: this browser's choice (`lib/usageAnalytics.ts`), read
 *   after mount so the server render and the first client render agree.
 * - A browser sending Global Privacy Control or Do Not Track: the switch is
 *   off and disabled, and says why. Nothing is sent whatever the choice.
 *
 * `session` is the page's own session (/account/preferences has it). A
 * page without one (/cookies renders without a session fetch) leaves it
 * out, and the switch follows what `UsageTracker` learned from the nav's
 * session; it stays disabled until that is known, so a logged-in visitor's
 * choice never lands in this browser by mistake. */
export function AnalyticsOptIn({ session }: { session?: { signedIn: boolean; accountOptIn: boolean } }) {
  // Re-render when the tracker learns the session or the choice changes.
  useSyncExternalStore(subscribeToConsent, consentVersion, serverVersion);
  const live = analyticsSession();
  const signedIn = session?.signedIn ?? live.signedIn;
  const accountOptIn = session ? session.accountOptIn : live.accountOptIn;

  // Browser-only reads, false in the server render and the hydrating one,
  // so both agree; `writeDeviceOptIn` notifies `subscribeToConsent`.
  const mounted = useSyncExternalStore(noSubscription, onClient, onServer);
  const blocked = useSyncExternalStore(noSubscription, privacySignalSet, onServer);
  const deviceOptIn = useSyncExternalStore(subscribeToConsent, readDeviceOptIn, onServer);
  const [accountChoice, setAccountChoice] = useState<boolean | null>(null);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const enabled = signedIn === true ? (accountChoice ?? accountOptIn) : deviceOptIn;

  async function change(next: boolean) {
    setError(null);
    if (signedIn !== true) {
      if (!writeDeviceOptIn(next)) {
        setError(describeFailure('save', 'your choice in this browser'));
      }
      return;
    }
    setAccountChoice(next);
    setSaving(true);
    try {
      const response = await fetch('/api/preferences/analytics', {
        method: 'PUT',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ optIn: next }),
      });
      if (response.ok) {
        setAccountOptIn(next);
      } else {
        setAccountChoice(!next);
        setError(await failureFromResponse('save', 'your choice', response));
      }
    } catch {
      setAccountChoice(!next);
      setError(describeFailure('save', 'your choice'));
    } finally {
      setSaving(false);
    }
  }

  const where = signedIn === true ? 'Saved to your account.' : 'Saved in this browser.';
  // The description sits outside the switch's label, so the switch's
  // accessible name stays its short label (Mantine's own `description`
  // lands inside the <label>).
  const descriptionId = useId();
  const noteId = useId();
  return (
    <Stack gap={4} data-analytics-opt-in>
      <Switch
        label="Help improve Distant Signal"
        aria-describedby={blocked ? `${descriptionId} ${noteId}` : descriptionId}
        checked={enabled && !blocked}
        disabled={saving || blocked || !mounted || signedIn === null}
        onChange={(event) => void change(event.currentTarget.checked)}
      />
      <Text id={descriptionId} size="sm" c="dimmed">
        {ANALYTICS_OPT_IN_DESCRIPTION} {where}
      </Text>
      {blocked && (
        <Text id={noteId} size="sm">
          {PRIVACY_SIGNAL_NOTE}
        </Text>
      )}
      {error && (
        <Text size="sm" c="var(--ds-color-error-text)" role="alert">
          {error}
        </Text>
      )}
    </Stack>
  );
}
