# Superpowers specs and plans

This directory holds the design specs (`specs/`) and implementation plans
(`plans/`) written while building Distant Signal.

## How to read these documents

- **They are historical records, not current documentation.** Each dated
  file describes the code, the data and the decisions *as of its date*.
  Later work often changed or replaced what a spec proposed, and line
  numbers, file paths, counts and "not yet" statements inside a spec are
  usually out of date. The code, the chart's `values.yaml`/`README.md` and
  the top-level `docs/` pages are the source of truth for current behaviour.
- **They are not edited to track the code.** A spec keeps its original text.
  When a spec's own status line is actively misleading, a short dated
  status note is added at the top and the original text is left below it.
- **Status lives in this index.** The table below records whether each
  spec's proposal is in the code today. Update the row, not the spec, when
  that changes.
- **Plans** (`plans/`) are the step-by-step execution records for specs.
  Their task lists and "judgment calls" describe how the work was done at
  the time. Some plans have been pruned; code comments should explain
  themselves rather than point at a plan.
- **New documents** use the `YYYY-MM-DD-<topic>-design.md` (spec),
  `-research.md` (research), `-review.md` (review) or `-plan.md` (plan)
  naming, and get a row here.

## Spec status

Status values:

| Status | Meaning |
|---|---|
| implemented | The core proposal is in the code. Details may have changed since. |
| partly | Some of the proposal is in the code; the note says what is missing. |
| not implemented | Nothing meaningful from the proposal is in the code. |
| superseded | Replaced by a later spec, or the code went a different way. |
| research / review | Research, findings or a review with no single build target. The note says whether it was acted on where that is known. |
| unverified | The status could not be confirmed from this repository. |

Statuses were checked against the code on 2026-09-28 by looking for each
spec's crates, routes, tables, components and chart values. The partly and
not implemented rows, and the rows for work that has since reached
production, were rechecked against `main` and production on 2026-10-19.
A partly or not implemented row says what is left and names the
task-bringer card that tracks it. The evidence
column names what was found, not everything the spec proposed. Where a
spec's counterpart lives outside this repository (the distant-signal-mcp
server), only this repository's side was checked.

178 specs: 120 implemented, 6 partly, 4 not implemented, 7 superseded, 41 research / review.

| Spec | Status | Evidence / note |
|---|---|---|
| [2026-07-06-aggregator-read-api-design](specs/2026-07-06-aggregator-read-api-design.md) | implemented | `crates/aggregator`, `crates/api/src/routes/line_status.rs` serve the TfL-shaped endpoints |
| [2026-07-06-ldbws-sampler-poller-design](specs/2026-07-06-ldbws-sampler-poller-design.md) | implemented | `crates/poller-ldbws` (schema.rs, rotation.rs, budget.rs) |
| [2026-07-07-frontend-design](specs/2026-07-07-frontend-design.md) | implemented | Next.js + `@mantine/core` in `frontend/package.json`; `frontend/app/*` pages |
| [2026-07-09-custom-lines-and-blended-stats-design](specs/2026-07-09-custom-lines-and-blended-stats-design.md) | implemented | `migrations/20260709100000_custom_lines.sql`; `sample_stats` in `crates/api/src/render.rs` |
| [2026-07-09-frontend-personalization-design](specs/2026-07-09-frontend-personalization-design.md) | implemented | `pinned_lines` in `data/preferences.rs`, `routes/preferences.rs`; `PinToggle.tsx` |
| [2026-07-09-outage-page-redesign-design](specs/2026-07-09-outage-page-redesign-design.md) | implemented | `IssueList.tsx`, `RepresentativeInfo.tsx`, `lib/severity.ts` worstStatus, `lib/sanitizeHtml` |
| [2026-07-11-operator-station-autocomplete-design](specs/2026-07-11-operator-station-autocomplete-design.md) | implemented | `search_stations` in `crates/api/src/data/reference.rs`, `routes/reference.rs` |
| [2026-07-12-dark-theme-design](specs/2026-07-12-dark-theme-design.md) | implemented | `frontend/components/ThemeToggle.tsx` used in `AppNavBar.tsx` |
| [2026-07-12-edit-custom-lines-design](specs/2026-07-12-edit-custom-lines-design.md) | implemented | `frontend/app/lines/[id]/edit/page.tsx`, `DeleteLineButton.tsx` |
| [2026-07-13-skipped-station-detection-design](specs/2026-07-13-skipped-station-detection-design.md) | implemented | `extract_skipped_stations` in poller-ldbws schema.rs; `skip_rate` in aggregation.rs; `20260921070000_skipped_stations.sql` |
| [2026-07-15-last-updated-indicators-design](specs/2026-07-15-last-updated-indicators-design.md) | implemented | `LastUpdated.tsx` in LineStatusCard; `DataFreshnessInfo.tsx` |
| [2026-07-16-stale-incident-handling-design](specs/2026-07-16-stale-incident-handling-design.md) | implemented | `WHERE NOT is_cleared` in aggregator queries.rs; `next_rail_day_boundary(first_seen_at)` |
| [2026-08-18-grape-theme-design](specs/2026-08-18-grape-theme-design.md) | implemented | `frontend/lib/theme.ts` sets grape primaryColor (primaryShade kept default, deliberately) |
| [2026-08-18-helm-chart-design](specs/2026-08-18-helm-chart-design.md) | implemented | Chart exists as `charts/distant-signal`, renamed from the spec's `charts/nr-status` |
| [2026-08-20-incident-nlp-extraction-design](specs/2026-08-20-incident-nlp-extraction-design.md) | implemented | `crates/enricher/src/llm.rs`; `20260820120000_incident_extraction.sql` |
| [2026-08-21-multi-period-extraction-design](specs/2026-08-21-multi-period-extraction-design.md) | implemented | `20260822090000_incident_extraction_periods.sql`; periods `resolution_status` in aggregator queries.rs |
| [2026-08-22-tfl-service-metrics-v2-design](specs/2026-08-22-tfl-service-metrics-v2-design.md) | implemented | `tfl-elizabeth` merge (`routes/lines.rs` is_merged_into_nr_line); `crates/poller-tfl/src/dlr/inference.rs` (Area 2, Overground groundwork, unverified) |
| [2026-08-28-train-tracking-design](specs/2026-08-28-train-tracking-design.md) | implemented | `crates/trust-consumer`, `20260828120000_train_tracking.sql` (now fed via movement-relay) |
| [2026-08-28-user-accounts-sso-design](specs/2026-08-28-user-accounts-sso-design.md) | implemented | `crates/api/src/auth/oidc.rs`, `SSO_ISSUER_URL`; `20260828100000_add_ownership.sql` |
| [2026-08-29-dev-oidc-server-design](specs/2026-08-29-dev-oidc-server-design.md) | implemented | Authentik wired in `docker-compose.yml`; referenced in `data/config.rs` |
| [2026-08-29-journey-ticket-tracking-design](specs/2026-08-29-journey-ticket-tracking-design.md) | implemented | `data/ticket_extraction.rs` (pkpass/PDF), `delay_repay_rules.rs`, `20260829090000_journey_ticket_tracking.sql` |
| [2026-08-29-journey-ticket-tracking-frontend-design](specs/2026-08-29-journey-ticket-tracking-frontend-design.md) | implemented | `frontend/app/track/mine/add-ticket`, `AttachTicketAction.tsx`, `DelayRepayEstimate.tsx` |
| [2026-08-29-line-coverage-gap-analysis](specs/2026-08-29-line-coverage-gap-analysis.md) | research / review | Acted on: `lines/*.toml` grew from 20 to 243 files |
| [2026-08-29-metrics-design](specs/2026-08-29-metrics-design.md) | implemented | `crates/common/src/metrics.rs` (Prometheus exporter); prometheus.io scrape annotations in chart templates |
| [2026-08-29-project-naming-research](specs/2026-08-29-project-naming-research.md) | research / review | Acted on: project renamed Distant Signal (`charts/distant-signal`) |
| [2026-08-29-train-tracking-frontend-design](specs/2026-08-29-train-tracking-frontend-design.md) | implemented | `frontend/app/train/[uid]/[date]`, `frontend/app/track`, `track/mine` pages |
| [2026-08-29-trust-schedule-delay-inference-design](specs/2026-08-29-trust-schedule-delay-inference-design.md) | research / review | Acted on: `DataQuality::TrustInferred` in common lib.rs; `schedule-ingest` crate |
| [2026-08-29-trust-schedule-delay-inference-timetable-verification](specs/2026-08-29-trust-schedule-delay-inference-timetable-verification.md) | research / review | Addendum checking CIF timetable claims; led to `schedule-ingest`/`schedule-reference` crates |
| [2026-08-29-trust-schedule-delay-validation-findings](specs/2026-08-29-trust-schedule-delay-validation-findings.md) | research / review | Findings from a validation run; no single build target |
| [2026-08-30-inferred-time-ranges-design](specs/2026-08-30-inferred-time-ranges-design.md) | implemented | `carry_forward_ldbws_from_date` in `crates/aggregator/src/queries.rs` |
| [2026-08-30-schedule-feed-ingress-design](specs/2026-08-30-schedule-feed-ingress-design.md) | research / review | Options survey; push chosen (2026-09-01-schedule-feed-push-design); `docker/schedule-sftp-entrypoint.sh` |
| [2026-08-30-schedule-feed-sftp-pull-design](specs/2026-08-30-schedule-feed-sftp-pull-design.md) | superseded | Self-marked superseded by 2026-09-01-schedule-feed-push-design (pull access is staff-only) |
| [2026-08-31-anonymous-user-ux-design](specs/2026-08-31-anonymous-user-ux-design.md) | implemented | Anonymous branch in `frontend/app/page.tsx`; login prompts in AllLinesTable/CustomLineForm |
| [2026-08-31-dynamic-post-login-redirect-design](specs/2026-08-31-dynamic-post-login-redirect-design.md) | implemented | `captured_return_to` in `routes/auth.rs`; `20260831090000_login_state_return_to.sql` |
| [2026-08-31-incident-detail-page-design](specs/2026-08-31-incident-detail-page-design.md) | implemented | `/incidents/{incidentId}` in `routes/incidents.rs`; `frontend/app/incidents/[id]` |
| [2026-08-31-line-history-graphics-design](specs/2026-08-31-line-history-graphics-design.md) | implemented | `lines/[id]/history/TrendsCharts.tsx`; `20260831090001_line_status_daily_stats.sql` |
| [2026-08-31-other-uk-transit-networks-research](specs/2026-08-31-other-uk-transit-networks-research.md) | research / review | No Metrolink/Nexus/tram pollers; Irish Rail/NIR pollers cover a separate area |
| [2026-08-31-private-custom-lines-and-tracked-trains-design](specs/2026-08-31-private-custom-lines-and-tracked-trains-design.md) | implemented | `20260901120000_custom_lines_owner_not_null.sql` (user_id NOT NULL); user-scoped train/ticket queries |
| [2026-08-31-sample-data-availability-design](specs/2026-08-31-sample-data-availability-design.md) | implemented | `sampleUnavailableReason` in `frontend/lib/sampleStats.ts`, refined by the 2026-09-01 sample-coverage spec |
| [2026-08-31-station-catalogue-completeness-research](specs/2026-08-31-station-catalogue-completeness-research.md) | research / review | Adoption of its station additions unverified |
| [2026-08-31-tickets-list-design](specs/2026-08-31-tickets-list-design.md) | implemented | `GET /Train/tickets/mine` in `crates/api/src/routes/train.rs`; list folded into `/track/mine` |
| [2026-08-31-tracked-trains-list-design](specs/2026-08-31-tracked-trains-list-design.md) | implemented | `GET /Train/mine` and `frontend/app/track/mine/page.tsx` |
| [2026-09-01-dark-reader-color-scheme-signal-research](specs/2026-09-01-dark-reader-color-scheme-signal-research.md) | research / review | Acted on: `frontend/components/ColorSchemeMeta.tsx` |
| [2026-09-01-disruption-impact-type-design](specs/2026-09-01-disruption-impact-type-design.md) | implemented | `impact_type` in enricher `llm.rs`, aggregator, `common/src/lib.rs`; `frontend/lib/impactType.ts` |
| [2026-09-01-disruption-type-extraction-research](specs/2026-09-01-disruption-type-extraction-research.md) | research / review | Acted on via the impact-type design; cited in `crates/enricher/src/llm.rs` |
| [2026-09-01-dynamic-color-scheme-meta-design](specs/2026-09-01-dynamic-color-scheme-meta-design.md) | implemented | `frontend/components/ColorSchemeMeta.tsx`, rendered in `app/layout.tsx` |
| [2026-09-01-embedded-chatbot-mcp-integration-research](specs/2026-09-01-embedded-chatbot-mcp-integration-research.md) | research / review | Acted on: `/chat`, `/connect-claude`, `frontend/lib/mcpOAuthProvider.ts` |
| [2026-09-01-enricher-period-cap-failures-research](specs/2026-09-01-enricher-period-cap-failures-research.md) | research / review | Acted on: cap-breach truncation in `crates/enricher/src/main.rs` |
| [2026-09-01-enricher-period-cap-remediation-design](specs/2026-09-01-enricher-period-cap-remediation-design.md) | implemented | Truncation replaces discard (`crates/enricher/src/main.rs`); prompt guidance in `llm.rs` |
| [2026-09-01-internal-service-accounts-design](specs/2026-09-01-internal-service-accounts-design.md) | superseded | By 2026-09-02-internal-service-oauth2-design (`build_internal_oauth_routes`, per-service `svc-*` groups) |
| [2026-09-01-line-status-sample-coverage-design](specs/2026-09-01-line-status-sample-coverage-design.md) | implemented | `SampleAvailability` in `crates/common/src/lib.rs`; `frontend/lib/sampleStats.ts` |
| [2026-09-01-pwa-manifest-design](specs/2026-09-01-pwa-manifest-design.md) | implemented | `frontend/app/manifest.ts`, `public/icon-192.png`, `icon-512.png` |
| [2026-09-01-pwa-support-research](specs/2026-09-01-pwa-support-research.md) | research / review | Acted on: manifest, then a service worker (`frontend/public/sw.js`) |
| [2026-09-01-schedule-feed-push-design](specs/2026-09-01-schedule-feed-push-design.md) | implemented | sftpgo `scheduleFeed.sftp` in chart values; `crates/schedule-ingest` |
| [2026-09-01-schedule-ingest-stanox-crs-table-design](specs/2026-09-01-schedule-ingest-stanox-crs-table-design.md) | implemented | `crates/schedule-reference`, `20260901150000_stanox_crs.sql`, trust-consumer reload with CSV fallback |
| [2026-09-01-stanox-crs-live-reference-data-research](specs/2026-09-01-stanox-crs-live-reference-data-research.md) | research / review | Acted on via the `stanox_crs` table and schedule-reference |
| [2026-09-01-tracked-trains-home-page-design](specs/2026-09-01-tracked-trains-home-page-design.md) | implemented | `frontend/app/page.tsx` renders tracked trains (`TrackedTrainStatusBadge`) |
| [2026-09-01-train-mcp-integration-design](specs/2026-09-01-train-mcp-integration-design.md) | implemented | The MCP server lives in a separate repo (distant-signal-mcp); its internals are not verified here |
| [2026-09-01-train-mcp-integration-research](specs/2026-09-01-train-mcp-integration-research.md) | research / review | Acted on: led to the separate distant-signal-mcp server |
| [2026-09-02-client-local-timezone-display-research](specs/2026-09-02-client-local-timezone-display-research.md) | research / review | Partly acted on: `formatLocalDateTime` in `frontend/lib/dateFormat.ts` |
| [2026-09-02-custom-line-creation-page-design](specs/2026-09-02-custom-line-creation-page-design.md) | implemented | `frontend/app/lines/new/page.tsx` |
| [2026-09-02-embedded-chatbot-dual-mode-design](specs/2026-09-02-embedded-chatbot-dual-mode-design.md) | superseded | Option B server orchestrator replaced by the client-side-tokens design; Option C `/connect-claude` exists as an instructions page. Its `/connect-claude/authorize` consent bridge was removed on 2026-09-29: the MCP server now logs users in through its own Authentik OIDC client |
| [2026-09-02-embedded-chatbot-option-b-client-side-tokens-design](specs/2026-09-02-embedded-chatbot-option-b-client-side-tokens-design.md) | implemented | `AnthropicKeySettings.tsx`, `ChatPanel.tsx`; `20260906090000_drop_chatbot_allowed_users.sql`. Since 2026-09-29 the MCP login it describes goes through the MCP server's own Authentik client, not the removed `/connect-claude/authorize` bridge |
| [2026-09-02-frontend-accessibility-audit-research](specs/2026-09-02-frontend-accessibility-audit-research.md) | research / review | Findings acted on; cited in `frontend/app/error.tsx`, `lib/theme.ts` |
| [2026-09-02-frontend-disconnect-reconnect-ux-design](specs/2026-09-02-frontend-disconnect-reconnect-ux-design.md) | implemented | `frontend/components/ConnectivityMonitor.tsx` |
| [2026-09-02-frontend-ui-ux-review](specs/2026-09-02-frontend-ui-ux-review.md) | research / review | Partly acted on; cited in `AllLinesTable.tsx`, `DisruptionDetail.tsx` |
| [2026-09-02-internal-service-oauth2-design](specs/2026-09-02-internal-service-oauth2-design.md) | implemented | `client_credentials` in `crates/common/src/oauth_client.rs`; `build_internal_oauth_routes` in `crates/api/src/app.rs` |
| [2026-09-02-line-history-chart-fixes-design](specs/2026-09-02-line-history-chart-fixes-design.md) | implemented | `TrendsCharts.tsx` legend, dash patterns, `ReferenceArea` |
| [2026-09-02-line-history-list-spamminess-research](specs/2026-09-02-line-history-list-spamminess-research.md) | research / review | Cited in `frontend/components/IncidentSearchForm.tsx`; extent of adoption unverified |
| [2026-09-02-line-status-notifications-design](specs/2026-09-02-line-status-notifications-design.md) | implemented | `20260902100000_notifications.sql`, `crates/notifier`, `push` handler in `sw.js` |
| [2026-09-02-mcp-board-tools-rest-api-sourcing-research](specs/2026-09-02-mcp-board-tools-rest-api-sourcing-research.md) | research / review | Recommends no refactor; nothing to build in this repo |
| [2026-09-02-mcp-server-first-party-hosting-design](specs/2026-09-02-mcp-server-first-party-hosting-design.md) | implemented | Two-repo split kept; the MCP server has its own repo and chart |
| [2026-09-02-mcp-server-oauth-access-groups-design](specs/2026-09-02-mcp-server-oauth-access-groups-design.md) | implemented | This repo's side: `oidcStoredGroupsExtra`, `20260902160000_user_access_groups.sql`; adapter side not verified |
| [2026-09-02-modal-login-prompt-design](specs/2026-09-02-modal-login-prompt-design.md) | implemented | `frontend/components/LoginPromptModal.tsx` |
| [2026-09-02-pwa-service-worker-design](specs/2026-09-02-pwa-service-worker-design.md) | implemented | `frontend/public/sw.js`, `offline.html`, `ServiceWorkerRegister.tsx` |
| [2026-09-02-rail-ticket-barcode-format-research](specs/2026-09-02-rail-ticket-barcode-format-research.md) | research / review | Input for the never-decode constraint; no decoding code |
| [2026-09-02-shared-internal-services-group-design](specs/2026-09-02-shared-internal-services-group-design.md) | not implemented | Recommended against a shared group; per-service `svc-*` groups kept, as the doc decided. Nothing left to build |
| [2026-09-02-slow-query-warnings-research](specs/2026-09-02-slow-query-warnings-research.md) | research / review | Acted on: cited in `crates/aggregator/src/queries.rs`; Postgres `max_wal_size` |
| [2026-09-02-standalone-ticket-entry-page-design](specs/2026-09-02-standalone-ticket-entry-page-design.md) | implemented | `frontend/app/track/mine/add-ticket/page.tsx` |
| [2026-09-02-ticket-display-delete-original-design](specs/2026-09-02-ticket-display-delete-original-design.md) | implemented | `DELETE /Train/tickets/{ticket_id}`, `DeleteTicketButton.tsx`; part 3 deliberately not designed |
| [2026-09-02-ticket-file-support-improvements-research](specs/2026-09-02-ticket-file-support-improvements-research.md) | research / review | Acted on: `auxiliaryFields`, `Out:`/`Ret:`, `barcode_format` in `crates/api/src/data/ticket_extraction.rs` |
| [2026-09-02-ticket-processing-improvements-design](specs/2026-09-02-ticket-processing-improvements-design.md) | implemented | `crates/api/src/data/ticket_extraction.rs` (ticket type, OTRL chain, barcode format detection) |
| [2026-09-02-ticket-upload-drag-and-drop-design](specs/2026-09-02-ticket-upload-drag-and-drop-design.md) | implemented | `@mantine/dropzone` in `frontend/components/TicketEntryForm.tsx` |
| [2026-09-02-trend-chart-granularity-design](specs/2026-09-02-trend-chart-granularity-design.md) | implemented | Line page embed shipped, later refined to half-hourly (`/Line/{id}/Stats/HalfHourly`) |
| [2026-09-02-vault-secret-store-design](specs/2026-09-02-vault-secret-store-design.md) | research / review | Recommends not adopting Vault; followed (chart has `existingSecret` hooks only) |
| [2026-09-03-full-coverage-metrics-transition-design](specs/2026-09-03-full-coverage-metrics-transition-design.md) | implemented | aggregator `merge_full_coverage_stats`, `DataQuality::TrustInferred`; migrations 20260903200000/200001 |
| [2026-09-03-full-coverage-per-station-stats-design](specs/2026-09-03-full-coverage-per-station-stats-design.md) | superseded | Deferral overridden by 2026-09-04-per-station-full-coverage-stats-design |
| [2026-09-03-half-hourly-coverage-trends-design](specs/2026-09-03-half-hourly-coverage-trends-design.md) | implemented | `HalfHourlyCoverageTrendsResults.tsx`, `/Line/{id}/Stats/Coverage/HalfHourly` |
| [2026-09-03-option-b-consumer-scoping](specs/2026-09-03-option-b-consumer-scoping.md) | research / review | Scoping verdict later overridden; the consumer shipped as `crates/full-coverage-consumer` |
| [2026-09-03-per-station-stats-design](specs/2026-09-03-per-station-stats-design.md) | implemented | `/stations/{crs}/sample-stats` in `crates/api/src/routes/station_stats.rs` |
| [2026-09-03-per-station-stats-research](specs/2026-09-03-per-station-stats-research.md) | research / review | Acted on: Option C shipped as `station_stats.rs` |
| [2026-09-03-schedule-feed-cadence-research](specs/2026-09-03-schedule-feed-cadence-research.md) | research / review | Subscription-choice research; no code artifact |
| [2026-09-03-schedule-feed-zip-delivery-correction](specs/2026-09-03-schedule-feed-zip-delivery-correction.md) | implemented | `find_zip_candidates` in `crates/schedule-ingest/src/delivery.rs` |
| [2026-09-03-track-a-train-autocomplete-design](specs/2026-09-03-track-a-train-autocomplete-design.md) | implemented | Mantine `Autocomplete` in `TrackTrainForm.tsx` |
| [2026-09-03-track-a-train-input-ux-research](specs/2026-09-03-track-a-train-input-ux-research.md) | research / review | Part 1 acted on; part 2 replaced by the trip-search designs |
| [2026-09-03-trip-search-design](specs/2026-09-03-trip-search-design.md) | implemented | `/stations/{crs}/departures` in `crates/api/src/routes/departures.rs` |
| [2026-09-04-movement-relay-design](specs/2026-09-04-movement-relay-design.md) | implemented | `crates/movement-relay`; chart `movementRelay.enabled` defaults to true |
| [2026-09-04-option-b-live-consumer-design](specs/2026-09-04-option-b-live-consumer-design.md) | implemented | `crates/full-coverage-consumer`; no longer shadow-only in production |
| [2026-09-04-per-station-full-coverage-stats-design](specs/2026-09-04-per-station-full-coverage-stats-design.md) | implemented | `20260904070000_station_full_coverage_samples.sql`; merged in `station_stats.rs` |
| [2026-09-04-track-a-train-picker-refactor-design](specs/2026-09-04-track-a-train-picker-refactor-design.md) | implemented | Cited throughout `TrackTrainForm.tsx` |
| [2026-09-04-whole-network-trip-search-design](specs/2026-09-04-whole-network-trip-search-design.md) | implemented | `/stations/{crs}/schedule-departures`; `pickCifDeparture` in `TrackTrainForm.tsx` |
| [2026-09-04-whole-network-trip-search-research](specs/2026-09-04-whole-network-trip-search-research.md) | research / review | Acted on: CIF fallback picker shipped |
| [2026-09-05-configurable-trend-granularity-design](specs/2026-09-05-configurable-trend-granularity-design.md) | implemented | `/Line/{id}/Stats/Hourly`, `/SixHourly` routes |
| [2026-09-05-country-filtering-design](specs/2026-09-05-country-filtering-design.md) | implemented | `selectedCountries` in `frontend/app/lines/AllLinesTable.tsx` |
| [2026-09-05-custom-tracking-names-design](specs/2026-09-05-custom-tracking-names-design.md) | implemented | `20260905130000_custom_tracking_names.sql` (`custom_name`) |
| [2026-09-05-incident-line-matching-false-positive-design](specs/2026-09-05-incident-line-matching-false-positive-design.md) | implemented | Operator-contradiction handling in `crates/common/src/matcher.rs` |
| [2026-09-05-ingestion-frequency-metrics-design](specs/2026-09-05-ingestion-frequency-metrics-design.md) | partly | Per-source arrival is covered another way: the ingest-writer's `distant_signal_ingest_freshness_timestamp_seconds{source}` gauge and `DistantSignalIngestSourceStale` (ingest architecture). Not yet: Decision 2's `last_incident_change` gauge and the Grafana panels; the chart ships no dashboards (DS-45) |
| [2026-09-05-ireland-rail-support-design](specs/2026-09-05-ireland-rail-support-design.md) | implemented | `crates/poller-irish-rail-gtfs`, `crates/poller-irish-rail-live`, island-of-ireland routes |
| [2026-09-05-ireland-vs-northern-ireland-friction-research](specs/2026-09-05-ireland-vs-northern-ireland-friction-research.md) | research / review | Acted on: combined Ireland spec and pollers shipped |
| [2026-09-05-mcp-deeper-api-integration-design](specs/2026-09-05-mcp-deeper-api-integration-design.md) | partly | Phases 2a/2b are in the API (`/public/lines/{id}/schedule`, `/trains`) and ds-mcp calls `/trains`. Not yet: phase 3a (`train_movement_events.raw_body` is still `{}` in trust-consumer `process.rs`) and 3b (no `full_coverage_train_state`) (DS-49) |
| [2026-09-05-nir-tier-a-implementation-design](specs/2026-09-05-nir-tier-a-implementation-design.md) | implemented | `crates/poller-nir-stations` |
| [2026-09-05-northern-ireland-rail-support-design](specs/2026-09-05-northern-ireland-rail-support-design.md) | superseded | By 2026-09-05-ireland-rail-support-design |
| [2026-09-05-rust-service-deduplication-design](specs/2026-09-05-rust-service-deduplication-design.md) | implemented | `crates/health-http`, `crates/common/src/{service_args,poller_loop}.rs` |
| [2026-09-05-schedule-first-train-tracking-design](specs/2026-09-05-schedule-first-train-tracking-design.md) | implemented | `crates/api/src/data/schedule_matching.rs`; schedule-first sweep config in `crates/api/src/config.rs` |
| [2026-09-05-status-observability-grafana-design](specs/2026-09-05-status-observability-grafana-design.md) | implemented | `templates/podmonitor.yaml`, `templates/prometheusrule.yaml` |
| [2026-09-05-status-observability-page-design](specs/2026-09-05-status-observability-page-design.md) | superseded | By the Grafana design; no `service_heartbeats` table |
| [2026-09-05-trend-sample-volume-chart-design](specs/2026-09-05-trend-sample-volume-chart-design.md) | implemented | `showVolume` in `TrendsCharts.tsx` |
| [2026-09-05-trust-event-backlog-design](specs/2026-09-05-trust-event-backlog-design.md) | implemented | `crates/trust-backlog-consumer`, `20260905160000_trust_event_backlog.sql` |
| [2026-09-06-schedule-line-population-future-dates-design](specs/2026-09-06-schedule-line-population-future-dates-design.md) | partly | `line_population_dates` in schedule-reference publishes today and tomorrow (2026-09-27). Not yet: anything further out, so a pin two or more days ahead has no population row until the day before (DS-46) |
| [2026-09-06-schedule-line-population-past-dates-design](specs/2026-09-06-schedule-line-population-past-dates-design.md) | not implemented | `MAX_PIN_AGE` still 6h in `crates/api/src/data/train_tracking.rs`. Left: the whole proposal, a scoping doc that was never approved (DS-46) |
| [2026-09-06-shared-train-identity-design](specs/2026-09-06-shared-train-identity-design.md) | implemented | `20260906100000_trains.sql`, `20260907100000_rename_tracked_trains.sql` (now `train_subscriptions`) |
| [2026-09-07-shared-train-status-write-race-design](specs/2026-09-07-shared-train-status-write-race-design.md) | implemented | Option C event-time monotonicity guard in `crates/api/src/data/train_tracking.rs` |
| [2026-09-07-tfl-incident-page-design](specs/2026-09-07-tfl-incident-page-design.md) | not implemented | No `/public/tfl-lines/{id}`, `tfl-history` or `tflLineIdFromSource` in crates/ or frontend/; TfL lines use the generic `/lines/[id]` page and its history. Left: the Option B live-snapshot page, a scoping doc that was never approved (DS-47) |
| [2026-09-07-train-listing-destination-search-sizing-design](specs/2026-09-07-train-listing-destination-search-sizing-design.md) | implemented | Approach C flat table, `20260907130000_schedule_destination_departures.sql` |
| [2026-09-07-train-listing-page-design](specs/2026-09-07-train-listing-page-design.md) | implemented | `frontend/app/trains/page.tsx`, `crates/api/src/routes/trains.rs` |
| [2026-09-08-calling-point-train-search-design](specs/2026-09-08-calling-point-train-search-design.md) | implemented | `20260908120000_schedule_destination_departures_calling_point_search.sql` |
| [2026-09-08-destination-arrival-time-filter-design](specs/2026-09-08-destination-arrival-time-filter-design.md) | superseded | `destination_arrival` shipped; `destination_from/to` params replaced by `arrival_from/to` (stops-at spec) |
| [2026-09-08-journey-timetable-overlay-design](specs/2026-09-08-journey-timetable-overlay-design.md) | implemented | `crates/api/src/data/journey.rs`, `JourneyStop` in `frontend/lib/types.ts` |
| [2026-09-08-tracked-train-reconciliation-design](specs/2026-09-08-tracked-train-reconciliation-design.md) | implemented | `crates/api/src/data/reconciliation.rs` sweep loop |
| [2026-09-09-mcp-schedule-data-follow-up-design](specs/2026-09-09-mcp-schedule-data-follow-up-design.md) | implemented | `GET /public/lines/{id}/trains` (`get_line_trains` in `crates/api/src/routes/lines.rs`) |
| [2026-09-09-stops-at-search-filter-design](specs/2026-09-09-stops-at-search-filter-design.md) | implemented | `stops_at`, `arrival_from`/`arrival_to` in `crates/api/src/routes/trains.rs` |
| [2026-09-09-trains-search-multi-day-design](specs/2026-09-09-trains-search-multi-day-design.md) | implemented | 7-day search window in `routes/trains.rs` |
| [2026-09-11-shared-groups-design](specs/2026-09-11-shared-groups-design.md) | implemented | `20260911090000_shared_groups.sql`, `routes/groups.rs`, `frontend/app/groups/` |
| [2026-09-12-custom-line-group-sharing-design](specs/2026-09-12-custom-line-group-sharing-design.md) | implemented | `20260915100000_custom_line_group_grants.sql`, `/groups/{id}/lines/custom` routes |
| [2026-09-12-eurostar-feasibility-research](specs/2026-09-12-eurostar-feasibility-research.md) | research / review | Concluded not feasible; no Eurostar code |
| [2026-09-12-group-lines-design](specs/2026-09-12-group-lines-design.md) | not implemented | No `group_lines` table; the spec itself recommended "not yet". Left: the whole design, pending that timing call (DS-48) |
| [2026-09-12-incident-archive-design](specs/2026-09-12-incident-archive-design.md) | implemented | `frontend/app/incidents/page.tsx`, `search_incidents` in `routes/incidents.rs` |
| [2026-09-12-journey-progress-visualization-design](specs/2026-09-12-journey-progress-visualization-design.md) | implemented | `frontend/components/JourneyProgress.tsx` |
| [2026-09-12-reliability-digest-design](specs/2026-09-12-reliability-digest-design.md) | implemented | `frontend/components/ReliabilityDigest.tsx` |
| [2026-09-12-station-accessibility-design](specs/2026-09-12-station-accessibility-design.md) | implemented | `/public/stations/{crs}/accessibility` in `routes/reference.rs`; rendering superseded by the 09-16 structured spec |
| [2026-09-12-station-timetable-design](specs/2026-09-12-station-timetable-design.md) | implemented | `frontend/components/StationTimetable.tsx` |
| [2026-09-16-custom-lines-in-incident-archive-filter-research](specs/2026-09-16-custom-lines-in-incident-archive-filter-research.md) | research / review | Not acted on: the incidents `line` filter is catalogue-only |
| [2026-09-16-structured-accessibility-rendering-design](specs/2026-09-16-structured-accessibility-rendering-design.md) | implemented | `frontend/lib/stationAccessibility.ts` (`sanitizeRichText`, `openingStatus`) |
| [2026-09-16-tfl-incident-archive-design](specs/2026-09-16-tfl-incident-archive-design.md) | research / review | Recommended "not yet"; line-filter fix acted on (`20260917090000_incidents_affected_lines.sql`) |
| [2026-09-17-accessibility-section-ux-review](specs/2026-09-17-accessibility-section-ux-review.md) | research / review | Acted on: `stationAccessibility.ts` cites review §3.5.4 |
| [2026-09-17-full-service-ux-accessibility-usability-review](specs/2026-09-17-full-service-ux-accessibility-usability-review.md) | research / review | Consolidated review; partly acted on (adoption per item unverified) |
| [2026-09-22-dynamic-trip-planning-design](specs/2026-09-22-dynamic-trip-planning-design.md) | implemented | `crates/trip-planner`, `/Trips/plan` in `routes/trips.rs`, `frontend/lib/tripPlan.ts` |
| [2026-09-22-journey-tracking-design](specs/2026-09-22-journey-tracking-design.md) | implemented | `20260922090000_journeys.sql`, `routes/journeys.rs`, `frontend/app/journeys/` |
| [2026-09-22-operator-overview-design](specs/2026-09-22-operator-overview-design.md) | implemented | `frontend/app/operators/`, `operators/[code]/history`, `20260922080000_pinned_operators.sql` |
| [2026-09-22-reusable-repeating-journeys-design](specs/2026-09-22-reusable-repeating-journeys-design.md) | implemented | `20260922140000_journey_templates.sql`, `routes/journey_templates.rs`, `frontend/app/journeys/templates/` |
| [2026-09-22-train-search-state-persistence-design](specs/2026-09-22-train-search-state-persistence-design.md) | implemented | URL write-back (`router.replace`) in `frontend/components/TrainSearchForm.tsx` |
| [2026-09-22-ux-review-collated-final](specs/2026-09-22-ux-review-collated-final.md) | research / review | Adoption unverified |
| [2026-09-22-ux-review-journey-creation-flow](specs/2026-09-22-ux-review-journey-creation-flow.md) | research / review | Cited in `crates/api/src/data/journeys.rs`; at least partly acted on |
| [2026-09-22-ux-review-journey-detail](specs/2026-09-22-ux-review-journey-detail.md) | research / review | Adoption unverified |
| [2026-09-22-ux-review-operators-homepage](specs/2026-09-22-ux-review-operators-homepage.md) | research / review | Adoption unverified |
| [2026-09-22-ux-review-status-dashboard-lines](specs/2026-09-22-ux-review-status-dashboard-lines.md) | research / review | Adoption unverified |
| [2026-09-23-unlisted-links-design](specs/2026-09-23-unlisted-links-design.md) | implemented | `20260923110000_unlisted_links.sql`, `frontend/app/journeys/shared/[token]/page.tsx` |
| [2026-09-27-full-coverage-windowed-stats-design](specs/2026-09-27-full-coverage-windowed-stats-design.md) | implemented | `20260927120000_full_coverage_line_window_stats.sql`; all switches off by default |
| [2026-09-28-corpus-sftp-ingest-design](specs/2026-09-28-corpus-sftp-ingest-design.md) | implemented | `crates/api/src/data/corpus.rs`, `corpus_crosswalk.rs`, the `corpus_compare` bin; `scheduleFeed.corpus` and `corpusFallback` off by default |
| [2026-09-28-trips-plan-live-overlay-design](specs/2026-09-28-trips-plan-live-overlay-design.md) | implemented | Live overlay and the waypoint chaining fix in `trip_planning_itinerary.rs` and `routes/trips.rs` |
| [2026-09-29-trips-plan-arrive-by-avoid-design](specs/2026-09-29-trips-plan-arrive-by-avoid-design.md) | implemented | `crates/trip-planner/src/{reverse,restrictions}.rs`, `plan_trip` in `trip_planning_itinerary.rs` |
| [2026-09-29-chat-add-mcp-server-links](specs/2026-09-29-chat-add-mcp-server-links.md) | implemented | `frontend/components/AddMcpServerLinks.tsx`, `frontend/lib/mcpInstallLinks.ts`; shown only when `railMcp.publicUrl` is set |
| [2026-09-30-backup-and-observability-gaps-design](specs/2026-09-30-backup-and-observability-gaps-design.md) | implemented | Live on mine-bringer 2026-10-01: pgBackRest PITR (`postgresql.pgbackrest`, tini image, exec grant as designed), explicit CronJob `timeZone` (CI check), Prometheus/Grafana PVCs, cold-archive expiry (`archive.expiry`, 730 d, dry-run), Loki + Alloy (7 d, all namespaces, shared `logging` service); Postgres password, SFTP host key and push password sealed as current values, not rotated; Redis, MCP SQLite and schedulefeed need no backups |
| [2026-09-30-redis-message-queue-evaluation-design](specs/2026-09-30-redis-message-queue-evaluation-design.md) | research / review | Evaluation plus five small changes. D2 and D5 (dead-letter age trim) built, D1 went to Ranma-Config, D4 kept the AOF alerts in the chart, D3 (disk encryption) open in Ranma-Config (RANMA-13), D6 declined |
| [2026-10-01-working-vs-public-times-design](specs/2026-10-01-working-vs-public-times-design.md) | implemented | Phase 1 (§10) and phase 2 (§11) landed 2026-10-01, the Delay Repay follow-ups 2026-10-07: public and exact working times stored, TRUST `gbtt_timestamp` stored (the backlog too, since 2026-10-07), set-down-only stops kept, additive `public*`/`working*`/`can*` API fields, delay on public at the passenger's stop (P5), Delay Repay at the ticket's destination (P1), planning on public minutes (P6), the live overlay on like-for-like times. ds-mcp maps `public*` (P7, ds-mcp `cc1d80d`). `scheduled*` stay WTT by decision (§9); P8's LDBWS matching on public minutes is not built |
| [2026-10-02-schedule-feed-gcs-landing-design](specs/2026-10-02-schedule-feed-gcs-landing-design.md) | partly | Now Google Cloud Storage. Decisions D1–D13 recorded 2026-10-02 (dedicated GCP project, Helm via Crossplane, Data Access audit logs, bucket wins, generic repo scope, dedicated in-transit bucket, DTD's service accounts and roles, reader deletes after a verified download, kill switch pauses the bindings). Chart side done: `charts/ds-ingest-bucket` (GCS delivery and audit-log buckets, bucket-level IAM, optional usage alerts and Pub/Sub; off by default, not deployed; checked against provider-upjet-gcp's CRDs by `scripts/check-ingest-bucket-chart.py`; published to the OCI registry), and in `charts/distant-signal` `scheduleFeed.sftp.enabled` (default on), the opt-in `scheduleFeed.bucket.*` values with the ingest env contract and reader-key mount, the `distant-signal.schedule-bucket` alerts, `scripts/check-schedulefeed-chart.py` and `docs/schedule-feed-bucket.md` ([the chart plan](plans/2026-10-02-gcs-landing-bucket-chart-plan.md)). The schedule-ingest bucket source (the Rust reader: GCS JSON API, key or keyless sign-in, loop guards, a second lane through the existing pipeline, cross-source dedup and precedence) built 2026-10-20 (DS-22), off by default; `expectedKeys` gains `NLC*.xml.gz`. `delivery_source` on both delivery tables (DS-89), a fake-gcs-server end-to-end suite (DS-92) the audit-log tailer (phase 5, DS-90) and the Pub/Sub trigger with its DS values (phase 6, DS-91; Ranma's side is RANMA-87) built 2026-10-21. Not yet: Ranma-Config's project, bucket release, keyless sign-in and kill-switch watcher, then the RDM/DTD destination request and the values flip (`docs/schedule-feed-bucket.md`, "Switching it on") |
| [2026-10-06-line-membership-design](specs/2026-10-06-line-membership-design.md) | implemented | `schedule_query::line_membership`, `scope`/`run_*_crs`/`direction`/`line_due` on the line population, `?scope=` on `/public/lines/{id}/trains` and `/schedule`, catalogue `crs_aliases`/`trunk_for`. The line page's "Trains on this line" reads `view=summary`, whose `scope` defaults to `line,shared` (shared trains in a collapsed group); the full-coverage switch runs `scope` in production (chart `windowedStats.lineMembership: scope` since 2026-10-09; the binary's own default stays `legacy`) |
| [2026-10-06-ingest-architecture-design](specs/2026-10-06-ingest-architecture-design.md) | partly | Phases 0–4 live; phase 5 ([the plan](plans/2026-10-06-ingest-architecture-plan.md), [the runbook](../ingest-phase5-runbook.md)): release A (every producer on its db/stream sink) live 2026-10-10, release B (5.1–5.3b: `/private` routes, HTTP sinks, app publish-staging grants) live 2026-10-13, release C (5.4 api Redis client and `api` ACL user dropped, Q1 `store_*` metrics, 5.4b writing binaries out of the api image, the chart side of 5.6, 5.7 docs) live 2026-10-15 (Helm v256). Not yet: 5.5, blocked because the api, aggregator, enricher and notifier still connect as `distant_signal_app`: phase 0b per service (RANMA-2 to RANMA-5), the 7-day role-usage report (DS-2), the narrowing with ported per-role test fixtures plus the alert rules' `api` alternative and Q8's `INGEST_SINK`/`*_SOURCE` parsing (release D, DS-3), the `app` drop (release E, DS-4, then RANMA-6); Ranma's `allow-egress-same-namespace` narrowing (5.6, RANMA-8) |
| [2026-10-06-incident-source-removal-design](specs/2026-10-06-incident-source-removal-design.md) | implemented | `crates/ds-store/migrations/20261006120000_incidents_source_removed.sql`, `ds_store::incidents::removal`; "Ended (no longer listed)" |
| [2026-10-06-incident-line-evidence-design](specs/2026-10-06-incident-line-evidence-design.md) | implemented | Live 2026-10-06, no shadow mode (user decision): station-evidence line matching and the unplanned cutoff's new anchor and exemptions |
| [2026-10-06-line-page-trains-design](specs/2026-10-06-line-page-trains-design.md) | implemented | Phases 1 and 2 on 2026-10-06, phase 3 on 2026-10-07: `view=summary` (`routes/line_trains_summary.rs`) and the windowed, direction-tabbed "Trains on this line" on `/lines/[id]` |
| [2026-10-06-tiploc-locations-design](specs/2026-10-06-tiploc-locations-design.md) | implemented | `20261007100000_tiploc_locations.sql`, `POST /private/tiploc-locations` from `schedule-reference`; `tiploc:` planner end points |
| [2026-10-06-trips-plan-via-and-max-changes-design](specs/2026-10-06-trips-plan-via-and-max-changes-design.md) | implemented | Pass-through `via` on `/Trips/plan`; `MAX_CHANGES_LIMIT` was 6 (12 since [2026-10-24](specs/2026-10-24-trips-plan-max-changes-12-design.md)) |
| [2026-10-07-trips-plan-or-group-vias-design](specs/2026-10-07-trips-plan-or-group-vias-design.md) | implemented | OR-choice vias and `group:NAME` (`reference-data/station-groups.csv`), `GET /Trips/station-groups`, group pickers on `/plan` |
| [2026-10-09-transit-networks-design](specs/2026-10-09-transit-networks-design.md) | implemented | Manchester Metrolink. Batches 1 to 5 of [the plan](plans/2026-10-09-transit-networks-plan.md) landed by 2026-10-14: `crates/poller-transit-schedule`, the `transit_*` tables, the `/public/transit` routes, `frontend/app/transit/**`, line status, follow and push. Built and dark in prod, every switch off (`api.transitNetworks: []`); [the runbook](../transit-networks.md). Rollout waits on the user (DS-11) |
| [2026-10-14-corpus-nlc-xml-format-design](specs/2026-10-14-corpus-nlc-xml-format-design.md) | implemented | `crates/schedule-ingest/src/{corpus,corpus_xml,corpus_formats}.rs`; the NLC XML snapshot loads beside the JSON extract (`scheduleFeed.corpus.xml`, `formatPrecedence: json` by default); `DistantSignalCorpusFormatDrift`. JSON stays the precedence; using the XML feed in production waits on the Rail Data Marketplace subscription (DS-16) |
| [2026-10-14-ds-mcp-delegated-access-design](specs/2026-10-14-ds-mcp-delegated-access-design.md) | partly | Phase 1 (save, list, read and delete own journeys through scoped grants) is live and switched on in production since 2026-10-15: `d3b6c369`, ds-mcp `91353fc`, [the operator guide](../connected-apps.md). Not yet: phase 2 (dashboards, alerts, "Saved by assistant" in the journeys UI, cap tuning; DS-8) and phase 3 (group reads behind a per-group opt-in; DS-9). Open follow-ups: the end-to-end log check (DS-5) and the share-link items (DS-10) |
| [2026-10-24-nottingham-net-design](specs/2026-10-24-nottingham-net-design.md) | implemented, off | Nottingham NET as the second transit network (DS-160): NET is in no BODS feed, so TNDS East Midlands TransXChange over FTP with a Traveline login; the TXC adapter (`adapter/txc.rs`, `ftp.rs`), `networks/net.toml`, `ds:ingest:transit-net`, `pollerTransitNet`, values `pollerTransitSchedule.networks.net`. Every switch off; needs the TNDS login before rollout ([the runbook](../transit-networks.md)) |
| [2026-10-24-transit-trip-planning-design](specs/2026-10-24-transit-trip-planning-design.md) | partly implemented | Accepted 2026-10-24 (DS-162; Q1 to Q12 decided as proposed). Metrolink phases M1 to M3 built (DS-164), behind `api.tripPlanTransitNetworks` and a `transit=true` opt-in, off by default; M4 is a runbook and a measurement script (docs/transit-networks.md, "Trip planning"). London Trams (LT1 to LT3) not started; superseded for London by [the TfL transit design](specs/2026-10-30-tfl-transit-design.md). |
| [2026-10-24-trips-plan-max-changes-12-design](specs/2026-10-24-trips-plan-max-changes-12-design.md) | implemented | DS-85: `/Trips/plan` `maxChanges` 0-12 (was 0-6), default 4 (was 2, stepping down to 2 where the options search-size guard needs it); measured on prod's timetable |
| [2026-10-30-tfl-transit-design](specs/2026-10-30-tfl-transit-design.md) | implemented, off | TfL's Underground, DLR and London Trams as one `tfl` transit network from TfL's Journey Planner TransXChange (DS-212): the narrowest-active-file override rule, line modes, status from `tfl-<line>` rows, `/transit/tfl`, line frequencies and branches, scheduled times on `/tfl/stops/{id}`. P1 to P4 built, off by default (`api.transitNetworks` and `api.tripPlanTransitNetworks`). Corrects the planning spec's BODS London counts (§16) |

## Plans

81 plans, in `plans/`. Each belongs to the spec with the matching
topic above; its status is that spec's status.
