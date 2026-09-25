//! Durable journey templates (Phase B, on-demand only) -- see
//! docs/superpowers/specs/2026-09-22-reusable-repeating-journeys-design.md
//! §2.2, §8 and
//! docs/superpowers/plans/2026-09-22-reusable-journeys-phaseB-durable-templates-plan.md.
//! This module owns `journey_templates`/`journey_template_legs` entirely --
//! it never reaches into `crate::data::journeys`'s private `insert_journey`/
//! `insert_leg` helpers (see this plan's own Global Constraints); it does
//! its own `journeys`/`journey_legs` writes for [`materialize_template`],
//! and reuses exactly two existing `journeys.rs` functions unchanged:
//! [`crate::data::journeys::journey_owner`] (ownership check for the
//! promote-from-journey path) and
//! [`crate::data::journeys::list_legs_for_journey`] (reads the source
//! journey's own legs back for that same path).
//!
//! **Phase C columns, present but inert**: `days_of_week`/`active`/
//! `starts_on`/`ends_on`/`default_match_mode`/`auto_commit_rule` are all
//! read and written by this module's CRUD functions (so a future Phase C
//! UI/sweep has somewhere to read/write without a second migration), but
//! [`materialize_template`] never inspects any of them -- every leg it
//! mints is unconditionally `match_mode = 'unmatched'`.

use chrono::{DateTime, NaiveDate, NaiveTime, Utc};
use serde::Serialize;
use sqlx::PgPool;

/// One `journey_templates` row, unresolved -- backs the ownership check
/// every write route folds a read through, and (via
/// [`get_owned_template`]) the detail route's own header fields.
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct JourneyTemplateRow {
    pub id: i64,
    pub user_id: String,
    pub custom_name: Option<String>,
    pub days_of_week: Option<i16>,
    pub active: bool,
    pub starts_on: Option<NaiveDate>,
    pub ends_on: Option<NaiveDate>,
    pub default_match_mode: String,
    pub auto_commit_rule: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// One `journey_template_legs` row plus resolved station names -- same
/// `LEFT JOIN stations ... ON s.crs = UPPER(...)` mechanism
/// `journeys::JourneyLegWithNamesRow` already uses, same "`None` means no
/// reference row for that code, not no leg" contract.
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct JourneyTemplateLegWithNamesRow {
    pub id: i64,
    pub template_id: i64,
    pub leg_order: i32,
    pub origin_crs: Option<String>,
    pub origin_name: Option<String>,
    pub destination_crs: Option<String>,
    pub destination_name: Option<String>,
    pub depart_after: Option<NaiveTime>,
    pub depart_before: Option<NaiveTime>,
    pub arrive_after: Option<NaiveTime>,
    pub arrive_before: Option<NaiveTime>,
}

/// One row of `GET /JourneyTemplates/mine` -- deliberately lighter than the
/// full detail response, mirroring `journeys::JourneyListItem`'s own
/// "list is lighter than detail" split. Summarizes a multi-leg template as
/// "first leg's origin -> last leg's destination," the same rollup
/// `app/journeys/[id]/page.tsx`'s `defaultJourneyTitle` computes client-side
/// for a journey with no `customName` -- computed server-side here instead
/// since a list row has no per-leg detail to compute it from client-side.
#[derive(Debug, Clone, sqlx::FromRow, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct JourneyTemplateListItem {
    pub id: i64,
    pub custom_name: Option<String>,
    pub created_at: DateTime<Utc>,
    pub leg_count: i64,
    pub first_origin_crs: Option<String>,
    pub first_origin_name: Option<String>,
    pub last_destination_crs: Option<String>,
    pub last_destination_name: Option<String>,
    pub active: bool,
    pub days_of_week: Option<i16>,
}

/// User-facing validation for a manually-entered template leg's
/// origin/destination -- same 3-letter CRS check as
/// `journeys::validate_window_leg`, deliberately WITHOUT that function's
/// "at least one window bound set" requirement. See this plan's Judgment
/// Call 2 for the full reasoning: a template leg is never itself
/// "matched," so the ambiguity that rule exists to prevent for an ordinary
/// journey leg doesn't apply here. Not called at all for a leg produced by
/// the promote-from-journey path (Task 3's route handles that leg
/// separately -- see [`create_template`]'s own doc comment).
pub fn validate_template_leg(origin_crs: &str, destination_crs: &str) -> Result<(), String> {
    if origin_crs.trim().len() != 3 {
        return Err(
            "Enter a valid origin station — CRS codes are three letters, like WOK or EUS."
                .to_string(),
        );
    }
    if destination_crs.trim().len() != 3 {
        return Err(
            "Enter a valid destination station — CRS codes are three letters, like WOK or \
             EUS."
                .to_string(),
        );
    }
    Ok(())
}

/// User-facing validation for a template's recurrence fields
/// (`default_match_mode`/`auto_commit_rule`/`days_of_week`/`starts_on`/
/// `ends_on`) -- same pure-validator pattern as [`validate_template_leg`],
/// called from the PUT route before persisting so an invalid value comes
/// back as a friendly 400 instead of a raw Postgres CHECK-constraint 500
/// from the `journey_templates` table's own `default_match_mode`/
/// `auto_commit_rule` constraints (Task 1's migration).
///
/// `auto_commit_rule = Some("earliest")` is rejected here even though the
/// DB's own CHECK constraint still permits it: per this plan's Judgment
/// Call 2, `'earliest'` is schema-legal but implementation-unreachable --
/// no code path ever acts on it, the sweep always uses nearest-to-now
/// regardless. Accepting it via the API would be a client-facing lie (set
/// it, read it back, but it's silently never honored), so only `None` and
/// `Some("nearest_to_now")` are valid going forward. The migration's CHECK
/// constraint itself deliberately stays as-is, permitting `'earliest'` at
/// the schema level as the plan's own reserved-value marker -- only this
/// API-level validator is tightened.
///
/// `days_of_week`, when `Some`, must be in `1..=127` -- the full range of
/// non-empty Mon-Sun bitmask combinations; `None` means "not recurring"
/// and is always accepted. `starts_on`/`ends_on`, when both `Some`, must
/// satisfy `starts_on <= ends_on` -- a template whose window starts after
/// it ends would silently never fire.
pub fn validate_template_recurrence(
    default_match_mode: &str,
    auto_commit_rule: Option<&str>,
    days_of_week: Option<i16>,
    starts_on: Option<NaiveDate>,
    ends_on: Option<NaiveDate>,
) -> Result<(), String> {
    if default_match_mode != "manual" && default_match_mode != "auto" {
        return Err("defaultMatchMode must be either \"manual\" or \"auto\".".to_string());
    }
    if let Some(rule) = auto_commit_rule
        && rule != "nearest_to_now"
    {
        return Err("autoCommitRule must be \"nearest_to_now\", or left unset.".to_string());
    }
    if let Some(days) = days_of_week
        && !(1..=127).contains(&days)
    {
        return Err(
            "daysOfWeek must select at least one day and no more than all seven.".to_string(),
        );
    }
    if let (Some(starts_on), Some(ends_on)) = (starts_on, ends_on)
        && starts_on > ends_on
    {
        return Err("startsOn must be on or before endsOn.".to_string());
    }
    Ok(())
}

/// A template leg's writable fields, already validated/CRS-normalized by
/// the caller (route layer for `manual` mode via [`validate_template_leg`];
/// the promote-from-journey route handler for `fromJourney` mode, which
/// copies a source journey leg's own fields verbatim with no re-validation
/// -- see Task 3's `post_journey_template`). Shared by [`create_template`]
/// and [`replace_template`].
pub struct TemplateLegInput {
    pub origin_crs: Option<String>,
    pub destination_crs: Option<String>,
    pub depart_after: Option<NaiveTime>,
    pub depart_before: Option<NaiveTime>,
    pub arrive_after: Option<NaiveTime>,
    pub arrive_before: Option<NaiveTime>,
}

async fn insert_template_leg(
    pool: &mut sqlx::PgConnection,
    template_id: i64,
    leg_order: i32,
    leg: &TemplateLegInput,
) -> anyhow::Result<i64> {
    let (id,): (i64,) = sqlx::query_as(
        "INSERT INTO journey_template_legs \
            (template_id, leg_order, origin_crs, destination_crs, \
             depart_after, depart_before, arrive_after, arrive_before) \
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8) \
         RETURNING id",
    )
    .bind(template_id)
    .bind(leg_order)
    .bind(&leg.origin_crs)
    .bind(&leg.destination_crs)
    .bind(leg.depart_after)
    .bind(leg.depart_before)
    .bind(leg.arrive_after)
    .bind(leg.arrive_before)
    .fetch_one(&mut *pool)
    .await?;
    Ok(id)
}

/// Creates a new template with `legs.len()` legs (`leg_order` 1-based,
/// assignment order), in one transaction -- all legs land or none do. The
/// ROUTE layer is responsible for having already validated every leg
/// (`validate_template_leg` for a `manual`-mode request; the
/// promote-from-journey path validates nothing here, since a source
/// journey's own legs were already validated when THAT journey was
/// created -- see Task 3). `legs` must be non-empty -- the route rejects
/// an empty array with 400 before this is ever called (see Task 3's own
/// validation step), so [`materialize_template`] can safely assume every
/// stored template has at least one leg.
pub async fn create_template(
    pool: &PgPool,
    user_id: &str,
    custom_name: Option<&str>,
    legs: &[TemplateLegInput],
) -> anyhow::Result<i64> {
    let mut tx = pool.begin().await?;
    let (template_id,): (i64,) = sqlx::query_as(
        "INSERT INTO journey_templates (user_id, custom_name) VALUES ($1, $2) RETURNING id",
    )
    .bind(user_id)
    .bind(custom_name)
    .fetch_one(&mut *tx)
    .await?;
    for (index, leg) in legs.iter().enumerate() {
        insert_template_leg(&mut tx, template_id, (index + 1) as i32, leg).await?;
    }
    tx.commit().await?;
    Ok(template_id)
}

/// Ownership-scoped, folds `user_id` directly into the `WHERE` — same
/// convention as every other ownership check in this codebase.
/// `Ok(None)` for "no such template, or not this caller's" (route maps to
/// 404, never 403).
pub async fn get_owned_template(
    pool: &PgPool,
    template_id: i64,
    user_id: &str,
) -> anyhow::Result<Option<JourneyTemplateRow>> {
    let row = sqlx::query_as::<_, JourneyTemplateRow>(
        "SELECT id, user_id, custom_name, days_of_week, active, starts_on, ends_on, \
                default_match_mode, auto_commit_rule, created_at, updated_at \
         FROM journey_templates WHERE id = $1 AND user_id = $2",
    )
    .bind(template_id)
    .bind(user_id)
    .fetch_optional(pool)
    .await?;
    Ok(row)
}

/// Every leg of a template, `leg_order` ascending, with resolved station
/// names. Deliberately NOT ownership-scoped on its own -- same reasoning
/// as `journeys::list_legs_for_journey`'s own doc comment: every real
/// caller (the detail route, `materialize_template` below) already
/// confirmed ownership one call earlier via [`get_owned_template`].
pub async fn list_template_legs(
    pool: &PgPool,
    template_id: i64,
) -> anyhow::Result<Vec<JourneyTemplateLegWithNamesRow>> {
    let rows = sqlx::query_as::<_, JourneyTemplateLegWithNamesRow>(
        "SELECT jtl.id, jtl.template_id, jtl.leg_order, jtl.origin_crs, so.name AS origin_name, \
                jtl.destination_crs, sd.name AS destination_name, \
                jtl.depart_after, jtl.depart_before, jtl.arrive_after, jtl.arrive_before \
         FROM journey_template_legs jtl \
         LEFT JOIN stations so ON so.crs = UPPER(jtl.origin_crs) \
         LEFT JOIN stations sd ON sd.crs = UPPER(jtl.destination_crs) \
         WHERE jtl.template_id = $1 ORDER BY jtl.leg_order",
    )
    .bind(template_id)
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

/// Most-recently-created template first, capped at
/// `train_tracking::MINE_LIST_LIMIT` -- same cap `journeys::JourneyListItem`
/// already shares. Each row is summarized by its first leg's origin and
/// last leg's destination (`MIN`/`MAX` on `leg_order`, joined back to the
/// leg rows that own those extremes) -- a template with zero legs (should
/// never happen given `create_template`'s own non-empty-legs invariant,
/// but defensively possible if a row is ever hand-edited) renders with
/// `leg_count = 0` and every origin/destination field `NULL`, not an error.
pub async fn list_templates_for_user(
    pool: &PgPool,
    user_id: &str,
) -> anyhow::Result<Vec<JourneyTemplateListItem>> {
    let rows = sqlx::query_as::<_, JourneyTemplateListItem>(
        "SELECT jt.id, jt.custom_name, jt.created_at, jt.active, jt.days_of_week, \
                COALESCE(leg_counts.leg_count, 0) AS leg_count, \
                first_leg.origin_crs AS first_origin_crs, so.name AS first_origin_name, \
                last_leg.destination_crs AS last_destination_crs, sd.name AS last_destination_name \
         FROM journey_templates jt \
         LEFT JOIN ( \
             SELECT template_id, COUNT(*) AS leg_count, \
                    MIN(leg_order) AS min_order, MAX(leg_order) AS max_order \
             FROM journey_template_legs GROUP BY template_id \
         ) leg_counts ON leg_counts.template_id = jt.id \
         LEFT JOIN journey_template_legs first_leg \
             ON first_leg.template_id = jt.id AND first_leg.leg_order = leg_counts.min_order \
         LEFT JOIN journey_template_legs last_leg \
             ON last_leg.template_id = jt.id AND last_leg.leg_order = leg_counts.max_order \
         LEFT JOIN stations so ON so.crs = UPPER(first_leg.origin_crs) \
         LEFT JOIN stations sd ON sd.crs = UPPER(last_leg.destination_crs) \
         WHERE jt.user_id = $1 \
         ORDER BY jt.created_at DESC \
         LIMIT $2",
    )
    .bind(user_id)
    .bind(crate::data::train_tracking::MINE_LIST_LIMIT)
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

/// Full-resource replace: updates `custom_name`, the six recurrence
/// fields, and wholesale replaces every leg, ownership-scoped, one
/// transaction. `Ok(false)` for "no such template, or not this caller's"
/// (route maps to 404) -- checked via the `UPDATE ... WHERE id = $1 AND
/// user_id = $2` itself, same fold-ownership-into-the-write convention as
/// `journeys::set_leg_train_subscription`. `legs` must be non-empty --
/// same route-level guard as [`create_template`]'s own contract.
/// `default_match_mode`/`auto_commit_rule` must already have passed
/// [`validate_template_recurrence`] -- this function trusts its caller and
/// otherwise relies on the table's own CHECK constraints (Task 1's
/// migration) as a last-resort guard.
#[allow(clippy::too_many_arguments)]
pub async fn replace_template(
    pool: &PgPool,
    template_id: i64,
    user_id: &str,
    custom_name: Option<&str>,
    legs: &[TemplateLegInput],
    days_of_week: Option<i16>,
    active: bool,
    starts_on: Option<NaiveDate>,
    ends_on: Option<NaiveDate>,
    default_match_mode: &str,
    auto_commit_rule: Option<&str>,
) -> anyhow::Result<bool> {
    let mut tx = pool.begin().await?;
    let result = sqlx::query(
        "UPDATE journey_templates SET custom_name = $1, days_of_week = $2, active = $3, \
                starts_on = $4, ends_on = $5, default_match_mode = $6, auto_commit_rule = $7, \
                updated_at = NOW() \
         WHERE id = $8 AND user_id = $9",
    )
    .bind(custom_name)
    .bind(days_of_week)
    .bind(active)
    .bind(starts_on)
    .bind(ends_on)
    .bind(default_match_mode)
    .bind(auto_commit_rule)
    .bind(template_id)
    .bind(user_id)
    .execute(&mut *tx)
    .await?;
    if result.rows_affected() == 0 {
        // Not owned/doesn't exist -- roll back (no leg mutation happened
        // yet) and report "not found" to the route.
        tx.rollback().await?;
        return Ok(false);
    }

    sqlx::query("DELETE FROM journey_template_legs WHERE template_id = $1")
        .bind(template_id)
        .execute(&mut *tx)
        .await?;
    for (index, leg) in legs.iter().enumerate() {
        insert_template_leg(&mut tx, template_id, (index + 1) as i32, leg).await?;
    }
    tx.commit().await?;
    Ok(true)
}

/// Deletes a template the caller owns. Cascades into
/// `journey_template_legs` (`ON DELETE CASCADE`, Task 1); every
/// `journeys` row this template ever produced survives untouched, losing
/// only its `source_template_id` (`ON DELETE SET NULL`, same table). `true`
/// if a row was deleted, `false` for "no such template, or not this
/// caller's" (404, never 403).
pub async fn delete_template(
    pool: &PgPool,
    template_id: i64,
    user_id: &str,
) -> anyhow::Result<bool> {
    let result = sqlx::query("DELETE FROM journey_templates WHERE id = $1 AND user_id = $2")
        .bind(template_id)
        .bind(user_id)
        .execute(pool)
        .await?;
    Ok(result.rows_affected() > 0)
}

/// The result of one successful materialization -- the new `journeys.id`
/// plus every `journey_legs.id` it produced, `leg_order` ascending (same
/// order the template's own legs were read in).
pub struct MaterializedJourney {
    pub journey_id: i64,
    pub leg_ids: Vec<i64>,
}

/// Stamps a new `journeys` row (plus one `journey_legs` row per template
/// leg) from `template_id`'s current shape, dated `service_date`. This is
/// the design doc's §3.2 per-template materialization logic, called
/// synchronously by Task 3's `POST /JourneyTemplates/{id}/materialize`
/// route with no scheduler involved -- Phase C's automated sweep (not this
/// plan) will call this SAME function once it exists, passing the due
/// template's own `user_id` instead of an authenticated caller's (see this
/// plan's closing section).
///
/// Every leg is minted `match_mode = 'unmatched'`, `train_subscription_id
/// = NULL`, `service_date = service_date` (the caller's target date, not
/// "today" -- there is no implicit "today" default anywhere in this
/// function, matching this codebase's established "every leg-creation
/// wire type requires an explicit service_date" convention). The
/// template's own `default_match_mode`/`auto_commit_rule` are READ (via
/// [`get_owned_template`]) but never inspected for this decision -- Phase
/// B's materialization is unconditionally manual-pick, regardless of what
/// those columns say (see this plan's Non-goals).
///
/// Deliberately carries NO idempotency guard against calling this twice
/// for the same `(template_id, service_date)` -- see this plan's Judgment
/// Call 7: that guard belongs to Phase C's sweep query, not to this
/// function, since a Phase-B human deliberately re-clicking "Run now" for
/// the same date (e.g. to top up a second occurrence) is a legitimate,
/// supported case here.
///
/// `Ok(None)` for "no such template, or not this caller's" (route maps to
/// 404). All-or-nothing inside one transaction: a multi-leg template
/// either fully materializes or the whole attempt is rolled back -- an
/// incomplete journey missing some of its legs would violate every other
/// reader's assumption that a journey's legs are exactly what its owner
/// asked for.
pub async fn materialize_template(
    pool: &PgPool,
    template_id: i64,
    user_id: &str,
    service_date: NaiveDate,
) -> anyhow::Result<Option<MaterializedJourney>> {
    let Some(template) = get_owned_template(pool, template_id, user_id).await? else {
        return Ok(None);
    };
    let legs = list_template_legs(pool, template_id).await?;
    // Invariant from create_template/replace_template: a stored template
    // always has >=1 leg. Defensive rather than an unwrap/panic if that's
    // ever violated by a hand-edited row -- report "nothing to
    // materialize" the same way "template not found" reads to the route,
    // rather than minting a zero-leg journeys row nothing else in this
    // codebase expects to see (journeys::delete_leg's own doc comment).
    if legs.is_empty() {
        return Ok(None);
    }

    let mut tx = pool.begin().await?;
    let (journey_id,): (i64,) = sqlx::query_as(
        "INSERT INTO journeys (user_id, custom_name, source_template_id) \
         VALUES ($1, $2, $3) RETURNING id",
    )
    .bind(user_id)
    .bind(&template.custom_name)
    .bind(template_id)
    .fetch_one(&mut *tx)
    .await?;

    let mut leg_ids = Vec::with_capacity(legs.len());
    for leg in &legs {
        let (leg_id,): (i64,) = sqlx::query_as(
            "INSERT INTO journey_legs \
                (journey_id, leg_order, origin_crs, destination_crs, service_date, \
                 train_subscription_id, match_mode, \
                 depart_after, depart_before, arrive_after, arrive_before) \
             VALUES ($1, $2, $3, $4, $5, NULL, 'unmatched', $6, $7, $8, $9) \
             RETURNING id",
        )
        .bind(journey_id)
        .bind(leg.leg_order)
        .bind(&leg.origin_crs)
        .bind(&leg.destination_crs)
        .bind(service_date)
        .bind(leg.depart_after)
        .bind(leg.depart_before)
        .bind(leg.arrive_after)
        .bind(leg.arrive_before)
        .fetch_one(&mut *tx)
        .await?;
        leg_ids.push(leg_id);
    }

    // An explicit "Run now" for a date whose occurrence was previously
    // DISCARDED (`journey_template_skipped_dates`, written by
    // `journeys::delete_journey`/`delete_leg` -- see
    // `journeys::record_template_occurrence_skips` for the re-mint bug that
    // tombstone closes) is the user unambiguously asking for that date back,
    // so it clears the tombstone: the recurrence sweep may mint for this
    // date again if this occurrence is later deleted... which would, itself,
    // write a fresh tombstone. Without this, a discarded date would stay
    // permanently un-sweepable even after the user changed their mind.
    sqlx::query(
        "DELETE FROM journey_template_skipped_dates WHERE template_id = $1 AND service_date = $2",
    )
    .bind(template_id)
    .bind(service_date)
    .execute(&mut *tx)
    .await?;

    tx.commit().await?;

    Ok(Some(MaterializedJourney {
        journey_id,
        leg_ids,
    }))
}

#[cfg(test)]
mod validate_template_leg_tests {
    use super::*;

    #[test]
    fn a_well_formed_leg_is_accepted() {
        assert!(validate_template_leg("WAT", "RDG").is_ok());
    }

    #[test]
    fn a_short_origin_code_is_rejected() {
        assert!(validate_template_leg("W", "RDG").is_err());
    }

    #[test]
    fn a_short_destination_code_is_rejected() {
        assert!(validate_template_leg("WAT", "R").is_err());
    }

    #[test]
    fn no_window_bound_is_required_unlike_validate_window_leg() {
        // Judgment Call 2 -- deliberately no window-bound check at all
        // here; this test exists to keep that decision from silently
        // regressing if someone copies validate_window_leg's body in
        // later.
        assert!(validate_template_leg("WAT", "RDG").is_ok());
    }

    #[test]
    fn validation_messages_carry_no_internal_field_names() {
        let message = validate_template_leg("W", "RDG").unwrap_err();
        assert!(!message.is_empty());
        assert!(
            !message.contains('_'),
            "user-facing copy leaked an identifier: {message}"
        );
    }
}

#[cfg(test)]
mod validate_template_recurrence_tests {
    use super::*;

    #[test]
    fn manual_mode_with_no_auto_commit_rule_is_accepted() {
        assert!(validate_template_recurrence("manual", None, None, None, None).is_ok());
    }

    #[test]
    fn auto_mode_with_nearest_to_now_is_accepted() {
        assert!(
            validate_template_recurrence("auto", Some("nearest_to_now"), None, None, None).is_ok()
        );
    }

    #[test]
    fn auto_mode_with_earliest_is_now_rejected() {
        // Final-review Finding 3.1 -- 'earliest' is schema-legal (the DB's
        // own CHECK constraint still allows it, deliberately, as a
        // reserved marker) but implementation-unreachable: no code path
        // ever acts on it, so the API-level validator must reject it.
        assert!(validate_template_recurrence("auto", Some("earliest"), None, None, None).is_err());
    }

    #[test]
    fn an_unrecognized_default_match_mode_is_rejected() {
        assert!(validate_template_recurrence("sometimes", None, None, None, None).is_err());
    }

    #[test]
    fn an_unrecognized_auto_commit_rule_is_rejected() {
        assert!(validate_template_recurrence("auto", Some("whenever"), None, None, None).is_err());
    }

    #[test]
    fn days_of_week_out_of_range_is_rejected() {
        for days in [-1_i16, 0, 128] {
            assert!(
                validate_template_recurrence("manual", None, Some(days), None, None).is_err(),
                "daysOfWeek = {days} must be rejected"
            );
        }
    }

    #[test]
    fn days_of_week_in_range_is_accepted() {
        for days in [1_i16, 64, 127] {
            assert!(
                validate_template_recurrence("manual", None, Some(days), None, None).is_ok(),
                "daysOfWeek = {days} must be accepted"
            );
        }
    }

    #[test]
    fn days_of_week_none_is_accepted() {
        assert!(validate_template_recurrence("manual", None, None, None, None).is_ok());
    }

    #[test]
    fn starts_on_after_ends_on_is_rejected() {
        let starts_on: NaiveDate = "2026-12-31".parse().unwrap();
        let ends_on: NaiveDate = "2026-10-01".parse().unwrap();
        assert!(
            validate_template_recurrence("manual", None, None, Some(starts_on), Some(ends_on))
                .is_err()
        );
    }

    #[test]
    fn starts_on_on_or_before_ends_on_is_accepted() {
        let starts_on: NaiveDate = "2026-10-01".parse().unwrap();
        let ends_on: NaiveDate = "2026-12-31".parse().unwrap();
        assert!(
            validate_template_recurrence("manual", None, None, Some(starts_on), Some(ends_on))
                .is_ok()
        );
        assert!(
            validate_template_recurrence("manual", None, None, Some(starts_on), Some(starts_on))
                .is_ok(),
            "equal starts_on/ends_on must be accepted"
        );
    }

    #[test]
    fn either_bound_missing_is_accepted() {
        let some_date: NaiveDate = "2026-10-01".parse().unwrap();
        assert!(validate_template_recurrence("manual", None, None, Some(some_date), None).is_ok());
        assert!(validate_template_recurrence("manual", None, None, None, Some(some_date)).is_ok());
    }
}

#[cfg(test)]
mod db_tests {
    use super::*;
    use sqlx::postgres::PgPoolOptions;

    async fn connect() -> PgPool {
        let database_url =
            std::env::var("DATABASE_URL").expect("DATABASE_URL must be set to run this test");
        PgPoolOptions::new()
            .connect(&database_url)
            .await
            .expect("connect to postgres")
    }

    async fn seed_user(pool: &PgPool, user_id: &str) {
        sqlx::query(
            "INSERT INTO users (id, email, name) VALUES ($1, $2, $3) ON CONFLICT (id) DO NOTHING",
        )
        .bind(user_id)
        .bind(format!("{user_id}@example.com"))
        .bind(user_id)
        .execute(pool)
        .await
        .expect("seed fixture user");
    }

    /// Same FK-respecting order as `journeys::db_tests::cleanup_user`, plus
    /// this module's own `journey_templates`/`journey_template_legs` rows --
    /// each module needing fixture cleanup keeps its own copy (that file's
    /// own precedent).
    async fn cleanup_user(pool: &PgPool, user_id: &str) {
        sqlx::query(
            "DELETE FROM journey_template_legs WHERE template_id IN \
                (SELECT id FROM journey_templates WHERE user_id = $1)",
        )
        .bind(user_id)
        .execute(pool)
        .await
        .expect("cleanup fixture journey_template_legs");
        sqlx::query("DELETE FROM journey_templates WHERE user_id = $1")
            .bind(user_id)
            .execute(pool)
            .await
            .expect("cleanup fixture journey_templates");
        sqlx::query(
            "DELETE FROM journey_legs WHERE journey_id IN (SELECT id FROM journeys WHERE user_id = $1)",
        )
        .bind(user_id)
        .execute(pool)
        .await
        .expect("cleanup fixture journey_legs");
        sqlx::query("DELETE FROM journeys WHERE user_id = $1")
            .bind(user_id)
            .execute(pool)
            .await
            .expect("cleanup fixture journeys");
        sqlx::query("DELETE FROM train_subscriptions WHERE user_id = $1")
            .bind(user_id)
            .execute(pool)
            .await
            .expect("cleanup fixture tracked_trains");
        sqlx::query("DELETE FROM users WHERE id = $1")
            .bind(user_id)
            .execute(pool)
            .await
            .expect("cleanup fixture user");
    }

    fn fixture_leg(origin_crs: &str, destination_crs: &str) -> TemplateLegInput {
        TemplateLegInput {
            origin_crs: Some(origin_crs.to_string()),
            destination_crs: Some(destination_crs.to_string()),
            depart_after: Some("08:00:00".parse().unwrap()),
            depart_before: None,
            arrive_after: None,
            arrive_before: None,
        }
    }

    #[tokio::test]
    #[ignore = "requires a live database; see this plan's Global Constraints for the \
                DATABASE_URL incantation, then run with `cargo test -p api \
                create_template_creates_a_template_with_ordered_legs -- --ignored --test-threads=1`"]
    async fn create_template_creates_a_template_with_ordered_legs() {
        let pool = connect().await;
        let user_id = "TEST-TEMPLATE-CREATE";
        seed_user(&pool, user_id).await;

        let legs = vec![fixture_leg("WAT", "RDG"), fixture_leg("RDG", "BRI")];
        let template_id = create_template(&pool, user_id, Some("Commute"), &legs)
            .await
            .expect("create template");

        let stored_legs = list_template_legs(&pool, template_id)
            .await
            .expect("list template legs");
        assert_eq!(stored_legs.len(), 2);
        assert_eq!(stored_legs[0].leg_order, 1);
        assert_eq!(stored_legs[0].origin_crs.as_deref(), Some("WAT"));
        assert_eq!(stored_legs[0].destination_crs.as_deref(), Some("RDG"));
        assert_eq!(stored_legs[1].leg_order, 2);
        assert_eq!(stored_legs[1].origin_crs.as_deref(), Some("RDG"));
        assert_eq!(stored_legs[1].destination_crs.as_deref(), Some("BRI"));

        cleanup_user(&pool, user_id).await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; see this plan's Global Constraints for the \
                DATABASE_URL incantation, then run with `cargo test -p api \
                get_owned_template_a_non_owner_gets_none -- --ignored --test-threads=1`"]
    async fn get_owned_template_a_non_owner_gets_none() {
        let pool = connect().await;
        let owner_id = "TEST-TEMPLATE-OWNER";
        let other_id = "TEST-TEMPLATE-OTHER";
        seed_user(&pool, owner_id).await;
        seed_user(&pool, other_id).await;

        let legs = vec![fixture_leg("WAT", "RDG")];
        let template_id = create_template(&pool, owner_id, None, &legs)
            .await
            .expect("create template");

        let as_other = get_owned_template(&pool, template_id, other_id)
            .await
            .expect("query as non-owner");
        assert!(as_other.is_none());

        let as_owner = get_owned_template(&pool, template_id, owner_id)
            .await
            .expect("query as owner")
            .expect("template exists for owner");
        assert_eq!(as_owner.id, template_id);

        cleanup_user(&pool, owner_id).await;
        cleanup_user(&pool, other_id).await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; see this plan's Global Constraints for the \
                DATABASE_URL incantation, then run with `cargo test -p api \
                replace_template_swaps_the_whole_leg_list -- --ignored --test-threads=1`"]
    async fn replace_template_swaps_the_whole_leg_list() {
        let pool = connect().await;
        let user_id = "TEST-TEMPLATE-REPLACE";
        seed_user(&pool, user_id).await;

        let legs = vec![fixture_leg("WAT", "RDG"), fixture_leg("RDG", "BRI")];
        let template_id = create_template(&pool, user_id, None, &legs)
            .await
            .expect("create template");

        let new_legs = vec![fixture_leg("EUS", "MAN")];
        let replaced = replace_template(
            &pool,
            template_id,
            user_id,
            Some("Renamed"),
            &new_legs,
            None,
            true,
            None,
            None,
            "manual",
            None,
        )
        .await
        .expect("replace template");
        assert!(replaced);

        let stored_legs = list_template_legs(&pool, template_id)
            .await
            .expect("list template legs");
        assert_eq!(stored_legs.len(), 1);
        assert_eq!(stored_legs[0].leg_order, 1);
        assert_eq!(stored_legs[0].origin_crs.as_deref(), Some("EUS"));
        assert_eq!(stored_legs[0].destination_crs.as_deref(), Some("MAN"));

        cleanup_user(&pool, user_id).await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; see this plan's Global Constraints for the \
                DATABASE_URL incantation, then run with `cargo test -p api \
                replace_template_a_non_owner_cannot_replace_it_and_it_survives -- --ignored --test-threads=1`"]
    async fn replace_template_a_non_owner_cannot_replace_it_and_it_survives() {
        let pool = connect().await;
        let owner_id = "TEST-TEMPLATE-REPLACE-OWNER";
        let other_id = "TEST-TEMPLATE-REPLACE-OTHER";
        seed_user(&pool, owner_id).await;
        seed_user(&pool, other_id).await;

        let legs = vec![fixture_leg("WAT", "RDG")];
        let template_id = create_template(&pool, owner_id, Some("Original"), &legs)
            .await
            .expect("create template");

        let new_legs = vec![fixture_leg("EUS", "MAN")];
        let replaced = replace_template(
            &pool,
            template_id,
            other_id,
            Some("Hijacked"),
            &new_legs,
            None,
            true,
            None,
            None,
            "manual",
            None,
        )
        .await
        .expect("attempt replace as non-owner");
        assert!(!replaced);

        let template = get_owned_template(&pool, template_id, owner_id)
            .await
            .expect("read template")
            .expect("template still exists");
        assert_eq!(template.custom_name.as_deref(), Some("Original"));

        let stored_legs = list_template_legs(&pool, template_id)
            .await
            .expect("list template legs");
        assert_eq!(stored_legs.len(), 1);
        assert_eq!(stored_legs[0].origin_crs.as_deref(), Some("WAT"));

        cleanup_user(&pool, owner_id).await;
        cleanup_user(&pool, other_id).await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; see this plan's Global Constraints for the \
                DATABASE_URL incantation, then run with `cargo test -p api \
                delete_template_the_owner_can_delete_it_and_produced_journeys_survive_orphaned -- --ignored --test-threads=1`"]
    async fn delete_template_the_owner_can_delete_it_and_produced_journeys_survive_orphaned() {
        let pool = connect().await;
        let user_id = "TEST-TEMPLATE-DELETE";
        seed_user(&pool, user_id).await;

        let legs = vec![fixture_leg("WAT", "RDG")];
        let template_id = create_template(&pool, user_id, None, &legs)
            .await
            .expect("create template");

        let materialized =
            materialize_template(&pool, template_id, user_id, "2026-09-22".parse().unwrap())
                .await
                .expect("materialize template")
                .expect("template exists to materialize");

        let deleted = delete_template(&pool, template_id, user_id)
            .await
            .expect("delete template");
        assert!(deleted);

        let gone = get_owned_template(&pool, template_id, user_id)
            .await
            .expect("query deleted template");
        assert!(gone.is_none());

        let source_template_id: Option<i64> =
            sqlx::query_scalar("SELECT source_template_id FROM journeys WHERE id = $1")
                .bind(materialized.journey_id)
                .fetch_one(&pool)
                .await
                .expect("read materialized journey's source_template_id");
        assert_eq!(source_template_id, None);

        // Materialized journey survives orphaned -- clean it up directly
        // since cleanup_user's own journey_templates delete already ran.
        sqlx::query("DELETE FROM journey_legs WHERE journey_id = $1")
            .bind(materialized.journey_id)
            .execute(&pool)
            .await
            .expect("cleanup materialized journey_legs");
        sqlx::query("DELETE FROM journeys WHERE id = $1")
            .bind(materialized.journey_id)
            .execute(&pool)
            .await
            .expect("cleanup materialized journey");

        cleanup_user(&pool, user_id).await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; see this plan's Global Constraints for the \
                DATABASE_URL incantation, then run with `cargo test -p api \
                materialize_template_mints_a_journey_with_unmatched_legs_dated_the_target_date -- --ignored --test-threads=1`"]
    async fn materialize_template_mints_a_journey_with_unmatched_legs_dated_the_target_date() {
        let pool = connect().await;
        let user_id = "TEST-TEMPLATE-MATERIALIZE";
        seed_user(&pool, user_id).await;

        let legs = vec![fixture_leg("WAT", "RDG"), fixture_leg("RDG", "BRI")];
        let template_id = create_template(&pool, user_id, None, &legs)
            .await
            .expect("create template");

        // Load-bearing for this plan's Non-goals: even with
        // default_match_mode = 'auto', materialize_template must still
        // mint every leg 'unmatched' -- Phase B ignores this column
        // entirely.
        sqlx::query("UPDATE journey_templates SET default_match_mode = 'auto' WHERE id = $1")
            .bind(template_id)
            .execute(&pool)
            .await
            .expect("seed default_match_mode = auto");

        let target_date: NaiveDate = "2026-10-01".parse().unwrap();
        let materialized = materialize_template(&pool, template_id, user_id, target_date)
            .await
            .expect("materialize template")
            .expect("template exists to materialize");
        assert_eq!(materialized.leg_ids.len(), 2);

        #[derive(sqlx::FromRow)]
        struct LegRow {
            leg_order: i32,
            match_mode: String,
            train_subscription_id: Option<i64>,
            service_date: NaiveDate,
        }
        let mut rows: Vec<LegRow> = sqlx::query_as(
            "SELECT leg_order, match_mode, train_subscription_id, service_date \
             FROM journey_legs WHERE journey_id = $1 ORDER BY leg_order",
        )
        .bind(materialized.journey_id)
        .fetch_all(&pool)
        .await
        .expect("read materialized legs");
        rows.sort_by_key(|row| row.leg_order);

        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].leg_order, 1);
        assert_eq!(rows[1].leg_order, 2);
        for row in &rows {
            assert_eq!(row.match_mode, "unmatched");
            assert_eq!(row.train_subscription_id, None);
            assert_eq!(row.service_date, target_date);
        }

        sqlx::query("DELETE FROM journey_legs WHERE journey_id = $1")
            .bind(materialized.journey_id)
            .execute(&pool)
            .await
            .expect("cleanup materialized journey_legs");
        sqlx::query("DELETE FROM journeys WHERE id = $1")
            .bind(materialized.journey_id)
            .execute(&pool)
            .await
            .expect("cleanup materialized journey");
        cleanup_user(&pool, user_id).await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; see this plan's Global Constraints for the \
                DATABASE_URL incantation, then run with `cargo test -p api \
                materialize_template_with_no_window_bounds_produces_a_fully_open_leg \
                -- --ignored --test-threads=1`"]
    async fn materialize_template_with_no_window_bounds_produces_a_fully_open_leg() {
        // Regression test for this plan's Judgment Call 2
        // (`validate_template_leg`'s own doc comment, and this module's
        // sibling `no_window_bound_is_required_unlike_validate_window_leg`
        // test): a template leg is allowed to carry NO time window at
        // all, and `materialize_template` must copy that verbatim onto
        // the minted `journey_legs` row -- all four of
        // `depart_after`/`depart_before`/`arrive_after`/`arrive_before`
        // stay `NULL`, not defaulted to anything.
        let pool = connect().await;
        let user_id = "TEST-TEMPLATE-MATERIALIZE-OPEN-WINDOW";
        seed_user(&pool, user_id).await;

        let legs = vec![TemplateLegInput {
            origin_crs: Some("WAT".to_string()),
            destination_crs: Some("RDG".to_string()),
            depart_after: None,
            depart_before: None,
            arrive_after: None,
            arrive_before: None,
        }];
        let template_id = create_template(&pool, user_id, None, &legs)
            .await
            .expect("create template");

        let target_date: NaiveDate = "2026-10-01".parse().unwrap();
        let materialized = materialize_template(&pool, template_id, user_id, target_date)
            .await
            .expect("materialize template")
            .expect("template exists to materialize");
        assert_eq!(materialized.leg_ids.len(), 1);

        #[derive(sqlx::FromRow)]
        struct LegWindowRow {
            depart_after: Option<NaiveTime>,
            depart_before: Option<NaiveTime>,
            arrive_after: Option<NaiveTime>,
            arrive_before: Option<NaiveTime>,
            match_mode: String,
        }
        let row: LegWindowRow = sqlx::query_as(
            "SELECT depart_after, depart_before, arrive_after, arrive_before, match_mode \
             FROM journey_legs WHERE id = $1",
        )
        .bind(materialized.leg_ids[0])
        .fetch_one(&pool)
        .await
        .expect("read materialized leg");
        assert_eq!(row.depart_after, None);
        assert_eq!(row.depart_before, None);
        assert_eq!(row.arrive_after, None);
        assert_eq!(row.arrive_before, None);
        assert_eq!(row.match_mode, "unmatched");

        sqlx::query("DELETE FROM journey_legs WHERE journey_id = $1")
            .bind(materialized.journey_id)
            .execute(&pool)
            .await
            .expect("cleanup materialized journey_legs");
        sqlx::query("DELETE FROM journeys WHERE id = $1")
            .bind(materialized.journey_id)
            .execute(&pool)
            .await
            .expect("cleanup materialized journey");
        cleanup_user(&pool, user_id).await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; see this plan's Global Constraints for the \
                DATABASE_URL incantation, then run with `cargo test -p api \
                materialize_template_a_non_owner_gets_none -- --ignored --test-threads=1`"]
    async fn materialize_template_a_non_owner_gets_none() {
        let pool = connect().await;
        let owner_id = "TEST-TEMPLATE-MAT-OWNER";
        let other_id = "TEST-TEMPLATE-MAT-OTHER";
        seed_user(&pool, owner_id).await;
        seed_user(&pool, other_id).await;

        let legs = vec![fixture_leg("WAT", "RDG")];
        let template_id = create_template(&pool, owner_id, None, &legs)
            .await
            .expect("create template");

        let result =
            materialize_template(&pool, template_id, other_id, "2026-09-22".parse().unwrap())
                .await
                .expect("attempt materialize as non-owner");
        assert!(result.is_none());

        cleanup_user(&pool, owner_id).await;
        cleanup_user(&pool, other_id).await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; see this plan's Global Constraints for the \
                DATABASE_URL incantation, then run with `cargo test -p api \
                materialize_template_can_be_called_twice_for_the_same_date_and_mints_two_journeys -- --ignored --test-threads=1`"]
    async fn materialize_template_can_be_called_twice_for_the_same_date_and_mints_two_journeys() {
        let pool = connect().await;
        let user_id = "TEST-TEMPLATE-MAT-TWICE";
        seed_user(&pool, user_id).await;

        let legs = vec![fixture_leg("WAT", "RDG")];
        let template_id = create_template(&pool, user_id, None, &legs)
            .await
            .expect("create template");

        let service_date: NaiveDate = "2026-09-22".parse().unwrap();
        let first = materialize_template(&pool, template_id, user_id, service_date)
            .await
            .expect("materialize first time")
            .expect("template exists");
        let second = materialize_template(&pool, template_id, user_id, service_date)
            .await
            .expect("materialize second time")
            .expect("template exists");

        assert_ne!(first.journey_id, second.journey_id);

        for journey_id in [first.journey_id, second.journey_id] {
            let source_template_id: Option<i64> =
                sqlx::query_scalar("SELECT source_template_id FROM journeys WHERE id = $1")
                    .bind(journey_id)
                    .fetch_one(&pool)
                    .await
                    .expect("read source_template_id");
            assert_eq!(source_template_id, Some(template_id));

            sqlx::query("DELETE FROM journey_legs WHERE journey_id = $1")
                .bind(journey_id)
                .execute(&pool)
                .await
                .expect("cleanup materialized journey_legs");
            sqlx::query("DELETE FROM journeys WHERE id = $1")
                .bind(journey_id)
                .execute(&pool)
                .await
                .expect("cleanup materialized journey");
        }

        cleanup_user(&pool, user_id).await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; see this plan's Global Constraints for the \
                DATABASE_URL incantation, then run with `cargo test -p api \
                materialize_template_copies_a_nullable_window_verbatim -- --ignored --test-threads=1`"]
    async fn materialize_template_copies_a_nullable_window_verbatim() {
        let pool = connect().await;
        let user_id = "TEST-TEMPLATE-MAT-NULL-WINDOW";
        seed_user(&pool, user_id).await;

        let legs = vec![TemplateLegInput {
            origin_crs: Some("WAT".to_string()),
            destination_crs: Some("RDG".to_string()),
            depart_after: None,
            depart_before: None,
            arrive_after: None,
            arrive_before: None,
        }];
        let template_id = create_template(&pool, user_id, None, &legs)
            .await
            .expect("create template");

        let materialized =
            materialize_template(&pool, template_id, user_id, "2026-09-22".parse().unwrap())
                .await
                .expect("materialize template")
                .expect("template exists");

        #[derive(sqlx::FromRow)]
        struct LegRow {
            origin_crs: Option<String>,
            destination_crs: Option<String>,
            depart_after: Option<NaiveTime>,
            depart_before: Option<NaiveTime>,
            arrive_after: Option<NaiveTime>,
            arrive_before: Option<NaiveTime>,
        }
        let row: LegRow = sqlx::query_as(
            "SELECT origin_crs, destination_crs, depart_after, depart_before, \
                    arrive_after, arrive_before \
             FROM journey_legs WHERE journey_id = $1",
        )
        .bind(materialized.journey_id)
        .fetch_one(&pool)
        .await
        .expect("read materialized leg");

        assert_eq!(row.origin_crs.as_deref(), Some("WAT"));
        assert_eq!(row.destination_crs.as_deref(), Some("RDG"));
        assert_eq!(row.depart_after, None);
        assert_eq!(row.depart_before, None);
        assert_eq!(row.arrive_after, None);
        assert_eq!(row.arrive_before, None);

        sqlx::query("DELETE FROM journey_legs WHERE journey_id = $1")
            .bind(materialized.journey_id)
            .execute(&pool)
            .await
            .expect("cleanup materialized journey_legs");
        sqlx::query("DELETE FROM journeys WHERE id = $1")
            .bind(materialized.journey_id)
            .execute(&pool)
            .await
            .expect("cleanup materialized journey");
        cleanup_user(&pool, user_id).await;
    }
}
