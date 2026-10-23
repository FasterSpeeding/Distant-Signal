import { memberLabel, MEMBER_PLACEHOLDER_INLINE } from './memberLabel';
import type { FollowedFrom } from './types';

/** The label on a journey the caller follows rather than owns (DS-146):
 * "Shared by Olive" for a group share, by the same member label rules as
 * a group's own "Shared by" credit (`lib/memberLabel.ts`). A share link
 * shows nothing about its owner, so a journey followed through one says
 * only how it was shared. */
export function followedFromLabel(from: FollowedFrom): string {
  if (from.via === 'link') return 'Shared with you by link';
  return `Shared by ${memberLabel(from.ownerName, from.ownerTag, MEMBER_PLACEHOLDER_INLINE)}`;
}
