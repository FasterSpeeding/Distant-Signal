import type { SharedGroupTrain } from './types';

/** One row of `/track/mine`'s shared-train half: the shared train itself,
 * plus EVERY group the caller can see it through. */
export interface MergedSharedTrain {
  train: SharedGroupTrain;
  /** Never empty, and de-duplicated -- one entry per distinct group the
   * caller shares with this train, in the order the API returned them
   * (newest share first, see `list_shared_trains_for_user`). */
  groupNames: string[];
}

/** Collapses `GET /public/groups/shared-trains`'s one-row-per-(group,
 * train) wire shape into one row per train, carrying every group name that
 * train arrived through.
 *
 * The backend deliberately does NOT collapse this itself (see
 * `groups::list_shared_trains_for_user`'s own doc comment): a train shared
 * into two of the caller's groups is genuinely two attributions, and
 * picking one server-side would silently drop a group name from the tag.
 * The other fields are identical across those rows by construction -- they
 * all describe the same `train_subscriptions` row, and only its OWNER can
 * share it (`groups::add_train_to_group` checks ownership), so `addedBy`
 * can't differ between them either -- which is why keeping the first row's
 * copy of them is lossless rather than an arbitrary pick.
 *
 * `ownTrainIds` is belt-and-braces, not the primary defence: the backend's
 * `ts.user_id <> $1` already excludes the caller's own subscriptions, so
 * this only matters if that ever regresses. Rendering a train BOTH as the
 * caller's own row (with rename/ticket controls) and again as a shared one
 * would be a visible duplicate, so the page filters here rather than
 * trusting one end alone. */
export function mergeSharedTrains(
  rows: SharedGroupTrain[],
  ownTrainIds: ReadonlySet<number> = new Set(),
): MergedSharedTrain[] {
  const merged = new Map<number, MergedSharedTrain>();
  for (const row of rows) {
    if (ownTrainIds.has(row.trainSubscriptionId)) continue;
    const existing = merged.get(row.trainSubscriptionId);
    if (!existing) {
      merged.set(row.trainSubscriptionId, { train: row, groupNames: [row.groupName] });
      continue;
    }
    if (!existing.groupNames.includes(row.groupName)) {
      existing.groupNames.push(row.groupName);
    }
  }
  return [...merged.values()];
}
