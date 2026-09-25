/** Labels for `Disruption.impactType`'s three known values -- see
 * `common::Disruption` (`crates/common/src/lib.rs`) and
 * docs/superpowers/specs/2026-09-01-disruption-impact-type-design.md
 * Decision 7. Rendered by both `DisruptionDetail.tsx` (expanded panel) and
 * `IssueList.tsx` (collapsed row) via `impactTypeLabel`, so both surfaces
 * stay in sync on wording by construction. */
export const IMPACT_TYPE_LABELS: Record<string, string> = {
  rail_replacement_bus: 'Rail Replacement Bus',
  no_scheduled_service: 'No Scheduled Service',
  diversion: 'Diversion',
};

/** `null` for a `null`/absent `impactType` (the common case -- no specific
 * fact stated) AND for any unrecognized value (schema drift, a future
 * taxonomy addition this frontend hasn't shipped yet) -- both fail safe to
 * "render nothing" rather than a raw snake_case string, unlike
 * `severityColor`'s fallback-to-'gray' (which must always render
 * *something*, since every status has a severity). `impact_type` is
 * already optional/supplementary everywhere in this design, so silently
 * omitting an unrecognized value is safe. */
export function impactTypeLabel(impactType: string | null | undefined): string | null {
  if (!impactType) return null;
  // Signal Box Audit, flib Low finding: "prototype-key lookups can render a
  // function as a label". `impactType` is deliberately open-ended (this
  // function's own doc comment anticipates "schema drift, a future taxonomy
  // addition"), so a bare `IMPACT_TYPE_LABELS[impactType]` would resolve
  // `impactType === 'constructor'` to `Object.prototype.constructor` (a
  // function) instead of `undefined` -- `?? null` never fires because a
  // function is neither `null` nor `undefined`, and `DisruptionDetail`/
  // `IssueList` would render that function where they expect a string or
  // `null`. Guarding with `hasOwnProperty` keeps the lookup to
  // IMPACT_TYPE_LABELS' own declared keys.
  return Object.prototype.hasOwnProperty.call(IMPACT_TYPE_LABELS, impactType)
    ? IMPACT_TYPE_LABELS[impactType]
    : null;
}
