//! Journey follows (DS-146): someone a journey is shared with, through a
//! group (`group_journeys`) or a public share link (`unlisted_links`),
//! "follows" it, so it shows among their own current and upcoming journeys
//! (`GET /Journeys/mine?include=followed`), read-only, labelled with who
//! shared it.
//!
//! A follow is a bookmark onto a share, never a grant of its own: it is
//! honoured only while the share it came through still lets the follower
//! in ([`VALID_FOLLOW`], checked on every read). The rows are also deleted
//! when that stops being true (see the `journey_follows` migration,
//! `20261024100000`, for every path), so the check at read time only
//! covers the moment before a cleanup runs and an expired link.
//!
//! A follow conveys exactly what the share already shows: the journey
//! detail a group member or a share-link visitor sees (`isOwner: false`),
//! never the owner's tickets, revisions, other journeys or share link. It
//! never gates a write: every write route stays owner-only.
//!
//! Followers get no pushes for now: the notifier has no grant on this
//! table. An opt-in would add a per-follow flag and that grant.

use std::collections::{HashMap, HashSet};

use anyhow::Result;
use chrono::{DateTime, Utc};
use serde::Serialize;
use sqlx::PgPool;

use crate::data::{unlisted_links, users};

/// The `unlisted_links.resource_type` of a journey share link
/// (`routes::journeys::JOURNEY_RESOURCE_TYPE`).
const JOURNEY_RESOURCE_TYPE: &str = "journey";

/// Whether the follow row `f` is still backed by its share: the follower is
/// still a member of the group (the share itself is guaranteed by the
/// composite foreign key onto `group_journeys`), or the link is neither
/// revoked nor expired. A SQL predicate over a `journey_follows f` row.
pub const VALID_FOLLOW: &str = "((f.group_id IS NOT NULL AND EXISTS ( \
         SELECT 1 FROM group_members fgm \
         WHERE fgm.group_id = f.group_id AND fgm.user_id = f.user_id)) \
     OR (f.link_token_hash IS NOT NULL AND EXISTS ( \
         SELECT 1 FROM unlisted_links ful \
         WHERE ful.token_hash = f.link_token_hash AND ful.revoked_at IS NULL \
           AND (ful.expires_at IS NULL OR ful.expires_at > now()))))";

/// How a follow reached the journey.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum FollowVia {
    Group,
    Link,
}

/// Who shared a followed journey and how, for the "Shared by ..." label.
/// Only what the share already shows: a group member sees the sharer's
/// name in the group, so a group follow carries it (by the same rule as
/// every other member label, `users::MemberDisplay`) and the group's name;
/// a share link shows nothing about its owner, so a link follow carries
/// neither.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FollowedFrom {
    pub via: FollowVia,
    /// The owner's display name; `None` for a link, or when no name is on
    /// file (then `owner_tag` is set, as `GroupJourney.addedByTag`).
    pub owner_name: Option<String>,
    pub owner_tag: Option<String>,
    /// The group the journey was followed through; `None` for a link.
    pub group_id: Option<String>,
    pub group_name: Option<String>,
    pub followed_at: DateTime<Utc>,
}

#[derive(Debug, sqlx::FromRow)]
struct FollowRow {
    journey_id: i64,
    created_at: DateTime<Utc>,
    group_id: Option<String>,
    group_name: Option<String>,
    owner_id: String,
    owner_name: Option<String>,
    owner_username: Option<String>,
}

impl FollowRow {
    fn into_followed_from(self) -> (i64, FollowedFrom) {
        let followed = match self.group_id {
            Some(group_id) => {
                let owner =
                    users::MemberDisplay::of(self.owner_name, self.owner_username, &self.owner_id);
                FollowedFrom {
                    via: FollowVia::Group,
                    owner_name: owner.label,
                    owner_tag: owner.tag,
                    group_id: Some(group_id),
                    group_name: self.group_name,
                    followed_at: self.created_at,
                }
            }
            None => FollowedFrom {
                via: FollowVia::Link,
                owner_name: None,
                owner_tag: None,
                group_id: None,
                group_name: None,
                followed_at: self.created_at,
            },
        };
        (self.journey_id, followed)
    }
}

/// `user_id`'s follows still backed by their share, keyed by journey id;
/// only the journeys in `journey_ids` when given.
pub async fn valid_follows(
    pool: &PgPool,
    user_id: &str,
    journey_ids: Option<&[i64]>,
) -> Result<HashMap<i64, FollowedFrom>> {
    let rows: Vec<FollowRow> = sqlx::query_as(&format!(
        "SELECT f.journey_id, f.created_at, f.group_id, g.name AS group_name, \
                j.user_id AS owner_id, u.name AS owner_name, u.username AS owner_username \
         FROM journey_follows f \
         JOIN journeys j ON j.id = f.journey_id \
         JOIN users u ON u.id = j.user_id \
         LEFT JOIN groups g ON g.id = f.group_id \
         WHERE f.user_id = $1 AND j.user_id <> $1 \
           AND ($2::bigint[] IS NULL OR f.journey_id = ANY($2)) \
           AND {VALID_FOLLOW}"
    ))
    .bind(user_id)
    .bind(journey_ids)
    .fetch_all(pool)
    .await?;
    Ok(rows
        .into_iter()
        .map(FollowRow::into_followed_from)
        .collect())
}

/// Which of `journey_ids` `user_id` follows (validly).
pub async fn followed_ids(
    pool: &PgPool,
    user_id: &str,
    journey_ids: &[i64],
) -> Result<HashSet<i64>> {
    if journey_ids.is_empty() {
        return Ok(HashSet::new());
    }
    Ok(valid_follows(pool, user_id, Some(journey_ids))
        .await?
        .into_keys()
        .collect())
}

/// `user_id`'s follow of `journey_id`, if it is still backed by its share.
pub async fn follow_of(
    pool: &PgPool,
    user_id: &str,
    journey_id: i64,
) -> Result<Option<FollowedFrom>> {
    Ok(valid_follows(pool, user_id, Some(&[journey_id]))
        .await?
        .remove(&journey_id))
}

/// Whether `user_id` may read `journey_id` through a follow (for
/// `journeys::journey_readable_by`).
pub async fn follows_validly(pool: &PgPool, user_id: &str, journey_id: i64) -> Result<bool> {
    let (valid,): (bool,) = sqlx::query_as(&format!(
        "SELECT EXISTS (SELECT 1 FROM journey_follows f \
                        WHERE f.user_id = $1 AND f.journey_id = $2 AND {VALID_FOLLOW})"
    ))
    .bind(user_id)
    .bind(journey_id)
    .fetch_one(pool)
    .await?;
    Ok(valid)
}

/// Whether `user_id` can follow `journey_id` through a group: it is shared
/// into a group they are a member of, and they don't own it.
pub async fn can_follow_through_group(
    pool: &PgPool,
    user_id: &str,
    journey_id: i64,
) -> Result<bool> {
    Ok(group_share_for(pool, user_id, journey_id, None)
        .await?
        .is_some())
}

/// The group through which `user_id` sees `journey_id`: `group_id` when
/// given and valid, else the first such group by id. `None` when there is
/// none, or when `user_id` owns the journey.
async fn group_share_for<'c, E>(
    executor: E,
    user_id: &str,
    journey_id: i64,
    group_id: Option<&str>,
) -> Result<Option<String>>
where
    E: sqlx::PgExecutor<'c>,
{
    Ok(sqlx::query_scalar(
        "SELECT gj.group_id FROM group_journeys gj \
         JOIN group_members gm ON gm.group_id = gj.group_id AND gm.user_id = $1 \
         JOIN journeys j ON j.id = gj.journey_id AND j.user_id <> $1 \
         WHERE gj.journey_id = $2 AND ($3::text IS NULL OR gj.group_id = $3) \
         ORDER BY gj.group_id LIMIT 1",
    )
    .bind(user_id)
    .bind(journey_id)
    .bind(group_id)
    .fetch_optional(executor)
    .await?)
}

/// The share a follow request names.
#[derive(Debug, Clone, Copy)]
pub enum FollowThrough<'a> {
    /// A group the journey is shared into; `None` picks any such group the
    /// caller is in.
    Group(Option<&'a str>),
    /// The plaintext token of the journey's share link.
    Link(&'a str),
}

/// What [`follow_journey`] did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FollowOutcome {
    Followed,
    /// Already following: the existing follow is kept as it was.
    AlreadyFollowing,
    /// The caller owns the journey: there is nothing to follow.
    OwnJourney,
    /// No such journey, or the named share doesn't let the caller in. The
    /// route answers `404` for both, as every journey read does.
    NotFound,
}

/// Follows `journey_id` for `user_id` through `through`. Idempotent: a
/// repeat leaves the first follow (and the share it came through) as it
/// was. The share is checked in the same transaction as the insert, and the
/// foreign keys onto `group_journeys`/`unlisted_links` hold the shared row
/// until commit.
pub async fn follow_journey(
    pool: &PgPool,
    user_id: &str,
    journey_id: i64,
    through: FollowThrough<'_>,
) -> Result<FollowOutcome> {
    let mut tx = pool.begin().await?;
    let owner: Option<String> =
        sqlx::query_scalar("SELECT user_id FROM journeys WHERE id = $1 FOR KEY SHARE")
            .bind(journey_id)
            .fetch_optional(&mut *tx)
            .await?;
    let Some(owner) = owner else {
        return Ok(FollowOutcome::NotFound);
    };
    if owner == user_id {
        return Ok(FollowOutcome::OwnJourney);
    }

    let (group_id, link_token_hash) = match through {
        FollowThrough::Group(group_id) => {
            match group_share_for(&mut *tx, user_id, journey_id, group_id).await? {
                Some(group_id) => (Some(group_id), None),
                None => return Ok(FollowOutcome::NotFound),
            }
        }
        FollowThrough::Link(token) => {
            let hash: Option<String> = sqlx::query_scalar(
                "SELECT token_hash FROM unlisted_links \
                 WHERE token_hash = $1 AND resource_type = $2 AND resource_id = $3 \
                   AND revoked_at IS NULL AND (expires_at IS NULL OR expires_at > now()) \
                 FOR KEY SHARE",
            )
            .bind(unlisted_links::hash_link_token(token))
            .bind(JOURNEY_RESOURCE_TYPE)
            .bind(journey_id.to_string())
            .fetch_optional(&mut *tx)
            .await?;
            match hash {
                Some(hash) => (None, Some(hash)),
                None => return Ok(FollowOutcome::NotFound),
            }
        }
    };

    // A follow whose share has since lapsed is replaced, not kept: the
    // caller has just shown they can still see the journey.
    sqlx::query(&format!(
        "DELETE FROM journey_follows f \
         WHERE f.user_id = $1 AND f.journey_id = $2 AND NOT {VALID_FOLLOW}"
    ))
    .bind(user_id)
    .bind(journey_id)
    .execute(&mut *tx)
    .await?;
    let inserted = sqlx::query(
        "INSERT INTO journey_follows (user_id, journey_id, group_id, link_token_hash) \
         VALUES ($1, $2, $3, $4) ON CONFLICT (user_id, journey_id) DO NOTHING",
    )
    .bind(user_id)
    .bind(journey_id)
    .bind(group_id)
    .bind(link_token_hash)
    .execute(&mut *tx)
    .await?
    .rows_affected();
    tx.commit().await?;
    Ok(if inserted > 0 {
        FollowOutcome::Followed
    } else {
        FollowOutcome::AlreadyFollowing
    })
}

/// Stops `user_id` following `journey_id`. Idempotent: `false` when there
/// was no follow.
pub async fn unfollow_journey(pool: &PgPool, user_id: &str, journey_id: i64) -> Result<bool> {
    let result = sqlx::query("DELETE FROM journey_follows WHERE user_id = $1 AND journey_id = $2")
        .bind(user_id)
        .bind(journey_id)
        .execute(pool)
        .await?;
    Ok(result.rows_affected() > 0)
}

/// Deletes the follows of `journey_id` that came through a link that is no
/// longer live: after the owner revokes or regenerates the journey's share
/// link (`routes::journeys`). Returns how many went.
pub async fn drop_dead_link_follows(pool: &PgPool, journey_id: i64) -> Result<u64> {
    Ok(sqlx::query(
        "DELETE FROM journey_follows f USING unlisted_links ul \
         WHERE f.journey_id = $1 AND ul.token_hash = f.link_token_hash \
           AND (ul.revoked_at IS NOT NULL OR (ul.expires_at IS NOT NULL AND ul.expires_at <= now()))",
    )
    .bind(journey_id)
    .execute(pool)
    .await?
    .rows_affected())
}

/// Deletes every follow no longer backed by its share (an expired link, or
/// a cleanup that raced the follow), for the hourly retention sweep
/// (`retention::prune_personal_data`). Returns how many went.
pub async fn prune_lapsed_follows<'c, E>(executor: E) -> Result<u64>
where
    E: sqlx::PgExecutor<'c>,
{
    Ok(sqlx::query(&format!(
        "DELETE FROM journey_follows f WHERE NOT {VALID_FOLLOW}"
    ))
    .execute(executor)
    .await?
    .rows_affected())
}

#[cfg(test)]
pub(crate) mod db_tests {
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

    pub(crate) async fn exec(pool: &PgPool, sql: &str, binds: &[&str]) {
        let mut query = sqlx::query(sql);
        for bind in binds {
            query = query.bind(*bind);
        }
        query
            .execute(pool)
            .await
            .unwrap_or_else(|err| panic!("{sql}: {err}"));
    }

    /// Two users, `{prefix}-owner` (named "Olive Owner") and
    /// `{prefix}-friend`, a group `{prefix}-g` they are both in, and a
    /// journey of the owner's with one untracked leg `hours_ahead` from
    /// now. Returns the journey id.
    pub(crate) async fn seed_shared_journey(pool: &PgPool, prefix: &str, hours_ahead: i64) -> i64 {
        cleanup(pool, prefix).await;
        let owner = format!("{prefix}-owner");
        let friend = format!("{prefix}-friend");
        let group = format!("{prefix}-g");
        exec(
            pool,
            "INSERT INTO users (id, name) VALUES ($1, 'Olive Owner'), ($2, $2)",
            &[&owner, &friend],
        )
        .await;
        exec(
            pool,
            "INSERT INTO groups (id, name, created_by) VALUES ($1, 'Family', $2)",
            &[&group, &owner],
        )
        .await;
        exec(
            pool,
            "INSERT INTO group_members (group_id, user_id, role) \
             VALUES ($1, $2, 'owner'), ($1, $3, 'member')",
            &[&group, &owner, &friend],
        )
        .await;
        let journey_id = seed_journey(pool, &owner, hours_ahead).await;
        exec(
            pool,
            &format!(
                "INSERT INTO group_journeys (group_id, journey_id, added_by) VALUES ($1, {journey_id}, $2)"
            ),
            &[&group, &owner],
        )
        .await;
        journey_id
    }

    /// A journey of `user_id`'s with one untracked leg `hours_ahead` hours
    /// from now (negative: in the past).
    pub(crate) async fn seed_journey(pool: &PgPool, user_id: &str, hours_ahead: i64) -> i64 {
        let journey_id: i64 =
            sqlx::query_scalar("INSERT INTO journeys (user_id) VALUES ($1) RETURNING id")
                .bind(user_id)
                .fetch_one(pool)
                .await
                .expect("seed journey");
        sqlx::query(
            "INSERT INTO journey_legs (journey_id, leg_order, service_date, match_mode, kind, \
                                       transit_mode, planned_departure, planned_arrival) \
             VALUES ($1, 1, ((now() + make_interval(hours => $2)) AT TIME ZONE 'Europe/London')::date, \
                     'manual', 'untracked', 'bus', now() + make_interval(hours => $2), \
                     now() + make_interval(hours => $2 + 1))",
        )
        .bind(journey_id)
        .bind(i32::try_from(hours_ahead).expect("small"))
        .execute(pool)
        .await
        .expect("seed leg");
        journey_id
    }

    pub(crate) async fn cleanup(pool: &PgPool, prefix: &str) {
        let like = format!("{prefix}-%");
        for sql in [
            "DELETE FROM unlisted_links WHERE created_by LIKE $1",
            "DELETE FROM groups WHERE id LIKE $1",
            "DELETE FROM journeys WHERE user_id LIKE $1",
            "DELETE FROM users WHERE id LIKE $1",
        ] {
            sqlx::query(sql)
                .bind(&like)
                .execute(pool)
                .await
                .expect("cleanup");
        }
    }

    async fn follow_count(pool: &PgPool, journey_id: i64) -> i64 {
        sqlx::query_scalar("SELECT COUNT(*) FROM journey_follows WHERE journey_id = $1")
            .bind(journey_id)
            .fetch_one(pool)
            .await
            .expect("count follows")
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `cargo test -p api journey_follows -- --ignored --test-threads=1`"]
    async fn a_group_member_follows_and_unfollows_a_shared_journey() {
        let pool = connect().await;
        let prefix = "jf-group";
        let journey = seed_shared_journey(&pool, prefix, 2).await;
        let friend = format!("{prefix}-friend");
        let owner = format!("{prefix}-owner");

        assert_eq!(
            follow_journey(&pool, &friend, journey, FollowThrough::Group(None))
                .await
                .unwrap(),
            FollowOutcome::Followed
        );
        assert_eq!(
            follow_journey(&pool, &friend, journey, FollowThrough::Group(None))
                .await
                .unwrap(),
            FollowOutcome::AlreadyFollowing
        );
        let follow = follow_of(&pool, &friend, journey)
            .await
            .unwrap()
            .expect("following");
        assert_eq!(follow.via, FollowVia::Group);
        assert_eq!(follow.owner_name.as_deref(), Some("Olive Owner"));
        assert_eq!(follow.group_name.as_deref(), Some("Family"));
        assert!(follows_validly(&pool, &friend, journey).await.unwrap());
        assert!(
            crate::data::journeys::journey_readable_by(&pool, journey, &friend)
                .await
                .unwrap()
        );

        // The owner can't follow their own journey; a stranger can't follow
        // through a group they aren't in, nor name someone else's group.
        assert_eq!(
            follow_journey(&pool, &owner, journey, FollowThrough::Group(None))
                .await
                .unwrap(),
            FollowOutcome::OwnJourney
        );
        exec(
            &pool,
            "INSERT INTO users (id) VALUES ($1)",
            &[&format!("{prefix}-stranger")],
        )
        .await;
        assert_eq!(
            follow_journey(
                &pool,
                &format!("{prefix}-stranger"),
                journey,
                FollowThrough::Group(Some(&format!("{prefix}-g")))
            )
            .await
            .unwrap(),
            FollowOutcome::NotFound
        );
        assert_eq!(
            follow_journey(&pool, &friend, i64::MAX, FollowThrough::Group(None))
                .await
                .unwrap(),
            FollowOutcome::NotFound
        );

        assert!(unfollow_journey(&pool, &friend, journey).await.unwrap());
        assert!(!unfollow_journey(&pool, &friend, journey).await.unwrap());
        assert_eq!(follow_count(&pool, journey).await, 0);
        cleanup(&pool, prefix).await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `cargo test -p api journey_follows -- --ignored --test-threads=1`"]
    async fn unsharing_the_journey_from_the_group_ends_the_follow() {
        let pool = connect().await;
        let prefix = "jf-unshare";
        let journey = seed_shared_journey(&pool, prefix, 2).await;
        let friend = format!("{prefix}-friend");
        let owner = format!("{prefix}-owner");
        follow_journey(&pool, &friend, journey, FollowThrough::Group(None))
            .await
            .unwrap();

        crate::data::groups::remove_journey_from_group(
            &pool,
            &format!("{prefix}-g"),
            journey,
            &owner,
            false,
        )
        .await
        .unwrap();
        assert_eq!(follow_count(&pool, journey).await, 0);
        assert!(follow_of(&pool, &friend, journey).await.unwrap().is_none());
        assert!(
            !crate::data::journeys::journey_readable_by(&pool, journey, &friend)
                .await
                .unwrap()
        );
        cleanup(&pool, prefix).await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `cargo test -p api journey_follows -- --ignored --test-threads=1`"]
    async fn leaving_the_group_ends_the_follow_and_reads_check_membership() {
        let pool = connect().await;
        let prefix = "jf-leave";
        let journey = seed_shared_journey(&pool, prefix, 2).await;
        let friend = format!("{prefix}-friend");
        let group = format!("{prefix}-g");
        follow_journey(&pool, &friend, journey, FollowThrough::Group(None))
            .await
            .unwrap();

        // At read time first: a membership row gone without the cleanup
        // (a race) is not honoured, and the sweep then deletes the follow.
        exec(
            &pool,
            "DELETE FROM group_members WHERE group_id = $1 AND user_id = $2",
            &[&group, &friend],
        )
        .await;
        assert!(follow_of(&pool, &friend, journey).await.unwrap().is_none());
        assert!(!follows_validly(&pool, &friend, journey).await.unwrap());
        assert!(prune_lapsed_follows(&pool).await.unwrap() >= 1);
        assert_eq!(follow_count(&pool, journey).await, 0);

        // Then the real path: leaving deletes it.
        exec(
            &pool,
            "INSERT INTO group_members (group_id, user_id, role) VALUES ($1, $2, 'member')",
            &[&group, &friend],
        )
        .await;
        follow_journey(&pool, &friend, journey, FollowThrough::Group(None))
            .await
            .unwrap();
        crate::data::groups::remove_member(&pool, &group, &friend, true)
            .await
            .unwrap();
        assert_eq!(follow_count(&pool, journey).await, 0);
        cleanup(&pool, prefix).await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `cargo test -p api journey_follows -- --ignored --test-threads=1`"]
    async fn a_link_follow_lasts_while_the_link_does() {
        let pool = connect().await;
        let prefix = "jf-link";
        let journey = seed_shared_journey(&pool, prefix, 2).await;
        let owner = format!("{prefix}-owner");
        let reader = format!("{prefix}-reader");
        exec(&pool, "INSERT INTO users (id) VALUES ($1)", &[&reader]).await;
        let resource = journey.to_string();
        let link =
            unlisted_links::rotate_link(&pool, JOURNEY_RESOURCE_TYPE, &resource, &owner, None)
                .await
                .unwrap();

        // A wrong token, or a token for another resource, follows nothing.
        assert_eq!(
            follow_journey(&pool, &reader, journey, FollowThrough::Link("nope"))
                .await
                .unwrap(),
            FollowOutcome::NotFound
        );
        assert_eq!(
            follow_journey(&pool, &reader, journey, FollowThrough::Link(&link.token))
                .await
                .unwrap(),
            FollowOutcome::Followed
        );
        let follow = follow_of(&pool, &reader, journey).await.unwrap().unwrap();
        assert_eq!(follow.via, FollowVia::Link);
        // A link shows nothing about its owner, so neither does the follow.
        assert_eq!(follow.owner_name, None);
        assert_eq!(follow.owner_tag, None);
        assert!(
            crate::data::journeys::journey_readable_by(&pool, journey, &reader)
                .await
                .unwrap()
        );

        // Expired: not honoured at read time, and pruned by the sweep.
        exec(
            &pool,
            "UPDATE unlisted_links SET expires_at = now() - INTERVAL '1 minute' WHERE created_by = $1",
            &[&owner],
        )
        .await;
        assert!(follow_of(&pool, &reader, journey).await.unwrap().is_none());
        assert!(
            !crate::data::journeys::journey_readable_by(&pool, journey, &reader)
                .await
                .unwrap()
        );
        prune_lapsed_follows(&pool).await.unwrap();
        assert_eq!(follow_count(&pool, journey).await, 0);

        // Revoked: the route's cleanup deletes it.
        let link =
            unlisted_links::rotate_link(&pool, JOURNEY_RESOURCE_TYPE, &resource, &owner, None)
                .await
                .unwrap();
        follow_journey(&pool, &reader, journey, FollowThrough::Link(&link.token))
            .await
            .unwrap();
        unlisted_links::revoke_link(&pool, JOURNEY_RESOURCE_TYPE, &resource)
            .await
            .unwrap();
        assert!(follow_of(&pool, &reader, journey).await.unwrap().is_none());
        assert_eq!(drop_dead_link_follows(&pool, journey).await.unwrap(), 1);
        assert_eq!(follow_count(&pool, journey).await, 0);

        // A pruned dead link row cascades too.
        let link =
            unlisted_links::rotate_link(&pool, JOURNEY_RESOURCE_TYPE, &resource, &owner, None)
                .await
                .unwrap();
        follow_journey(&pool, &reader, journey, FollowThrough::Link(&link.token))
            .await
            .unwrap();
        exec(
            &pool,
            "DELETE FROM unlisted_links WHERE created_by = $1",
            &[&owner],
        )
        .await;
        assert_eq!(follow_count(&pool, journey).await, 0);
        cleanup(&pool, prefix).await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `cargo test -p api journey_follows -- --ignored --test-threads=1`"]
    async fn deleting_the_journey_ends_every_follow() {
        let pool = connect().await;
        let prefix = "jf-delete";
        let journey = seed_shared_journey(&pool, prefix, 2).await;
        let friend = format!("{prefix}-friend");
        let owner = format!("{prefix}-owner");
        follow_journey(&pool, &friend, journey, FollowThrough::Group(None))
            .await
            .unwrap();
        assert!(
            crate::data::journeys::delete_journey(&pool, journey, &owner)
                .await
                .unwrap()
        );
        assert_eq!(follow_count(&pool, journey).await, 0);
        cleanup(&pool, prefix).await;
    }

    /// The one-share CHECK: a follow names a group or a link, not both or
    /// neither.
    #[tokio::test]
    #[ignore = "requires a live database; run with `cargo test -p api journey_follows -- --ignored --test-threads=1`"]
    async fn a_follow_names_exactly_one_share() {
        let pool = connect().await;
        let prefix = "jf-check";
        let journey = seed_shared_journey(&pool, prefix, 2).await;
        let friend = format!("{prefix}-friend");
        let neither =
            sqlx::query("INSERT INTO journey_follows (user_id, journey_id) VALUES ($1, $2)")
                .bind(&friend)
                .bind(journey)
                .execute(&pool)
                .await;
        assert!(neither.is_err(), "a follow through no share was accepted");
        cleanup(&pool, prefix).await;
    }
}
