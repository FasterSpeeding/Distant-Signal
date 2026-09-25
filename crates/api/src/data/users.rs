//! Queries for `users`/`sessions`/`oidc_login_state` -- the tables Task
//! 1's migration creates. See
//! docs/superpowers/specs/2026-08-28-user-accounts-sso-design.md's Data
//! model section.

use anyhow::Result;
use sqlx::PgPool;

use crate::auth::oidc::OidcIdentity;

/// Only ever `Some` when the ID token asserted `email_verified: true` --
/// the actual enforcement point for design doc Open Question 2 (see
/// `crates/api/src/auth/oidc.rs`'s `identity_from_claims` doc comment,
/// which maps the claim through unfiltered; this is where it's filtered).
fn verified_email(identity: &OidcIdentity) -> Option<&str> {
    identity
        .email_verified
        .then_some(identity.email.as_deref())
        .flatten()
}

/// A claim that is PRESENT but blank (`""`, or whitespace only) carries
/// exactly as much information as an absent one, and every consumer of
/// `users.name`/`users.email` in this app treats "absent" as "fall back to
/// something else" -- so the two have to be made indistinguishable here,
/// at the boundary, rather than at each of those consumers.
///
/// This is not hypothetical: an identity provider with no name on file for
/// a user generally sends `"name": ""` rather than omitting the claim
/// (Authentik's own `profile` scope mapping returns the user's `name`
/// attribute verbatim, and that attribute is optional and defaults to the
/// empty string). Stored raw, that empty string answers every
/// `Option`-shaped "does this user have a name?" downstream with a
/// confident yes -- `Some("")` is `Some`, and `"" ?? placeholder` is `""`
/// -- and so gets rendered as the label itself: a group member row that is
/// just a role badge, a "Shared by " with nothing after it, an empty
/// nav-bar label.
///
/// Also trims: a name of `"  Ada  "` is `"Ada"`, never rendered with its
/// padding intact.
///
/// Applied when writing a row (`upsert_user`) and again on the read path
/// that renders a label to OTHER people (`display_label`), so rows stored
/// before any of this normalization existed read the same as rows written
/// today. The remaining read path -- `get_session_with_user`, showing a
/// user their own name in the nav bar -- returns `u.name` raw and leans on
/// the frontend's own `?.trim() ||`; that one is self-view only, so a blank
/// there costs a fallback, not a wrong label shown to someone else.
///
/// `auth::oidc` has its own twin of this, applied one layer earlier while
/// choosing WHICH claim becomes the name/username. Two one-line copies of
/// `trim` + "is it empty" is the deliberate trade against a mutual
/// dependency between these modules, whose production code points one way
/// only (`data::users` -> `auth::oidc`; test code here and there crosses
/// back, to assert the two layers compose). Contrast
/// `auth::oidc::looks_like_email_address`, which is privacy-load-bearing,
/// has to stay identical at both sites, and so is shared outright.
fn non_blank(value: Option<&str>) -> Option<&str> {
    value.map(str::trim).filter(|trimmed| !trimmed.is_empty())
}

/// The one label this app is willing to show OTHER people for a user:
/// their `users.name`, else their `users.username`, else nothing at all.
///
/// Deliberately never the email, which is what the group member list and
/// shared-train attribution used to fall back to. A group is a set of
/// people who may have joined via nothing but a link, and shipping one
/// member's email address to the rest of them is a privacy leak, not a
/// display-name fallback -- so the fallback is the OTHER non-email
/// identifier this app stores: `username`, from the standard
/// `preferred_username` claim (`auth::oidc`). `id` is not a candidate at
/// any point: it is the opaque OIDC subject, not a name.
///
/// `None` -- nothing usable on file -- is the signal for the frontend to
/// render its own generic placeholder ("A member" / "a member") instead,
/// distinguished between users by `MemberDisplay`'s tag.
pub fn display_label(name: Option<String>, username: Option<String>) -> Option<String> {
    shareable(name.as_deref())
        .or_else(|| shareable(username.as_deref()))
        .map(str::to_string)
}

/// `non_blank`, plus: not an email address.
///
/// Not selecting `users.email` is only half of "never show one member's
/// email to the others" -- the other half is that an IdP is perfectly
/// entitled to put an email address in the claims we DO show. Authentik
/// can be configured with a user's email as their username, and plenty of
/// directories set the `name` attribute to the email for accounts created
/// in bulk. A value containing `@` is treated as an email and declined,
/// falling through to the next candidate and ultimately to the generic
/// placeholder: "we can't say who this is" is the correct outcome there,
/// not "here is their email address".
///
/// `@` is a deliberately blunt test: the cost of a false negative -- a
/// leaked email address -- is much higher than the cost of a false
/// positive, which is a member shown as the generic placeholder. The test
/// itself lives in `auth::oidc::looks_like_email_address`, shared with the
/// claim boundary rather than written out twice: `identity_from_claims`
/// uses the SAME predicate to rank an email-shaped claim below a non-email
/// alternative, and the two must agree or the boundary could promote a
/// value this function then silently declines.
///
/// This stays the enforcement point regardless. The boundary only reorders
/// candidates; when every claim the IdP sent is email-shaped it still
/// stores one, and this is what refuses to render it.
///
/// Note what that means on some deployments. On Entra ID / Azure AD,
/// `preferred_username` IS the UPN and is therefore email-shaped for
/// essentially every user, so this fallback is inert there by
/// construction, not just occasionally -- everyone without a `name` shows
/// as "A member". That is the intended trade, not a bug to go debugging:
/// the alternative is showing the whole group an address.
fn shareable(value: Option<&str>) -> Option<&str> {
    non_blank(value).filter(|candidate| !crate::auth::oidc::looks_like_email_address(candidate))
}

/// Domain separation for `anonymous_tag`, so the digest this app renders
/// can never coincide with some other digest of the same user id computed
/// for an unrelated purpose (a cache key, an ETag) and turn one into an
/// oracle for the other.
const ANONYMOUS_TAG_DOMAIN: &str = "network-rail-status/member-display-tag/v1:";

/// Six lowercase hex characters that are the same for one user every time
/// and different between users -- the thing that makes two members who BOTH
/// render as the generic placeholder tell apart as "A member (#a1b2c3)" and
/// "A member (#d4e5f6)" rather than as two identical rows.
///
/// Derived from `users.id` -- the opaque OIDC subject -- and from nothing
/// else. That is the whole privacy argument, and it is deliberately not an
/// argument about the hash being strong:
///
/// * Everyone who can see a tag can ALREADY see the id it came from. The
///   id is a field of the very same JSON (`GroupMember.user_id`,
///   `GroupTrain.added_by`, `GroupDetail.owner_id` -- the frontend needs
///   it to key rows and to gate the remove/promote buttons), and that JSON
///   is reachable by every member of the group, not only by the ones whose
///   rendered page happens to carry the id: `routes::groups::list_members`
///   gates on `require_member` with no role check, and the frontend's
///   same-origin `/api/[...path]` proxy will forward a plain member's own
///   `GET /api/groups/{id}/members` straight to it. A value computed from
///   nothing but a field its whole audience can already fetch tells that
///   audience nothing new, whatever the function is: the tag is strictly
///   less informative than the `userId` it is derived from.
/// * Email, name and username are not inputs. There is therefore no
///   candidate-email dictionary to run against the tag at all: hashing a
///   guessed address produces nothing comparable to it. This is the
///   [[feedback-no-email-exposure]] rule applied to the derivation and not
///   only to the rendered string -- the tag must not be a roundabout way of
///   shipping an address, so an address never enters it.
/// * SHA-256 truncated to 24 bits is one-way in the only direction that
///   matters here anyway: ~16.7M subjects share any given tag, so the tag
///   pins down no individual even for someone who can enumerate subjects.
///
/// Six characters, hex, always rendered inside "(#...)": short enough to
/// scan down a member list, and visibly a machine token rather than a name
/// a reader might mistake for the person's actual one.
///
/// Collisions are possible and harmless: two colliding members render
/// identically, which is exactly the pre-fix behaviour for that pair and
/// no worse -- nothing keys off the tag, it is a display string. Know the
/// real numbers before leaning on it, though: 24 bits is 16.7M values, so
/// by the birthday bound a group of ~20 members has roughly a 1-in-90,000
/// chance of any collision at all, while ~4,800 members in one group would
/// be an even-money bet. That is comfortable for the friend/family-sized
/// groups this feature is for and NOT comfortable for an org-wide one.
/// Widening it (4 bytes, or base32 rather than hex) is a display change
/// rather than a correctness fix, but is not free either: the tag is also
/// what a member recognises another member by across visits, so changing
/// the derivation renames everybody at once.
fn anonymous_tag(user_id: &str) -> String {
    use sha2::{Digest, Sha256};

    let digest = Sha256::digest(format!("{ANONYMOUS_TAG_DOMAIN}{user_id}").as_bytes());
    digest[..3]
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

/// What one user looks like to OTHER users: the label if this app has one
/// it is willing to show (`display_label`), and otherwise a tag to
/// distinguish them by (`anonymous_tag`) -- never both, never neither.
///
/// The two are a pair and are built together here rather than at each
/// render site, because "the tag appears only when there is no label" is
/// the property that keeps a real name unsuffixed (a member called Ada
/// renders as "Ada", exactly as before this existed) and it should be
/// impossible for one of the four call sites in `data::groups` to get it
/// subtly wrong on its own.
///
/// The frontend composes the visible string, not this: it owns the
/// placeholder's wording and its capitalisation, which differs by position
/// ("A member (#a1b2c3)" standing alone in a member row, "Shared by a
/// member (#a1b2c3)" mid-sentence). Sending the tag rather than a finished
/// "A member (#a1b2c3)" also keeps `displayName`'s wire contract honest:
/// that field is a person's name or `null`, and a consumer that ignores
/// `displayTag` entirely still behaves exactly as it did before.
pub struct MemberDisplay {
    pub label: Option<String>,
    pub tag: Option<String>,
}

impl MemberDisplay {
    pub fn of(name: Option<String>, username: Option<String>, user_id: &str) -> Self {
        let label = display_label(name, username);
        let tag = label.is_none().then(|| anonymous_tag(user_id));
        MemberDisplay { label, tag }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn identity(email_verified: bool) -> OidcIdentity {
        OidcIdentity {
            sub: "user-123".to_string(),
            email: Some("rider@example.com".to_string()),
            email_verified,
            name: Some("Ada Rider".to_string()),
            preferred_username: Some("ada".to_string()),
            groups: Vec::new(),
        }
    }

    #[test]
    fn verified_email_is_kept() {
        assert_eq!(verified_email(&identity(true)), Some("rider@example.com"));
    }

    #[test]
    fn unverified_email_is_dropped() {
        assert_eq!(verified_email(&identity(false)), None);
    }

    #[test]
    fn no_email_claim_at_all_is_none_regardless_of_verified_flag() {
        let mut i = identity(true);
        i.email = None;
        assert_eq!(verified_email(&i), None);
    }

    #[test]
    fn non_blank_keeps_a_real_value_but_trims_it() {
        assert_eq!(non_blank(Some("Ada Rider")), Some("Ada Rider"));
        assert_eq!(non_blank(Some("  Ada Rider  ")), Some("Ada Rider"));
    }

    #[test]
    fn non_blank_maps_every_shape_of_blank_onto_none() {
        assert_eq!(non_blank(None), None);
        assert_eq!(non_blank(Some("")), None);
        assert_eq!(non_blank(Some("   ")), None);
        assert_eq!(non_blank(Some("\t\n")), None);
    }

    fn label(name: Option<&str>, username: Option<&str>) -> Option<String> {
        display_label(name.map(str::to_string), username.map(str::to_string))
    }

    #[test]
    fn display_label_prefers_the_name_and_trims_it() {
        assert_eq!(
            label(Some("Ada Rider"), Some("ada")),
            Some("Ada Rider".to_string())
        );
        assert_eq!(
            label(Some("  Ada Rider  "), Some("ada")),
            Some("Ada Rider".to_string())
        );
    }

    /// Half the reported bug: a present-but-blank `name` is not a label.
    /// It used to be treated as one (`Some("")` is `Some`), which is what
    /// rendered an empty member row and a "Shared by " with nothing after
    /// it. It now falls through to the username, exactly as an absent name
    /// does.
    #[test]
    fn display_label_falls_through_a_blank_name_to_the_username() {
        assert_eq!(label(Some(""), Some("ada")), Some("ada".to_string()));
        assert_eq!(label(Some("   "), Some("ada")), Some("ada".to_string()));
        assert_eq!(label(None, Some("  ada  ")), Some("ada".to_string()));
    }

    /// Never an email, and never the opaque `users.id`: with neither a
    /// name nor a username on file there is simply nothing this app is
    /// willing to show other members, and `None` is how it says so.
    #[test]
    fn display_label_is_none_when_neither_is_usable() {
        assert_eq!(label(Some(""), Some("")), None);
        assert_eq!(label(Some("   "), None), None);
        assert_eq!(label(None, None), None);
    }

    /// The other half of "never show one member's email to the rest of the
    /// group": not selecting `users.email` doesn't help if the IdP put an
    /// email address in the `name` or `preferred_username` claim instead,
    /// which plenty of directories do.
    #[test]
    fn display_label_declines_an_email_address_in_either_field() {
        assert_eq!(
            label(Some("rider@example.com"), Some("ada")),
            Some("ada".to_string())
        );
        assert_eq!(
            label(Some("Ada Rider"), Some("rider@example.com")),
            Some("Ada Rider".to_string())
        );
        assert_eq!(
            label(Some("rider@example.com"), Some("rider@example.com")),
            None
        );
        assert_eq!(label(None, Some("rider@example.com")), None);
    }

    fn display(name: Option<&str>, username: Option<&str>, user_id: &str) -> MemberDisplay {
        MemberDisplay::of(
            name.map(str::to_string),
            username.map(str::to_string),
            user_id,
        )
    }

    /// A member with a usable name is completely untouched by the tag: the
    /// label is what it always was, and there is no suffix for anything to
    /// append. Same for the username fallback.
    #[test]
    fn a_member_with_a_real_name_or_username_is_unchanged_and_untagged() {
        let named = display(Some("Ada Rider"), Some("ada"), "user-1");
        assert_eq!(named.label.as_deref(), Some("Ada Rider"));
        assert_eq!(named.tag, None);

        let by_username = display(Some("  "), Some("ada"), "user-1");
        assert_eq!(by_username.label.as_deref(), Some("ada"));
        assert_eq!(by_username.tag, None);
    }

    /// The bug this exists for. On an IdP where the username claim IS the
    /// user's email by design (Entra ID's `preferred_username` is the UPN),
    /// every member's name AND username are email-shaped, every label is
    /// declined, and every row used to render as the same "A member" --
    /// leaving an admin no way to tell who added a shared train or who to
    /// remove. Two such members must now differ.
    #[test]
    fn two_members_with_nothing_showable_still_render_differently() {
        let one = display(
            Some("ada@example.com"),
            Some("ada@example.com"),
            "3f2504e0-4f89-11d3-9a0c-0305e82c3301",
        );
        let two = display(
            Some("grace@example.com"),
            Some("grace@example.com"),
            "3f2504e0-4f89-11d3-9a0c-0305e82c3302",
        );

        assert_eq!(one.label, None, "an email is still never a label");
        assert_eq!(two.label, None, "an email is still never a label");
        assert_ne!(one.tag, two.tag);
        assert!(one.tag.is_some() && two.tag.is_some());
    }

    /// Stable across renders, requests and process restarts: it is a pure
    /// function of the user id, with nothing random and nothing
    /// time-varying in it. A member recognised as "(#a1b2c3)" yesterday is
    /// the same "(#a1b2c3)" today.
    #[test]
    fn the_tag_is_stable_for_the_same_user() {
        let first = display(None, None, "user-1").tag;
        let second = display(Some(""), Some("   "), "user-1").tag;
        let third = display(Some("a@b.com"), None, "user-1").tag;
        assert_eq!(first, second);
        assert_eq!(second, third);
        assert_eq!(first, Some(anonymous_tag("user-1")));
    }

    /// Short, lowercase hex, and unmistakably not a person's name -- the
    /// row has to read as "anonymous but distinguishable", not as a fake
    /// name for someone.
    #[test]
    fn the_tag_is_six_lowercase_hex_characters() {
        for user_id in ["user-1", "", "3f2504e0-4f89-11d3-9a0c-0305e82c3301", "ß🙂"] {
            let tag = anonymous_tag(user_id);
            assert_eq!(tag.len(), 6, "tag for {user_id:?} was {tag:?}");
            assert!(
                tag.chars()
                    .all(|c| c.is_ascii_hexdigit() && !c.is_uppercase()),
                "tag for {user_id:?} was {tag:?}"
            );
        }
    }

    /// The derivation takes the opaque user id and NOTHING else. An email
    /// (or a name, or a username) attached to the same user cannot change
    /// the tag, so the tag carries no trace of one: there is nothing for a
    /// "hash the addresses I can guess and compare" attack to compare
    /// against, and the email guard above is not quietly undone by a
    /// suffix that encodes the address it just refused to show.
    #[test]
    fn the_tag_is_derived_from_the_user_id_alone_never_from_email_or_name() {
        let same_user_different_claims = [
            display(None, None, "user-1"),
            display(
                Some("rider@example.com"),
                Some("rider@example.com"),
                "user-1",
            ),
            display(Some("someone.else@example.org"), None, "user-1"),
        ];
        for d in &same_user_different_claims {
            assert_eq!(d.tag, same_user_different_claims[0].tag);
        }

        // And the id itself is not recoverable by looking at the tag: the
        // rendered value is 24 bits of digest, not an encoding of the
        // input, so it is neither the id nor any prefix/suffix of it.
        let user_id = "rider@example.com-as-a-subject";
        let tag = anonymous_tag(user_id);
        assert!(!tag.contains('@'));
        assert!(!user_id.contains(&tag));
        assert!(!tag.contains("rider"));
    }

    /// Different ids give different tags in the small-group case this is
    /// actually for -- a sanity check that the truncation didn't leave
    /// something degenerate like "always the same three bytes".
    #[test]
    fn nearby_user_ids_do_not_collide() {
        let tags: std::collections::HashSet<String> = (0..50)
            .map(|i| anonymous_tag(&format!("user-{i}")))
            .collect();
        assert_eq!(tags.len(), 50);
    }
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct User {
    pub id: String,
    pub email: Option<String>,
    pub name: Option<String>,
    pub username: Option<String>,
}

/// Creates the user on first login, or updates `email`/`name`/`groups`/
/// `last_login_at` on every return visit -- design doc: "upserted, not
/// just inserted once." `groups` is overwritten wholesale, never merged
/// with what was already stored -- see
/// docs/superpowers/specs/2026-09-02-mcp-server-oauth-access-groups-design.md's
/// Global Constraints: a group removed in Authentik is reflected on the
/// user's very next login.
///
/// `name`/`username`/`email` are normalized through `non_blank` on the way
/// in, so a blank claim is stored as SQL `NULL` rather than as an empty
/// string -- the read side (`display_label`, and the session shape's own
/// name-else-email fallback) then needs no special case for a value this
/// app never writes. The read side normalizes too, for rows written before
/// this normalization existed.
///
/// `username` (the `preferred_username` claim) is overwritten on every
/// login for the same reason `name` and `email` are: this table mirrors
/// what the IdP currently asserts, it is not an independent record.
pub async fn upsert_user(pool: &PgPool, identity: &OidcIdentity) -> Result<User> {
    let email = non_blank(verified_email(identity));
    let name = non_blank(identity.name.as_deref());
    let username = non_blank(identity.preferred_username.as_deref());
    let row = sqlx::query_as::<_, User>(
        "INSERT INTO users (id, email, name, username, groups, created_at, last_login_at) \
         VALUES ($1, $2, $3, $4, $5, NOW(), NOW()) \
         ON CONFLICT (id) DO UPDATE SET \
            email = EXCLUDED.email, name = EXCLUDED.name, \
            username = EXCLUDED.username, groups = EXCLUDED.groups, \
            last_login_at = NOW() \
         RETURNING id, email, name, username",
    )
    .bind(&identity.sub)
    .bind(email)
    .bind(name)
    .bind(username)
    .bind(&identity.groups)
    .fetch_one(pool)
    .await?;
    Ok(row)
}

/// `sessions.refresh_token` is deliberately left NULL. The design doc's
/// only use for it is silent ID-token renewal before local session expiry
/// ("Expiry and refresh"), and nothing in this plan implements that -- the
/// column is written by no other path and read by none at all. Storing a
/// live IdP credential server-side with zero present consumer is pure
/// added blast radius on a database leak, and the design doc's own Open
/// Question 5 already flags that this schema would hold it in plaintext
/// with no column-encryption precedent anywhere in the repo.
///
/// The column itself stays in the schema, unused, for whoever implements
/// refresh: it is nullable, so nothing needs migrating when that lands,
/// and this is the only INSERT that would have to start binding it.
pub async fn insert_session(
    pool: &PgPool,
    hashed_token: &str,
    user_id: &str,
    ttl_days: i64,
) -> Result<()> {
    sqlx::query(
        "INSERT INTO sessions (id, user_id, refresh_token, created_at, expires_at) \
         VALUES ($1, $2, NULL, NOW(), NOW() + make_interval(days => $3))",
    )
    .bind(hashed_token)
    .bind(user_id)
    .bind(ttl_days as i32)
    .execute(pool)
    .await?;
    Ok(())
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct SessionUser {
    pub id: String,
    pub email: Option<String>,
    pub name: Option<String>,
    pub groups: Vec<String>,
}

/// Looks up a session by its *hashed* token and joins the owning user, but
/// only if it hasn't expired -- an expired row reads back identically to
/// no row at all. Expired rows are additionally, actually deleted (not
/// just excluded from lookups) by `prune_expired_sessions`'s own periodic
/// sweep (`main.rs`'s `session_cleanup_sweep_loop`) -- this function's own
/// `WHERE ... expires_at > NOW()` doesn't depend on that sweep having run
/// recently, it's just what keeps the table from growing without bound.
pub async fn get_session_with_user(
    pool: &PgPool,
    hashed_token: &str,
) -> Result<Option<SessionUser>> {
    let row = sqlx::query_as::<_, SessionUser>(
        "SELECT u.id, u.email, u.name, u.groups \
         FROM sessions s JOIN users u ON u.id = s.user_id \
         WHERE s.id = $1 AND s.expires_at > NOW()",
    )
    .bind(hashed_token)
    .fetch_optional(pool)
    .await?;
    Ok(row)
}

pub async fn delete_session(pool: &PgPool, hashed_token: &str) -> Result<()> {
    sqlx::query("DELETE FROM sessions WHERE id = $1")
        .bind(hashed_token)
        .execute(pool)
        .await?;
    Ok(())
}

/// Deletes every `sessions` row whose `expires_at` has already passed.
/// `get_session_with_user` already excludes an expired row from every
/// lookup (`WHERE s.expires_at > NOW()`), so a row this deletes was never
/// usable as a live session anyway -- this exists purely so the table
/// doesn't grow without bound on a long-lived deployment, backed by the
/// `sessions_expires_at` index (`migrations/20260925220000_sessions_expires_at_index.sql`)
/// so the sweep that calls this stays a cheap, index-only-ish DELETE
/// rather than a full scan as the table grows. Called periodically by
/// `main.rs`'s `session_cleanup_sweep_loop`, mirroring the "a
/// request/response server also runs one background interval loop"
/// pattern already established there for the schedule-match/
/// reconciliation/backlog-match sweeps.
///
/// Returns the number of rows deleted (for logging only -- callers should
/// never branch on this).
pub async fn prune_expired_sessions(pool: &PgPool) -> Result<u64> {
    let result = sqlx::query("DELETE FROM sessions WHERE expires_at <= NOW()")
        .execute(pool)
        .await?;
    Ok(result.rows_affected())
}

/// Deliberately derives NEITHER `Debug` nor `Clone`, unlike the other row
/// types in this module. All three fields are plaintext single-use
/// secrets, and a derived `Debug` is exactly how they end up in a log line
/// or a panic message -- the same reasoning that made `AppState`'s and
/// `OidcConfig`'s `Debug` impls hand-rolled and secret-redacting (see
/// `crate::app` and `crate::auth::oidc`); there is nothing worth printing
/// here at all, so this one simply goes without. Nothing debug-formats or
/// clones one today, and leaving the derives in place is just an open
/// invitation for a future `tracing::debug!(?stored)` to change that.
#[derive(sqlx::FromRow)]
pub struct LoginState {
    pub pkce_verifier: String,
    pub nonce: String,
    pub csrf_state: String,
    pub return_to: Option<String>,
}

pub async fn insert_login_state(
    pool: &PgPool,
    id: &str,
    pkce_verifier: &str,
    nonce: &str,
    csrf_state: &str,
    return_to: Option<&str>,
) -> Result<()> {
    // Opportunistic cleanup -- no cron needed for a table this small and
    // self-limiting; every login attempt takes out its own trash.
    sqlx::query("DELETE FROM oidc_login_state WHERE created_at < NOW() - INTERVAL '15 minutes'")
        .execute(pool)
        .await?;
    sqlx::query(
        "INSERT INTO oidc_login_state (id, pkce_verifier, nonce, csrf_state, return_to, created_at) \
         VALUES ($1, $2, $3, $4, $5, NOW())",
    )
    .bind(id)
    .bind(pkce_verifier)
    .bind(nonce)
    .bind(csrf_state)
    .bind(return_to)
    .execute(pool)
    .await?;
    Ok(())
}

/// Fetches and deletes in one step -- login state is single-use by
/// construction (a replayed callback with the same state must not
/// succeed twice). `None` if the id is unknown, already consumed, or
/// older than the 15-minute window `insert_login_state` also sweeps on.
pub async fn consume_login_state(pool: &PgPool, id: &str) -> Result<Option<LoginState>> {
    let row = sqlx::query_as::<_, LoginState>(
        "DELETE FROM oidc_login_state \
         WHERE id = $1 AND created_at > NOW() - INTERVAL '15 minutes' \
         RETURNING pkce_verifier, nonce, csrf_state, return_to",
    )
    .bind(id)
    .fetch_optional(pool)
    .await?;
    Ok(row)
}

#[cfg(test)]
mod db_tests {
    use super::*;

    #[tokio::test]
    #[ignore = "requires a live database; see the plan's Global Constraints for the \
                DATABASE_URL incantation, then run with `cargo test -p api \
                session_round_trip_creates_looks_up_and_deletes -- --ignored`"]
    async fn session_round_trip_creates_looks_up_and_deletes() {
        use sqlx::postgres::PgPoolOptions;

        let database_url =
            std::env::var("DATABASE_URL").expect("DATABASE_URL must be set to run this test");
        let pool = PgPoolOptions::new()
            .connect(&database_url)
            .await
            .expect("connect to postgres");

        let mut identity = OidcIdentity {
            sub: "TEST-USER-ROUND-TRIP".to_string(),
            email: Some("test@example.com".to_string()),
            email_verified: true,
            name: Some("Test Rider".to_string()),
            preferred_username: Some("test-rider".to_string()),
            groups: Vec::new(),
        };
        let user = upsert_user(&pool, &identity).await.expect("upsert user");
        assert_eq!(user.id, "TEST-USER-ROUND-TRIP");
        assert_eq!(user.name.as_deref(), Some("Test Rider"));
        assert_eq!(user.username.as_deref(), Some("test-rider"));

        // A blank `name` claim -- what an IdP with no name on file for the
        // user actually sends -- must be stored as NULL, not as `''`.
        // Storing `''` is what made `display_label`'s fallback (and the
        // frontend's own placeholder) fail to fire, rendering a member row
        // with an empty label. Same for a blank `preferred_username`, and
        // for the padding around an otherwise-real value.
        identity.name = Some("   ".to_string());
        identity.preferred_username = Some("  test-rider  ".to_string());
        let user = upsert_user(&pool, &identity).await.expect("re-upsert user");
        assert_eq!(user.name, None);
        assert_eq!(user.username.as_deref(), Some("test-rider"));

        identity.preferred_username = Some(String::new());
        let user = upsert_user(&pool, &identity).await.expect("re-upsert user");
        assert_eq!(user.username, None);

        identity.name = Some("Test Rider".to_string());

        insert_session(&pool, "test-hashed-token", &user.id, 14)
            .await
            .expect("insert session");

        let found = get_session_with_user(&pool, "test-hashed-token")
            .await
            .expect("lookup session")
            .expect("session should exist");
        assert_eq!(found.id, "TEST-USER-ROUND-TRIP");

        delete_session(&pool, "test-hashed-token")
            .await
            .expect("delete session");
        let gone = get_session_with_user(&pool, "test-hashed-token")
            .await
            .expect("lookup after delete");
        assert!(gone.is_none());

        // Cleanup -- cascades to sessions via ON DELETE CASCADE, though
        // the session row above was already explicitly deleted.
        sqlx::query("DELETE FROM users WHERE id = 'TEST-USER-ROUND-TRIP'")
            .execute(&pool)
            .await
            .expect("cleanup test user");
    }

    /// Reproduces the real gap this fixes: before `prune_expired_sessions`
    /// existed, an expired `sessions` row was excluded from every LOOKUP
    /// (`get_session_with_user`'s own `WHERE expires_at > NOW()`) but
    /// never actually deleted by anything -- the table only ever grew.
    /// Inserts one already-expired row and one still-live row directly
    /// (bypassing `insert_session`, which only ever writes a
    /// future-dated `expires_at`), then asserts the sweep deletes exactly
    /// the expired one and leaves the live one alone.
    #[tokio::test]
    #[ignore = "requires a live database; see the plan's Global Constraints for the \
                DATABASE_URL incantation, then run with `cargo test -p api \
                prune_expired_sessions_deletes_only_rows_past_their_expiry -- --ignored`"]
    async fn prune_expired_sessions_deletes_only_rows_past_their_expiry() {
        use sqlx::postgres::PgPoolOptions;

        let database_url =
            std::env::var("DATABASE_URL").expect("DATABASE_URL must be set to run this test");
        let pool = PgPoolOptions::new()
            .connect(&database_url)
            .await
            .expect("connect to postgres");

        let identity = OidcIdentity {
            sub: "TEST-USER-SESSION-PRUNE".to_string(),
            email: Some("test@example.com".to_string()),
            email_verified: true,
            name: Some("Test Rider".to_string()),
            preferred_username: Some("test-rider".to_string()),
            groups: Vec::new(),
        };
        let user = upsert_user(&pool, &identity).await.expect("upsert user");

        // An already-expired row -- what a session that outlived its TTL
        // and was never cleaned up looks like today.
        sqlx::query(
            "INSERT INTO sessions (id, user_id, refresh_token, created_at, expires_at) \
             VALUES ($1, $2, NULL, NOW() - INTERVAL '20 days', NOW() - INTERVAL '6 days')",
        )
        .bind("test-expired-session-token")
        .bind(&user.id)
        .execute(&pool)
        .await
        .expect("insert expired session");

        // A still-live row that must survive the sweep untouched.
        insert_session(&pool, "test-live-session-token", &user.id, 14)
            .await
            .expect("insert live session");

        let deleted = prune_expired_sessions(&pool)
            .await
            .expect("prune expired sessions");
        assert_eq!(
            deleted, 1,
            "exactly the one already-expired row should be deleted"
        );

        let expired_gone = get_session_with_user(&pool, "test-expired-session-token")
            .await
            .expect("lookup after prune");
        assert!(expired_gone.is_none());

        let live_still_there = get_session_with_user(&pool, "test-live-session-token")
            .await
            .expect("lookup after prune")
            .expect("live session must survive the sweep");
        assert_eq!(live_still_there.id, "TEST-USER-SESSION-PRUNE");

        // Running the sweep again must be a no-op -- idempotent, not an
        // error, and must not touch the surviving live row.
        let deleted_again = prune_expired_sessions(&pool)
            .await
            .expect("prune expired sessions a second time");
        assert_eq!(deleted_again, 0);

        // Cleanup.
        delete_session(&pool, "test-live-session-token")
            .await
            .expect("delete live session");
        sqlx::query("DELETE FROM users WHERE id = 'TEST-USER-SESSION-PRUNE'")
            .execute(&pool)
            .await
            .expect("cleanup test user");
    }

    #[tokio::test]
    #[ignore = "requires a live database; see the plan's Global Constraints for the \
                DATABASE_URL incantation, then run with `cargo test -p api \
                groups_are_overwritten_not_merged_on_repeat_login -- --ignored`"]
    async fn groups_are_overwritten_not_merged_on_repeat_login() {
        use sqlx::postgres::PgPoolOptions;

        let database_url =
            std::env::var("DATABASE_URL").expect("DATABASE_URL must be set to run this test");
        let pool = PgPoolOptions::new()
            .connect(&database_url)
            .await
            .expect("connect to postgres");

        let mut identity = OidcIdentity {
            sub: "TEST-USER-GROUPS-OVERWRITE".to_string(),
            email: Some("test@example.com".to_string()),
            email_verified: true,
            name: Some("Test Rider".to_string()),
            preferred_username: Some("test-rider".to_string()),
            groups: vec!["mcp-users".to_string(), "mcp-live-boards".to_string()],
        };
        let user = upsert_user(&pool, &identity)
            .await
            .expect("first login upsert");

        insert_session(&pool, "test-hashed-token-groups", &user.id, 14)
            .await
            .expect("insert session");
        let found = get_session_with_user(&pool, "test-hashed-token-groups")
            .await
            .expect("lookup session")
            .expect("session should exist");
        assert_eq!(
            found.groups,
            vec!["mcp-users".to_string(), "mcp-live-boards".to_string()]
        );

        // Second login, with mcp-live-boards removed in Authentik -- must
        // be reflected exactly, not unioned with the first login's set.
        identity.groups = vec!["mcp-users".to_string()];
        upsert_user(&pool, &identity)
            .await
            .expect("second login upsert");
        let found_again = get_session_with_user(&pool, "test-hashed-token-groups")
            .await
            .expect("lookup session")
            .expect("session should still exist");
        assert_eq!(found_again.groups, vec!["mcp-users".to_string()]);

        delete_session(&pool, "test-hashed-token-groups")
            .await
            .expect("delete session");
        sqlx::query("DELETE FROM users WHERE id = 'TEST-USER-GROUPS-OVERWRITE'")
            .execute(&pool)
            .await
            .expect("cleanup");
    }
}
