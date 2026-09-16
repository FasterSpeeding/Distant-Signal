/** How this app names another user to you: the group member list, the
 * "Shared by ..." credit on a group-shared train, a group's owner.
 *
 * The backend decides WHETHER there is a name to show and never sends an
 * email address as one (`crates/api/src/data/users.rs`'s `display_label`:
 * `users.name`, else `users.username`, else nothing -- an email-shaped
 * value in either field is declined rather than rendered). This module
 * owns the other half: what to render when the answer is "nothing", which
 * is a wording-and-capitalisation decision and so belongs on this side.
 *
 * The suffix is the fix for a group whose identity provider gives this app
 * no showable name for ANYONE. On Entra ID / Azure AD `preferred_username`
 * is the UPN and is therefore email-shaped for essentially every account,
 * so name and username are both declined for every member and every row
 * used to read as the identical "A member" -- correct on privacy, useless
 * for telling an eight-person group apart, and actively in the way of an
 * admin working out who shared a train or who to remove. `displayTag` is
 * six hex characters derived from the member's own opaque user id (which
 * these payloads already carry in the clear as `userId`/`addedBy`), so the
 * rows differ without anything about the person being revealed. */

/** The generic placeholder, standing alone (a member row's whole label). */
export const MEMBER_PLACEHOLDER = 'A member';

/** The same placeholder mid-sentence ("Shared by a member"). */
export const MEMBER_PLACEHOLDER_INLINE = 'a member';

/**
 * `displayName` when there is one, else the placeholder, suffixed with the
 * tag when the backend sent one.
 *
 * `?.trim() ||`, not `??`: a member whose identity provider has no name on
 * file for them can reach here as a BLANK `displayName` rather than a null
 * one. The backend normalizes that to null on both read and write now, but
 * rows written before it did still exist, and `??` would happily render
 * the empty string as a row's entire label.
 *
 * A member the app CAN name is returned exactly as before -- unsuffixed,
 * whatever `displayTag` says. The backend only ever sends a tag alongside
 * a null name, and this belt-and-braces ordering means a future backend
 * bug that sent both could not start decorating real people's names.
 */
export function memberLabel(
  displayName: string | null | undefined,
  displayTag: string | null | undefined,
  placeholder: string = MEMBER_PLACEHOLDER,
): string {
  const name = displayName?.trim();
  if (name) return name;
  const tag = displayTag?.trim();
  return tag ? `${placeholder} (#${tag})` : placeholder;
}
