use anyhow::Result;
use clap::ValueHint;
pub use common::Defaults;
pub use common::config::LineCatalogue;
use common::config::parse_lines;
use serde::de::DeserializeOwned;

fn parse_toml_path<T: DeserializeOwned>(path: &'_ str) -> Result<T> {
    let text = std::fs::read_to_string(path)?;
    Ok(toml::from_str(&text)?)
}

#[derive(Debug, clap::Parser)]
#[expect(
    clippy::struct_excessive_bools,
    reason = "clap arguments: each bool is an independent on/off env setting, not a state machine"
)]
pub struct ServiceArguments {
    #[arg(short, long, env, default_value = "0.0.0.0:8080")]
    pub bind_url: String,
    #[arg(short, long, env)]
    pub database_url: String,
    /// Connection URL for the startup migrations (`ds_store::migrate`) only.
    /// Unset or empty: the migrations use `database_url`, as before.
    ///
    /// Set when the database has separate roles (docs/postgres-app-role.md):
    /// `database_url` is then the non-superuser app role (DML only) every
    /// pool uses, and this is the owner role that owns the schema and may
    /// run DDL. Nothing else in `api` connects with it. Never logged.
    #[arg(long, env, hide_env_values = true)]
    pub migration_database_url: Option<String>,
    #[arg(long, env)]
    pub redis_url: String,

    /// Redis AUTH password (chart `redis.auth`, from a Secret). Unset or
    /// empty: no AUTH, `redis_url` is used as-is. Applied to `redis_url` by
    /// `common::redis_auth::redis_url_with_password`, never logged.
    #[arg(long, env, hide_env_values = true)]
    pub redis_password: Option<common::secret::Secret>,
    /// OIDC issuer base URL for the internal-service `OAuth2` provider
    /// (Authentik) -- JWKS endpoint is learned via standard OIDC
    /// discovery against this URL, same mechanism as `sso_issuer_url`
    /// below. May be the same Authentik instance as `sso_issuer_url` (a
    /// different Application/Provider under it) or a different one --
    /// operator's call.
    /// docs/superpowers/specs/2026-09-02-internal-service-oauth2-design.md
    /// Decision 6.
    #[arg(long, env)]
    pub internal_oauth_issuer_url: String,

    /// Expected `aud` claim on a verified internal-service access token --
    /// the shared Authentik `OAuth2` Provider's own `client_id` (Decision 1:
    /// one provider, 8 service accounts underneath it). Must match every
    /// real caller's own `internal_oauth_client_id` (its own config) --
    /// same value, independently configured on each side.
    #[arg(long, env)]
    pub internal_oauth_client_id: String,

    /// Required Authentik group name per real caller (Decision 3) --
    /// gates which /private/* routes each caller's verified token may
    /// reach. Not secret (a group name isn't confidential). Suggested
    /// defaults only -- an operator's actual Authentik group names are
    /// not mandated by this design.
    #[arg(long, env, default_value = "svc-poller-incidents")]
    pub internal_oauth_group_incidents: String,
    #[arg(long, env, default_value = "svc-poller-stations")]
    pub internal_oauth_group_stations: String,
    #[arg(long, env, default_value = "svc-poller-tocs")]
    pub internal_oauth_group_tocs: String,
    #[arg(long, env, default_value = "svc-poller-ldbws")]
    pub internal_oauth_group_ldbws: String,
    #[arg(long, env, default_value = "svc-poller-tfl")]
    pub internal_oauth_group_tfl: String,
    #[arg(long, env, default_value = "svc-trust-consumer")]
    pub internal_oauth_group_trust_consumer: String,
    #[arg(long, env, default_value = "svc-schedule-ingest")]
    pub internal_oauth_group_schedule_ingest: String,
    #[arg(long, env, default_value = "svc-schedule-reference")]
    pub internal_oauth_group_schedule_reference: String,
    /// Gates every route the (now real, merged) `full-coverage-consumer`
    /// producer surface uses: `POST`/`GET /private/station-full-coverage-samples`
    /// (per-station, `docs/superpowers/specs/2026-09-04-per-station-full-coverage-stats-design.md`
    /// Open Question #4) and `POST`/`GET /private/full-coverage-stats`
    /// (per-line, this plan's own Task 6) -- one producer service, one
    /// credential, both endpoints it writes to. See
    /// docs/superpowers/specs/2026-09-04-option-b-live-consumer-design.md
    /// Decision 5.
    #[arg(long, env, default_value = "svc-full-coverage-consumer")]
    pub internal_oauth_group_full_coverage: String,
    /// Authentik group required to call `/private/trust-event-backlog`.
    /// `trust-backlog-consumer`'s own service-account group.
    #[arg(long, env, default_value = "svc-trust-backlog-consumer")]
    pub internal_oauth_group_trust_backlog: String,
    /// Gates `POST`/`GET /private/island-of-ireland-stations` and
    /// `/island-of-ireland-lines` -- the new `poller-irish-rail-gtfs`
    /// crate's own credential. See
    /// docs/superpowers/plans/2026-09-05-ireland-rail-support-plan.md Task A3.
    ///
    /// Empty by default (2026-09-30), as are the other two island-of-Ireland
    /// groups below: those pollers ship disabled and no such Authentik group
    /// exists, so the api no longer names one. Empty closes the routes it
    /// gates (every caller gets `403`); set it when enabling the poller.
    #[arg(long, env, default_value = "")]
    pub internal_oauth_group_irish_rail_gtfs: String,
    /// Gates `POST`/`GET /private/island-of-ireland-station-samples` -- the
    /// new `poller-irish-rail-live` crate's own credential. See
    /// docs/superpowers/plans/2026-09-05-ireland-rail-support-plan.md Task B3.
    /// Empty by default: see `internal_oauth_group_irish_rail_gtfs`.
    #[arg(long, env, default_value = "")]
    pub internal_oauth_group_irish_rail_live: String,
    /// Gates `POST`/`GET /private/island-of-ireland-stations` and
    /// `/island-of-ireland-lines` ALONGSIDE `poller-irish-rail-gtfs`'s own
    /// credential above -- `poller-nir-stations`'s own credential. Two
    /// independent producer services write to these same two tables (one
    /// per island-of-ireland network); each keeps its own service
    /// identity rather than sharing `internal_oauth_group_irish_rail_gtfs`,
    /// matching this file's existing one-producer-one-credential
    /// convention. See
    /// docs/superpowers/plans/2026-09-05-nir-tier-a-implementation-plan.md
    /// Task 1. Empty by default: see `internal_oauth_group_irish_rail_gtfs`.
    #[arg(long, env, default_value = "")]
    pub internal_oauth_group_nir_stations: String,
    /// Gates `POST /private/corpus-locations` -- Network Rail CORPUS loads
    /// from `schedule-ingest`'s CORPUS mode
    /// (docs/superpowers/specs/2026-09-28-corpus-sftp-ingest-design.md).
    /// Its own group rather than `internal_oauth_group_schedule_ingest`, so
    /// replacing the whole location table is a grant made on purpose: the
    /// schedule-ingest service account is added to this group in Authentik
    /// when CORPUS ingest is enabled.
    #[arg(long, env, default_value = "svc-corpus-ingest")]
    pub internal_oauth_group_corpus: String,
    /// Authentik group of the Distant-Signal-MCP's service account (a
    /// separate service in its own namespace that calls the PUBLIC routes
    /// in-cluster). Unlike the groups above it gates no `/private/*` route:
    /// a verified token carrying it only moves that caller's rate-limited
    /// public requests onto their own finite budget (`API_RATE_LIMIT_MCP_*`,
    /// keyed `svc:mcp`) instead of the per-IP one. See `crate::rate_limit`'s
    /// module doc. Empty turns the feature off: every bearer on a public
    /// route is then ignored, as before this existed. Must not equal any
    /// `/private/*` group (checked at startup).
    #[arg(long, env, default_value = "srv-ds-mcp")]
    pub internal_oauth_group_mcp: String,
    /// Authentik/SSO group (via the `groups` OIDC claim, already decoded
    /// into `AuthenticatedUser.groups` on every login -- see
    /// `crates/api/src/auth/oidc.rs` and `data::users::upsert_user`) that
    /// grants access to the embedded chatbot (`ChatbotAuthorizedUser`,
    /// `crates/api/src/auth.rs`). This is an END-USER access group, NOT one
    /// of the `internal_oauth_group_*` fields above -- those gate
    /// machine/service-account credentials on `/private/*` routes; this one
    /// gates a real person's own SSO session on `GET /public/chatbot/access`.
    /// Not secret (a group name isn't confidential). Suggested default only
    /// -- an operator's actual Authentik group name is not mandated by this
    /// design. Supersedes the former per-user `chatbot_allowed_users` DB
    /// allowlist (dropped; see the migration removing it).
    ///
    /// Only consulted when `chatbot_access` is [`ChatbotAccessMode::Group`]
    /// (the default). In that mode an empty value means NOBODY gets the
    /// chatbot (no stored group is ever blank), exactly as before
    /// `chatbot_access` existed; use `CHATBOT_ACCESS=authenticated`, not an
    /// empty group, to open it to every logged-in user.
    #[arg(long, env, default_value = "distant-signal-chatbot-users")]
    pub chatbot_access_group: String,

    /// Who gets the embedded chatbot (`/chat`, `GET /public/chatbot/access`):
    /// `group` (the default) -- only logged-in users in
    /// `chatbot_access_group`; `authenticated` -- every logged-in user,
    /// whatever their groups. Logged-out requests are refused (`401`) in both
    /// modes. A deploy-time switch so the ungating ships dark and is flipped
    /// together with distant-signal-mcp's own ungating.
    #[arg(long, env, value_enum, default_value_t = ChatbotAccessMode::Group)]
    pub chatbot_access: ChatbotAccessMode,

    /// Authentik/SSO end-user group (read from the same `groups` OIDC claim
    /// as `chatbot_access_group`) whose members may call the admin
    /// session-revocation endpoint, `POST /public/admin/users/revoke-sessions`
    /// (`routes::admin`). EMPTY BY DEFAULT, and empty means the feature is
    /// off: nobody is an admin and that endpoint answers `403` to everyone.
    ///
    /// Like every group-gated check in this crate, membership is read from
    /// `users.groups`, which is written only at login. Adding someone to this
    /// group takes effect at their next login; removing them takes effect at
    /// their next login, or immediately if another admin revokes their
    /// sessions. See docs/session-revocation.md.
    #[arg(long, env, default_value = "")]
    pub admin_group: String,

    /// OIDC issuer base URL (e.g. `https://sso.example.com/realms/rail`).
    /// `crates/api` discovers every other endpoint (authorization, token,
    /// JWKS) from this single URL's `.well-known/openid-configuration`
    /// document -- see the design doc's OIDC-over-SAML research for why.
    /// No default: every deployment must point this at its own
    /// operator-run/subscribed SSO server. Discovery itself is lazy (see
    /// this plan's Global Constraints) -- this field is only syntactically
    /// validated at startup, not dereferenced over the network.
    #[arg(long, env)]
    pub sso_issuer_url: String,

    /// OIDC client id this app is registered as with the issuer above.
    #[arg(long, env)]
    pub sso_client_id: String,

    /// OIDC client secret paired with `sso_client_id`. A genuinely new
    /// *kind* of secret for this crate -- every other credential this
    /// crate's own config used to hold (the removed shared-secret field
    /// this design retired; the RDM API keys living in sibling pollers'
    /// own configs) is a single shared/bearer token, not a paired `OAuth2`
    /// confidential-client secret -- but handled with the same posture:
    /// env-only, required, never
    /// logged. `ServiceArguments` derives `Debug`; avoid ever logging
    /// `app.config` wholesale (nothing in this codebase does today) --
    /// log individual non-secret fields instead if a future debug log
    /// needs to reference config.
    #[arg(long, env)]
    pub sso_client_secret: String,

    /// The exact redirect URI registered with the SSO server for the
    /// authorization-code callback. Deliberately NOT this service's own
    /// origin -- it must be the *frontend's* public origin plus
    /// `/api/auth/callback` (e.g.
    /// `https://rail.example.com/api/auth/callback`), proxied through to
    /// this crate's `/public/auth/callback` by
    /// `frontend/app/api/[...path]/route.ts` (Task 8). If this pointed at
    /// `crates/api`'s own origin instead, the `Set-Cookie` the callback
    /// handler issues would be scoped to `api`'s origin, not the origin
    /// the browser subsequently talks to for every other request -- the
    /// session cookie would never come back. See the design doc's Session
    /// architecture section.
    #[arg(long, env)]
    pub sso_redirect_url: String,

    /// Where `/auth/callback` and `/auth/logout` send the browser once
    /// they're done, WHEN no per-attempt return path was captured or the
    /// one captured failed validation -- the frontend's own root URL (e.g.
    /// `https://rail.example.com/`). No longer the sole destination for
    /// every successful login: see `routes::auth::callback` and
    /// `auth::validate_return_to` for the per-login-attempt `return_to`
    /// this now falls back from. docs/superpowers/specs/2026-08-31-dynamic-post-login-redirect-design.md.
    #[arg(long, env)]
    pub sso_post_login_redirect_url: String,

    /// Session lifetime in days: a FIXED expiry stamped once at sign-in,
    /// not a sliding window. `sessions.expires_at` is set to
    /// `NOW() + this` by `data::users::insert_session` and never touched
    /// again -- no code path anywhere extends it on activity, so a session
    /// dies exactly this many days after login however heavily it was
    /// used. (The design doc's "Expiry and refresh" section describes a
    /// sliding window instead; that is unimplemented, and this doc comment
    /// used to claim it. If you implement it, the write goes in
    /// `auth::AuthenticatedUser::from_request_parts`, the one place every
    /// authenticated request resolves its session.) Design doc proposes 14
    /// as a starting figure, not researched further there; kept
    /// configurable since it's a product/ops tuning knob, not a protocol
    /// constant.
    #[arg(long, env, default_value_t = 14)]
    pub session_ttl_days: i64,

    /// How many days of `line_status_history` rows the aggregator actually
    /// keeps before `queries::prune_history` (`crates/aggregator`) deletes
    /// them. This crate never reads or prunes that table itself -- the
    /// only reason this field exists here is so `/public/history-retention`
    /// (`routes/history_retention.rs`) can hand the frontend's history
    /// range picker the real ceiling, instead of the frontend guessing or
    /// hardcoding a number that could silently drift from what's actually
    /// configured. Deployments MUST set this to the same value they give
    /// the aggregator's own `HISTORY_RETENTION_DAYS` -- `docker-compose.yml`
    /// and the Helm chart (`values.yaml`'s `aggregator.historyRetentionDays`)
    /// both source both services' env vars from the one value, but nothing
    /// in this crate enforces the two staying in sync beyond that
    /// convention.
    #[arg(long, env, default_value_t = 7)]
    pub history_retention_days: i64,

    /// How many days of `line_status_daily_stats` rows the aggregator
    /// actually keeps before `queries::prune_daily_stats` deletes them.
    /// This crate never reads or prunes that table itself -- the only
    /// reason this field exists here is so `/public/history-retention`
    /// (`routes/history_retention.rs`) can hand the frontend's Trends-tab
    /// granularity control the real ceiling. Deployments MUST set this to
    /// the same value they give the aggregator's own
    /// `DAILY_STATS_RETENTION_DAYS` -- same convention as
    /// `history_retention_days`, above.
    #[arg(long, env, default_value_t = 300)]
    pub daily_stats_retention_days: i64,

    /// How many hours of `line_status_half_hourly_stats` rows the
    /// aggregator actually keeps before `queries::prune_half_hourly_stats`
    /// deletes them. Same "static config echo, never enforced here"
    /// posture as `history_retention_days`/`daily_stats_retention_days`
    /// above. Deployments MUST set this to the same value they give the
    /// aggregator's own `HALF_HOURLY_STATS_RETENTION_HOURS`.
    #[arg(long, env, default_value_t = 840)]
    pub half_hourly_stats_retention_hours: i64,

    /// Whether to expose `/metrics` at all and count requests into it.
    /// Unlike the other 7 binaries, `api`'s own public HTTP listener stays
    /// up regardless (it's the main service) -- this only controls whether
    /// requests are counted and whether the SEPARATE internal-only
    /// `/metrics` listener (`metrics_port`, below) is started at all.
    #[arg(long, env, default_value_t = true)]
    pub metrics_enabled: bool,

    /// Port for `api`'s own internal-only Prometheus `/metrics` listener --
    /// bound and served by a SEPARATE `axum::serve` task
    /// (`main::spawn_metrics_listener`) from the one `bind_url` binds for
    /// public traffic.
    ///
    /// 2026-09-25 Signal Box Audit Low finding: until this field existed,
    /// `/metrics` was registered as an ordinary route on the SAME router as
    /// every public endpoint, sharing `bind_url`'s listener/port with no
    /// authentication of its own. That listener is also exactly what the
    /// chart's Ingress resource (`charts/distant-signal/templates/ingress.yaml`)
    /// points at with a catch-all `path: / (Prefix)` rule when
    /// `ingress.api.enabled` is true -- and `NetworkPolicy` (pod-to-pod only)
    /// cannot see, let alone block, traffic arriving through an Ingress
    /// controller. So whenever an operator had BOTH `metrics_enabled` and
    /// `ingress.api.enabled` set (both common), `/metrics` -- read-only
    /// request-count/latency telemetry, not a secret, but still internal
    /// operational detail -- was reachable by anyone on the public
    /// internet, unauthenticated.
    ///
    /// Fixed the same way every other binary in this workspace already
    /// serves its own `/metrics`, via a listener the Ingress never routes
    /// to and that the chart's `NetworkPolicy` scopes to the monitoring
    /// namespace alone (see `networkpolicy.yaml`'s api-metrics rule) --
    /// this crate just can't reuse `common::metrics::install` verbatim like
    /// they do, since it already drives `axum-prometheus`'s
    /// `PrometheusMetricLayerBuilder` off its own public router rather than
    /// installing `metrics-exporter-prometheus` directly (see that
    /// module's own doc comment). `main::spawn_metrics_listener` instead
    /// stands up a second, tiny `axum::Router` serving only `GET /metrics`,
    /// fed by the SAME `PrometheusHandle` the request-counting middleware
    /// on the public listener already produces -- one shared recorder, two
    /// listeners.
    ///
    /// Default `9091` matches the chart-wide `metrics.port` default every
    /// other workload (aggregator/enricher/every poller) already uses --
    /// safe to share the same numeric default across every workload since
    /// each lives in its own Pod's network namespace, never colliding with
    /// another container's listener.
    #[arg(long, env, default_value_t = 9091)]
    pub metrics_port: u16,

    #[arg(long, value_parser = parse_toml_path::<Defaults>, value_hint = ValueHint::FilePath, value_name = "FILE")]
    pub defaults_file: Option<Defaults>,
    /// Directory of line-catalogue TOML files, loaded once at startup.
    /// Defaults to `/app/lines` (baked into the Docker image — see
    /// `docker/api.Dockerfile`), overridable via `LINES_DIR` for local
    /// (non-Docker) runs.
    #[arg(long = "lines-dir", env = "LINES_DIR", default_value = "/app/lines", value_parser = parse_lines, value_hint = ValueHint::FilePath, value_name = "DIR")]
    pub lines: LineCatalogue,

    /// The VAPID public key `crates/notifier` signs push messages with —
    /// handed to the browser's `PushManager.subscribe({ applicationServerKey })`
    /// call unchanged. The matching PRIVATE key lives only in `crates/notifier`'s
    /// own config -- `api` never needs it, since `api` only stores
    /// subscriptions, it never sends to them.
    #[arg(long, env)]
    pub vapid_public_key: String,

    /// Global override for `LineDefinition.full_coverage_enabled`
    /// (Decision 3's per-line TOML rollout gate, `crates/common/src/lib.rs`).
    /// When `true`, `data::station_stats::full_coverage_enabled_for`
    /// treats EVERY catalogued line as full-coverage-enabled, regardless
    /// of what its own `lines/*.toml` entry sets -- the SAME global flag
    /// name/semantics as `crates/aggregator`'s own
    /// `full_coverage_enabled_default` (both services independently gate
    /// on `LineDefinition.full_coverage_enabled`, so both need the
    /// override). Default `false` is deliberate: this flag must never
    /// silently change behavior for a deployment that doesn't explicitly
    /// set it, and `true` is never baked in here as the default (that
    /// would require a rebuild to ever revert) -- an operator opts in via
    /// this env var / the Helm chart's `api.fullCoverageEnabledDefault`
    /// value.
    #[arg(long, env, default_value_t = false)]
    pub full_coverage_enabled_default: bool,

    /// How often `api`'s own background schedule-match sweep re-attempts
    /// Decision 3's schedule-first resolution against every still-`pending`
    /// tracked-train row -- the retroactive-fix mechanism for a pin
    /// created before its service's `schedule_line_population` cycle ran,
    /// or before this feature shipped at all
    /// (docs/superpowers/specs/2026-09-05-schedule-first-train-tracking-design.md
    /// Decision 6). Same "plain interval, no jitter" shape as
    /// `schedule-reference`'s own `poll_interval_secs`
    /// (`crates/schedule-reference/src/config.rs:25`). 300s (5 minutes)
    /// default -- frequent enough that a newly-published
    /// `schedule_line_population` row is picked up within a rail day's
    /// working hours, cheap enough (a handful of still-pending rows on a
    /// typical day) not to matter at this cadence. Deliberately NOT wired
    /// into the Helm chart -- "default suffices, override via env if an
    /// operator ever needs to" posture. Unlike its three sibling sweep
    /// intervals below (`reconciliation_sweep_interval_secs`,
    /// `backlog_match_sweep_interval_secs`, `session_cleanup_interval_secs`,
    /// all wired into `api-deployment.yaml` as part of the 2026-09-25
    /// Signal Box Audit Low pass), this ONE field's "unwired" state is a
    /// deliberate, this-comment-carries-it decision, not an oversight --
    /// kept unwired specifically so at least one example survives of an
    /// intentionally-not-chart-exposed tunable, rather than every field in
    /// this struct ending up wired by convention alone.
    #[arg(long, env, default_value_t = 300)]
    pub schedule_match_interval_secs: u64,

    /// How often the reconciliation sweep re-attempts (1) flipping a
    /// `train_subscriptions` row stuck at `resolution_status = 'pending'`
    /// once `train_movement_events` proves the train was tracked, and (2)
    /// schedule-enriching an NR-primary tracked train from
    /// `schedule_destination_departures` when TRUST backlog had nothing at
    /// track-creation time. See
    /// docs/superpowers/specs/2026-09-08-tracked-train-reconciliation-design.md
    /// Decision 5. 300s default, reusing `schedule_match_interval_secs`'s
    /// own reasoning for the identical class of concern -- both halves of
    /// this sweep are cheap enough at this cadence not to matter, and
    /// frequent enough that a stuck row is fixed within a rail day's
    /// working hours.
    #[arg(long, env, default_value_t = 300)]
    pub reconciliation_sweep_interval_secs: u64,

    /// How long past a candidate's true origin departure the schedule-
    /// enrichment half of the reconciliation sweep waits before attempting
    /// a CIF-only match -- a courtesy to the live TRUST/backlog paths' own
    /// normal resolution window (`common::MATCH_TOLERANCE`, ±20 minutes),
    /// not a data-availability requirement. See the design doc's Decision
    /// 3. 30 minutes default, at the upper (more conservative) end of that
    /// document's own suggested 15-30 minute range.
    #[arg(long, env, default_value_t = 30)]
    pub schedule_enrichment_grace_minutes: i64,

    /// How often the backlog-match sweep re-attempts
    /// `trust_event_backlog_match::attempt_backlog_match` against every
    /// still-`pending` pin `attempt_backlog_match` hasn't yet resolved.
    /// Fixes a real gap: that function was, until this field's own sweep
    /// loop, only ever called once, synchronously, at pin-creation time
    /// (`routes::train::post_track`) -- typically before the tracked
    /// train has even departed, when `trust_event_backlog` has nothing
    /// for it yet. A pin whose real departure lands outside
    /// `common::MATCH_TOLERANCE` of its `resolve_origin_departure` check
    /// (`trust-consumer`, a common occurrence under disruption) then had
    /// no retry at all, despite the backlog filling in with exactly the
    /// row a later attempt would match, over the following hours -- same
    /// class of "one-shot attempt, no periodic retry" gap
    /// `schedule_match_interval_secs`/`reconciliation_sweep_interval_secs`
    /// already exist to close for their own concerns. 300s default,
    /// reusing those two fields' own reasoning for the identical class of
    /// concern -- cheap enough at this cadence (a handful of still-pending
    /// rows on a typical day) not to matter, frequent enough that a
    /// backlog row landing mid-day is picked up within the same rail day.
    #[arg(long, env, default_value_t = 300)]
    pub backlog_match_sweep_interval_secs: u64,

    /// How often `api`'s own background session-cleanup sweep deletes
    /// expired `sessions` rows (`data::users::prune_expired_sessions`).
    /// Nothing else in this crate ever prunes that table --
    /// `get_session_with_user` already excludes an expired row from every
    /// lookup (`WHERE s.expires_at > NOW()`), so a stale row is never
    /// usable as a live session; this sweep exists purely to keep the
    /// table (and its `sessions_expires_at` index, added alongside this
    /// field) from growing without bound on a long-lived deployment. Same
    /// "plain interval, no jitter" shape as
    /// `schedule_match_interval_secs`/`reconciliation_sweep_interval_secs`/
    /// `backlog_match_sweep_interval_secs` above. 3600s (1 hour) default --
    /// deleting expired rows is cheap and idempotent at this cadence, and
    /// there is no correctness reason to run it any more often than that
    /// (unlike the other three sweeps, this one never resolves anything a
    /// user is waiting on).
    #[arg(long, env, default_value_t = 3600)]
    pub session_cleanup_interval_secs: u64,

    /// Run the api's background loops (ingest architecture plan 1B.7):
    /// the schedule-match, reconciliation and backlog-match sweeps, the
    /// one-shot CORPUS crosswalk check at startup, and
    /// `session_cleanup_sweep_loop`. **On by default** (today's behaviour).
    /// The first four take the same advisory locks as the ingest-writer's
    /// loops (`ds_store::loops`), so with both on each sweep runs in one
    /// process at a time. Turned off (`api.backgroundLoops: false`) once
    /// the writer's loops (`ingestWriter.loops.enabled`) and the
    /// `apiMaintenance` `CronJob` run them instead (spec §12.3).
    #[arg(long, env = "API_BACKGROUND_LOOPS", default_value_t = true, action = clap::ArgAction::Set)]
    pub background_loops: bool,

    /// `POST /private/incidents`' row heartbeat (ingest architecture plan
    /// 2c.6; `ds_store::incidents::RowHeartbeat`). **On by default**
    /// (today's behaviour): every listed incident gets `fetched_at = NOW()`
    /// on every poll, about 600k row updates a day. Off: only changed rows
    /// are written and the display time comes from
    /// `incident_feed_state.last_snapshot_at`. The chart renders it from
    /// `pollers.incidents.ingest.rowHeartbeat`, the same value
    /// poller-incidents' DB sink reads, so the two writers cannot disagree.
    #[arg(long, env = "INCIDENTS_ROW_HEARTBEAT", default_value_t = true, action = clap::ArgAction::Set)]
    pub incidents_row_heartbeat: bool,

    /// Personal-data retention (UK legal audit LEG-5, 2026-09-27): days
    /// after its travel date that a tracked train (with its attached
    /// tickets, notification state and group shares), a standalone ticket,
    /// a journey (with its legs and share link) and a journey template's
    /// "skip this date" marker are kept before
    /// `data::retention::prune_personal_data` deletes them. Checked hourly
    /// by `session_cleanup_sweep_loop`. Default 548 days (18 months): the
    /// audit's suggested figure, covering Delay Repay's 28-day claim window
    /// with plenty of room for personal reliability history. 0 disables it.
    /// Journey templates, groups, pins and custom lines are never pruned by
    /// age. See docs/personal-data-retention.md.
    #[arg(long, env, default_value_t = 548)]
    pub past_travel_retention_days: i64,

    /// Personal-data retention: push subscriptions are deleted once their
    /// user has not logged in for this many days AND the subscription has
    /// not been renewed in that time. `last_seen_at` alone is not an
    /// activity signal (the browser only refreshes it on re-subscribe),
    /// hence the login condition. Default 365. 0 disables it.
    #[arg(long, env, default_value_t = 365)]
    pub stale_push_subscription_days: i64,

    /// Personal-data retention: whole accounts with no login for this many
    /// days and no live session are deleted, through the same path as
    /// self-service deletion (`data::account::delete_account`), so owned
    /// groups are handed over rather than lost. **Off by default (0)**:
    /// the app holds no email addresses, so there is no way to warn a
    /// user first, and turning this on is an operator decision that the
    /// privacy notice must state. The audit suggests 730 (24 months).
    #[arg(long, env, default_value_t = 0)]
    pub inactive_account_retention_days: i64,
}

/// `ServiceArguments::chatbot_access`: who gets the embedded chatbot.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, clap::ValueEnum)]
pub enum ChatbotAccessMode {
    /// Only logged-in users in `ServiceArguments::chatbot_access_group`.
    #[default]
    Group,
    /// Every logged-in user; `chatbot_access_group` is ignored.
    Authenticated,
}

impl ChatbotAccessMode {
    /// The wire/env spelling, also returned by `GET /public/chatbot/access`
    /// so the frontend can word its access notes to match.
    pub fn as_str(self) -> &'static str {
        match self {
            ChatbotAccessMode::Group => "group",
            ChatbotAccessMode::Authenticated => "authenticated",
        }
    }
}

/// LEG-6 (UK GDPR data minimisation): `users.groups` keeps only the `IdP`
/// groups something reads, not every group the SSO user belongs to (the
/// production `IdP` maps unrelated groups such as `grafana-access` into the
/// claim). The groups read are:
///
/// - `chatbot_access_group` (the `/chat` gate), only while `chatbot_access`
///   is `group` -- in `authenticated` mode nothing reads it, so it is not
///   kept (switching back to `group` then takes effect per user at their
///   next login, like any other group change);
/// - `admin_group`, when set (admin session revocation);
/// - any extra groups listed (comma-separated) in [`STORED_GROUPS_EXTRA_ENV`].
///   None by default: the `distant-signal-mcp` adapter used to read
///   `mcp-users`/`mcp-live-boards` back from `GET /public/auth/session`, but
///   since 2026-09-29 it takes groups straight from Authentik, so nothing
///   here reads them.
pub const STORED_GROUPS_EXTRA_ENV: &str = "OIDC_STORED_GROUPS_EXTRA";

/// The default for [`STORED_GROUPS_EXTRA_ENV`]: no extra groups.
pub const DEFAULT_STORED_GROUPS_EXTRA: &str = "";

impl ServiceArguments {
    /// The `IdP` groups a login may store on `users.groups`; see
    /// [`STORED_GROUPS_EXTRA_ENV`].
    pub fn stored_group_allowlist(&self) -> Vec<String> {
        let extra = std::env::var(STORED_GROUPS_EXTRA_ENV)
            .unwrap_or_else(|_| DEFAULT_STORED_GROUPS_EXTRA.to_string());
        let chatbot_group = match self.chatbot_access {
            ChatbotAccessMode::Group => self.chatbot_access_group.as_str(),
            ChatbotAccessMode::Authenticated => "",
        };
        stored_group_allowlist(chatbot_group, &self.admin_group, &extra)
    }
}

/// [`ServiceArguments::stored_group_allowlist`] without the environment.
/// Blank entries are dropped, so an empty `admin_group` allows nothing.
pub fn stored_group_allowlist(
    chatbot_access_group: &str,
    admin_group: &str,
    extra: &str,
) -> Vec<String> {
    let mut allowed: Vec<String> = Vec::new();
    for group in [chatbot_access_group, admin_group]
        .into_iter()
        .chain(extra.split(','))
        .map(str::trim)
        .filter(|group| !group.is_empty())
    {
        if !allowed.iter().any(|existing| existing == group) {
            allowed.push(group.to_string());
        }
    }
    allowed
}

#[cfg(test)]
mod stored_group_allowlist_tests {
    use super::stored_group_allowlist;

    #[test]
    fn includes_the_chatbot_admin_and_extra_groups() {
        assert_eq!(
            stored_group_allowlist("chat", "admins", "mcp-users, mcp-live-boards"),
            ["chat", "admins", "mcp-users", "mcp-live-boards"]
        );
    }

    #[test]
    fn an_empty_admin_group_or_extra_list_allows_nothing_extra() {
        assert_eq!(stored_group_allowlist("chat", "", ""), ["chat"]);
        assert_eq!(stored_group_allowlist("chat", "  ", " , "), ["chat"]);
    }

    #[test]
    fn duplicates_are_listed_once() {
        assert_eq!(
            stored_group_allowlist("chat", "chat", "chat,x"),
            ["chat", "x"]
        );
    }
}

/// The one invariant this crate cannot check at compile time and that has now
/// been broken in production: **every `INTERNAL_OAUTH_GROUP_*` env var this
/// `ServiceArguments` declares must also be set on the `api` container in
/// `charts/distant-signal/templates/api-deployment.yaml`.**
///
/// This is the same class of declared-but-unwired bug that
/// `crates/schedule-reference/src/config.rs`'s own `chart_env_wiring_tests`
/// module exists to prevent, and it is dangerous for the same reason: every
/// group field above carries a *suggested default*, so a var the chart forgets
/// to set does NOT fail fast. clap is satisfied, `main` starts, and `api`
/// silently enforces the crate's own suggested group name instead of the one
/// the operator actually created in Authentik.
///
/// What that looks like in production: `require_internal_oauth` compares the
/// caller's verified `groups` claim against the wrong name, so EVERY
/// `/private/*` request from that one caller is rejected `403` -- forever, and
/// only for that caller, so nothing else looks broken. That is exactly what
/// happened to `INTERNAL_OAUTH_GROUP_TRUST_BACKLOG`: declared 2026-09-05
/// alongside `/private/trust-event-backlog`, but never added to
/// `api-deployment.yaml`, which wired the other 12 group vars. Every
/// `POST /private/trust-event-backlog` from `trust-backlog-consumer` 403'd,
/// and because that crate's backlog is a Redis queue it drains only on
/// success, the observable symptom was an ever-growing Redis key -- not an
/// error anyone was watching. `values.yaml`'s own comment still said "8 group
/// fields" when the struct had 13, which is how far this had already drifted.
///
/// A test is what makes the next one impossible to ship.
#[cfg(test)]
mod chart_env_wiring_tests {
    use clap::CommandFactory;

    use super::ServiceArguments;

    /// Prefix shared by every machine-credential group field above.
    /// Deliberately NOT matched against `CHATBOT_ACCESS_GROUP`, which is an
    /// end-user SSO group, not one of these -- see its own doc comment.
    const GROUP_ENV_PREFIX: &str = "INTERNAL_OAUTH_GROUP_";

    /// The `api` container's own slice of the api Deployment template. Scoped
    /// from its `- name: api` line to EOF rather than matching the whole file,
    /// matching `crates/schedule-reference/src/config.rs`'s equivalent helper
    /// -- so a var that only ever appears in one of this template's leading
    /// `fail`-guard comment blocks cannot satisfy the check by accident.
    /// `api` is the only container in this template, so "to EOF" is the whole
    /// block.
    fn api_container_block() -> String {
        let chart = common::manifest_dir!()
            .join("../../charts/distant-signal/templates/api-deployment.yaml");
        let rendered = std::fs::read_to_string(&chart)
            .unwrap_or_else(|err| panic!("read {}: {err}", chart.display()));
        let marker = "- name: api\n";
        let start = rendered.find(marker).expect(
            "the api Deployment must still declare a container named `api`; if it was renamed, \
             update this test's marker",
        );
        rendered[start..].to_string()
    }

    #[test]
    fn every_internal_oauth_group_this_config_declares_is_set_on_the_charts_api_container() {
        let block = api_container_block();
        let command = ServiceArguments::command();

        let declared: Vec<String> = command
            .get_arguments()
            .filter_map(|arg| arg.get_env().and_then(|env| env.to_str()))
            .filter(|env| env.starts_with(GROUP_ENV_PREFIX))
            .map(str::to_string)
            .collect();
        assert!(
            declared.len() >= 13,
            "sanity check: this ServiceArguments declares one {GROUP_ENV_PREFIX}* env var per \
             real /private/* caller (13 of them as of 2026-09-25); got {declared:?}"
        );

        let missing: Vec<&String> = declared
            .iter()
            .filter(|env| !block.contains(&format!("- name: {env}")))
            .collect();

        assert!(
            missing.is_empty(),
            "these {GROUP_ENV_PREFIX}* env vars are declared by crates/api/src/data/config.rs but \
             never set on the `api` container in \
             charts/distant-signal/templates/api-deployment.yaml, so under Helm api silently \
             enforces this crate's own suggested group name instead of the operator's real \
             Authentik group -- every /private/* request from the affected caller then 403s \
             forever, with no startup failure and no error on api's side: {missing:?}"
        );
    }

    /// The chart must not set a group var this struct no longer declares
    /// either: a stale `INTERNAL_OAUTH_GROUP_*` in the template is a value an
    /// operator can configure that silently does nothing, and it is the same
    /// drift in the opposite direction.
    #[test]
    fn the_chart_sets_no_internal_oauth_group_this_config_does_not_declare() {
        let block = api_container_block();
        let command = ServiceArguments::command();

        let declared: Vec<String> = command
            .get_arguments()
            .filter_map(|arg| arg.get_env().and_then(|env| env.to_str()))
            .filter(|env| env.starts_with(GROUP_ENV_PREFIX))
            .map(str::to_string)
            .collect();

        let stale: Vec<&str> = block
            .lines()
            .filter_map(|line| line.trim().strip_prefix("- name: "))
            .filter(|env| env.starts_with(GROUP_ENV_PREFIX))
            .filter(|env| !declared.iter().any(|d| d == env))
            .collect();

        assert!(
            stale.is_empty(),
            "charts/distant-signal/templates/api-deployment.yaml sets these \
             {GROUP_ENV_PREFIX}* env vars on the api container, but \
             crates/api/src/data/config.rs no longer declares them, so clap ignores them and any \
             operator who configures one gets no effect at all: {stale:?}"
        );
    }

    /// `CHATBOT_ACCESS` has a safe default (`group`), so an unwired var would
    /// not fail -- it would just make the coordinated ungating impossible to
    /// flip from values.yaml. Also pins the chart's default to `group`.
    #[test]
    fn chatbot_access_is_set_on_the_charts_api_container() {
        let block = api_container_block();
        let declared = ServiceArguments::command()
            .get_arguments()
            .filter_map(|arg| arg.get_env().and_then(|env| env.to_str()))
            .any(|env| env == "CHATBOT_ACCESS");
        assert!(declared, "config.rs must still declare CHATBOT_ACCESS");
        assert!(
            block.contains("- name: CHATBOT_ACCESS\n"),
            "CHATBOT_ACCESS is declared by crates/api/src/data/config.rs but never set on the \
             `api` container in charts/distant-signal/templates/api-deployment.yaml"
        );
        let values = std::fs::read_to_string(
            common::manifest_dir!().join("../../charts/distant-signal/values.yaml"),
        )
        .expect("read values.yaml");
        assert!(
            values.contains("\n  chatbotAccess: group\n"),
            "api.chatbotAccess must default to `group` so shipping this code changes nothing"
        );
    }

    #[test]
    fn chatbot_access_parses_both_modes_and_defaults_to_group() {
        use super::ChatbotAccessMode;
        use clap::ValueEnum;
        assert_eq!(ChatbotAccessMode::default(), ChatbotAccessMode::Group);
        for mode in ChatbotAccessMode::value_variants() {
            assert_eq!(
                ChatbotAccessMode::from_str(mode.as_str(), false),
                Ok(*mode),
                "as_str must match the clap/env spelling"
            );
        }
        assert!(ChatbotAccessMode::from_str("", false).is_err());
    }

    /// Narrower cousin of the two tests above, for the four sweep-cadence
    /// tunables wired into `api-deployment.yaml` in the same 2026-09-25
    /// Signal Box Audit Low pass that added this test:
    /// `reconciliation_sweep_interval_secs`, `schedule_enrichment_grace_minutes`,
    /// `backlog_match_sweep_interval_secs` and `session_cleanup_interval_secs`.
    /// Each had a working default and so was a silent "can't tune without a
    /// redeploy" gap rather than the group-var tests' silent-403 hazard --
    /// still the same underlying bug class, still worth a guard.
    /// `schedule_match_interval_secs` is deliberately excluded: its own doc
    /// comment in this file explains why it stays unwired on purpose.
    /// Plan 1B.7: `API_BACKGROUND_LOOPS` defaults to true (today's
    /// behaviour) and takes an explicit value, so the chart's
    /// `API_BACKGROUND_LOOPS=false` turns the loops off.
    #[test]
    fn background_loops_default_on_and_the_env_name_is_stable() {
        let command = ServiceArguments::command();
        let arg = command
            .get_arguments()
            .find(|arg| arg.get_id() == "background_loops")
            .expect("background_loops is declared");
        assert_eq!(
            arg.get_env().and_then(|env| env.to_str()),
            Some("API_BACKGROUND_LOOPS")
        );
        assert_eq!(arg.get_default_values(), ["true"]);
        assert!(matches!(arg.get_action(), clap::ArgAction::Set));
        let block = api_container_block();
        assert!(
            block.contains("- name: API_BACKGROUND_LOOPS"),
            "the chart renders API_BACKGROUND_LOOPS (when api.backgroundLoops is false)"
        );
    }

    /// Plan 2c.6: `INCIDENTS_ROW_HEARTBEAT` defaults to true (today's
    /// behaviour), takes an explicit value, and the chart can turn it off.
    #[test]
    fn incidents_row_heartbeat_default_on_and_the_env_name_is_stable() {
        let command = ServiceArguments::command();
        let arg = command
            .get_arguments()
            .find(|arg| arg.get_id() == "incidents_row_heartbeat")
            .expect("incidents_row_heartbeat is declared");
        assert_eq!(
            arg.get_env().and_then(|env| env.to_str()),
            Some("INCIDENTS_ROW_HEARTBEAT")
        );
        assert_eq!(arg.get_default_values(), ["true"]);
        assert!(matches!(arg.get_action(), clap::ArgAction::Set));
        assert!(
            api_container_block().contains("- name: INCIDENTS_ROW_HEARTBEAT"),
            "the chart renders INCIDENTS_ROW_HEARTBEAT (when pollers.incidents.ingest.rowHeartbeat is false)"
        );
    }

    #[test]
    fn every_sweep_interval_tunable_this_config_declares_is_set_on_the_charts_api_container() {
        const SWEEP_ENV_VARS: &[&str] = &[
            "RECONCILIATION_SWEEP_INTERVAL_SECS",
            "SCHEDULE_ENRICHMENT_GRACE_MINUTES",
            "BACKLOG_MATCH_SWEEP_INTERVAL_SECS",
            "SESSION_CLEANUP_INTERVAL_SECS",
            "PAST_TRAVEL_RETENTION_DAYS",
            "STALE_PUSH_SUBSCRIPTION_DAYS",
            "INACTIVE_ACCOUNT_RETENTION_DAYS",
        ];
        let block = api_container_block();
        let command = ServiceArguments::command();

        let declared_env_names: Vec<String> = command
            .get_arguments()
            .filter_map(|arg| arg.get_env().and_then(|env| env.to_str()))
            .map(str::to_string)
            .collect();
        for env in SWEEP_ENV_VARS {
            assert!(
                declared_env_names.iter().any(|d| d == env),
                "sanity check: crates/api/src/data/config.rs must still declare {env} as an \
                 env-backed ServiceArguments field; this test's own env var list is stale if not"
            );
            assert!(
                block.contains(&format!("- name: {env}")),
                "{env} is declared by crates/api/src/data/config.rs but never set on the `api` \
                 container in charts/distant-signal/templates/api-deployment.yaml, so under Helm \
                 an operator can never tune it without a rebuild -- it silently stays at this \
                 crate's own default forever"
            );
        }
    }
}
