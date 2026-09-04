//! Thin HTTP client wrappers -- deliberately separate from correlation
//! logic, same reasoning as trust-consumer's own queries.rs module doc:
//! keeps correlation logic unit-testable without a live api.
//!
//! Not yet wired into `main.rs`'s loop (that's Task 13) -- `#![allow(dead_code)]`
//! here is temporary, same posture as `config::Config::shadow_line_ids`.
#![allow(dead_code)]

pub async fn fetch_line_population(
    client: &reqwest::Client,
    url: &str,
    tokens: &common::oauth_client::OAuthTokenCache,
    line_id: &str,
    service_date: chrono::NaiveDate,
) -> anyhow::Result<Option<serde_json::Value>> {
    let token = tokens.get_token(client).await?;
    let response = client
        .get(url)
        .query(&[
            ("line_id", line_id),
            ("service_date", &service_date.to_string()),
        ])
        .bearer_auth(&token)
        .send()
        .await?
        .error_for_status()?;
    Ok(response.json().await?)
}

pub async fn fetch_stanox_crs(
    client: &reqwest::Client,
    url: &str,
    tokens: &common::oauth_client::OAuthTokenCache,
) -> anyhow::Result<Vec<common::StanoxCrsRecord>> {
    // Identical to trust_consumer::queries::fetch_stanox_crs -- not
    // extracted into trust-schema (Task 1's scope was parsing/dedup/
    // journey only, deliberately not HTTP client code, which has no
    // shared logic beyond "GET + bearer + deserialize," already trivial).
    let token = tokens.get_token(client).await?;
    let response = client
        .get(url)
        .bearer_auth(&token)
        .send()
        .await?
        .error_for_status()?;
    Ok(response.json().await?)
}
