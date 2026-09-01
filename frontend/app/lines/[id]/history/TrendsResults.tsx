// Placeholder for Task 9, which replaces this wholesale with the real
// Trends rollup chart (built on `getLineDailyStats`). Mirrors the
// `TicketEntryForm`/`TicketPanel` ordering precedent used elsewhere in this
// repo: the page-level `Tabs` wiring (Task 8) lands first referencing this
// file's exported shape, and the real component fills it in later without
// requiring any changes to the page that renders it.
export async function TrendsResults({
  id,
  from,
  to,
}: {
  id: string;
  from: string;
  to: string;
}) {
  return null;
}
