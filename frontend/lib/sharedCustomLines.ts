import type { SharedGroupCustomLine } from './types';

/** One row of the home page's "Lines shared with you" section: the shared
 * custom line itself, plus EVERY group the caller can see it through. */
export interface MergedSharedCustomLine {
  line: SharedGroupCustomLine;
  /** Never empty, and de-duplicated -- one entry per distinct group the
   * caller shares with this line, in the order the API returned them
   * (newest grant first, see `list_shared_custom_lines_for_user`). */
  groupNames: string[];
}

/** Collapses `GET /public/groups/shared-custom-lines`'s one-row-per-(group,
 * line) wire shape into one row per line, carrying every group name that
 * line arrived through.
 *
 * The backend deliberately does NOT collapse this itself (see
 * `groups::list_shared_custom_lines_for_user`'s own doc comment): a line
 * granted into two of the caller's groups is genuinely two attributions,
 * and picking one server-side would silently drop a group name from the
 * tag. The other fields are identical across those rows by construction --
 * they all describe the same `custom_lines` row, and only its OWNER can
 * grant it (`groups::grant_custom_line` checks ownership), so `grantedBy`
 * can't differ between them either -- which is why keeping the first row's
 * copy of them is lossless rather than an arbitrary pick.
 *
 * `excludeLineIds` drops any line the calling page is ALREADY rendering
 * elsewhere, so it never appears twice on one screen. Two cases matter,
 * and they are different in kind:
 *
 * 1. **A line the caller pinned.** A granted member can pin a shared line
 *    like any other, and `pinned_lines` takes any id, so the home page's
 *    "Your Lines" section would render it from `allReports` and this
 *    section would render it again. The page passes its pinned set for
 *    exactly this reason -- and "Your Lines" is the right place for it,
 *    since the caller put it there deliberately.
 * 2. **A line the caller owns.** Belt-and-braces only: the backend's
 *    `cl.user_id <> $1` already excludes those
 *    (`list_shared_custom_lines_for_user`), so this only matters if that
 *    ever regresses.
 *
 * Mirrors `lib/sharedTrains.ts`'s `mergeSharedTrains` exactly. */
export function mergeSharedCustomLines(
  rows: SharedGroupCustomLine[],
  excludeLineIds: ReadonlySet<string> = new Set(),
): MergedSharedCustomLine[] {
  const merged = new Map<string, MergedSharedCustomLine>();
  for (const row of rows) {
    if (excludeLineIds.has(row.lineId)) continue;
    const existing = merged.get(row.lineId);
    if (!existing) {
      merged.set(row.lineId, { line: row, groupNames: [row.groupName] });
      continue;
    }
    if (!existing.groupNames.includes(row.groupName)) {
      existing.groupNames.push(row.groupName);
    }
  }
  return [...merged.values()];
}
