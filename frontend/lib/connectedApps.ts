import { formatDate } from './dateFormat';
import { journeyDisplayName, journeyNameInputFromLegs } from './journeyName';
import type { ConnectedAppDeletion, ConnectedAppScope } from './types';

/** Every scope, in the order the pages list them. */
export const CONNECTED_APP_SCOPES: readonly ConnectedAppScope[] = [
  'journeys:read',
  'journeys:write',
  'journeys:delete',
];

/** What each scope lets an app do, as the consent page and the Connected
 * apps list say it (docs/connected-apps.md). */
const SCOPE_SENTENCES: Record<ConnectedAppScope, string> = {
  'journeys:read': 'See your saved journeys',
  'journeys:write': 'Save journeys to your account',
  'journeys:delete': 'Delete journeys it saved, unless you have shared them',
};

export function isConnectedAppScope(scope: string): scope is ConnectedAppScope {
  return (CONNECTED_APP_SCOPES as readonly string[]).includes(scope);
}

/** The sentence for a scope; an unknown one is shown as its name. */
export function scopeSentence(scope: string): string {
  return isConnectedAppScope(scope) ? SCOPE_SENTENCES[scope] : scope;
}

/** The scopes a link asked to preselect (`?scopes=journeys:read journeys:write`,
 * space- or comma-separated), limited to those the app may have. Only
 * preselects boxes: the grant takes what the visitor submits. Every
 * allowed scope when the link names none. */
export function preselectedScopes(requested: string | undefined, allowed: readonly string[]): string[] {
  const asked = (requested ?? '').split(/[\s,]+/).filter((scope) => scope !== '');
  const picked = allowed.filter((scope) => asked.includes(scope));
  return picked.length > 0 ? picked : [...allowed];
}

/** A one-line description of a deleted journey: its name, then each leg's
 * stations, date and train, for re-saving it by hand. The name follows the
 * one journey-naming rule (`lib/journeyName.ts`): the custom name, else
 * first origin to last destination and the first day. The snapshot holds
 * station codes only, so the route is in codes ("EUS to MKC"). With no
 * legs to name it by, it is "Journey" and its number. */
export function deletionSummary(deletion: ConnectedAppDeletion): { title: string; legs: string[] } {
  const snapshotLegs = deletion.summary.legs ?? [];
  const title =
    snapshotLegs.length > 0
      ? journeyDisplayName(
          journeyNameInputFromLegs(
            deletion.summary.customName,
            snapshotLegs.map((leg) => ({
              originCrs: leg.originCrs ?? null,
              originName: null,
              destinationCrs: leg.destinationCrs ?? null,
              destinationName: null,
              serviceDate: leg.serviceDate ?? null,
            })),
          ),
        )
      : deletion.summary.customName?.trim() || `Journey ${deletion.journeyId}`;
  const legs = snapshotLegs.map((leg) => {
    const route = [leg.originCrs, leg.destinationCrs].filter(Boolean).join(' to ');
    const parts = [
      route || 'Unknown route',
      leg.serviceDate ? `on ${formatDate(`${leg.serviceDate}T12:00:00Z`)}` : null,
      leg.trainUid ? `train ${leg.trainUid}` : null,
    ];
    return parts.filter(Boolean).join(', ');
  });
  return { title, legs };
}
