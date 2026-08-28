
use std::path::PathBuf;

use anyhow::Result;
use clap::ValueHint;
use serde::de::DeserializeOwned;

use crate::data::LineDefinition;

pub use common::Defaults;

fn parse_toml_path<T: DeserializeOwned>(path: &'_ str) -> Result<T> {
    let text = std::fs::read_to_string(path)?;
    Ok(toml::from_str(&text)?)
}

fn parse_lines(path: &str) -> Result<LineCatalogue> {
    LineDefinition::from_dir(&PathBuf::from(path)).map(LineCatalogue)
}

/// Newtype around the parsed line catalogue.
///
/// `clap_derive` infers the type it downcasts an `ArgMatches` entry to from
/// the field's *syntactic* shape, not from the `value_parser`'s `Value`
/// type: a bare `Vec<LineDefinition>` field is always treated as "one
/// `LineDefinition` per CLI occurrence, collected via `ArgAction::Append`" —
/// confirmed by a runtime panic ("Mismatch between definition and access of
/// `lines`") the moment `--lines-dir`/`LINES_DIR`/`default_value` actually
/// supplied a value, which nothing did before this field had a default.
/// `parse_lines` instead produces the *entire* vec from a single
/// `--lines-dir` occurrence, so the field type must not look like `Vec<T>`
/// to the derive macro. This newtype (plus `Deref`) sidesteps that:
/// `app.config.lines` still coerces to `&[LineDefinition]` at every existing
/// call site (`crate::routes::samples`, `data::samples::dedup_sample_stations`)
/// with no changes needed there.
#[derive(Debug, Clone, Default)]
pub struct LineCatalogue(pub Vec<LineDefinition>);

impl std::ops::Deref for LineCatalogue {
    type Target = Vec<LineDefinition>;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

#[derive(Debug, clap::Parser)]
pub struct ServiceArguments {
    #[arg(short, long, env, default_value = "0.0.0.0:8080")]
    pub bind_url: String,
    #[arg(short, long, env)]
    pub database_url: String,
    #[arg(long, env)]
    pub redis_url: String,
    /// Shared secret pollers must present via `X-Internal-Token` to reach
    /// `private_router()` endpoints.
    #[arg(long, env)]
    pub internal_token: String,
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
    /// *kind* of secret for this crate -- every other credential here
    /// (`internal_token`, the RDM API keys in sibling pollers) is a single
    /// shared/bearer token, not a paired OAuth2 confidential-client secret
    /// -- but handled with the same posture: env-only, required, never
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
    /// they're done -- the frontend's own root URL (e.g.
    /// `https://rail.example.com/`). One fixed target, not a round-tripped
    /// "return to this page" value -- a v1 scope simplification (see this
    /// plan's Global Constraints).
    #[arg(long, env)]
    pub sso_post_login_redirect_url: String,

    /// Sliding-window session lifetime in days. Design doc proposes 14 as
    /// a starting figure, not researched further there; kept configurable
    /// since it's a product/ops tuning knob, not a protocol constant.
    #[arg(long, env, default_value_t = 14)]
    pub session_ttl_days: i64,
    #[arg(long, value_parser = parse_toml_path::<Defaults>, value_hint = ValueHint::FilePath, value_name = "FILE")]
    pub defaults_file: Option<Defaults>,
    /// Directory of line-catalogue TOML files, loaded once at startup.
    /// Defaults to `/app/lines` (baked into the Docker image — see
    /// `docker/api.Dockerfile`), overridable via `LINES_DIR` for local
    /// (non-Docker) runs.
    #[arg(long = "lines-dir", env = "LINES_DIR", default_value = "/app/lines", value_parser = parse_lines, value_hint = ValueHint::FilePath, value_name = "DIR")]
    pub lines: LineCatalogue,
}
