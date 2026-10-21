//! A small Google Cloud Storage JSON API client for the bucket source
//! (`bucket.rs`): list the bucket root, download one generation of an
//! object, and delete one generation conditionally. Over the workspace's
//! `reqwest`, with a bearer token from either credential mode:
//!
//! - **key** (`GOOGLE_SERVICE_ACCOUNT_PATH`): the reader service account's
//!   JSON key signs a self-signed JWT (RS256, `scope` claim, no token
//!   endpoint), exactly as `object_store`'s GCS client does
//!   (`SelfSignedJwt`). The key file is re-read on every refresh, so a
//!   rotated Secret is picked up without a restart; a missing or invalid
//!   one is revoked access.
//! - **keyless** (`GOOGLE_APPLICATION_CREDENTIALS`): an `external_account`
//!   configuration exchanged at Google STS and IAM Credentials by
//!   `common::gcp_external_account::ExternalAccountTokenSource`. A
//!   configuration that cannot be loaded is revoked access, re-read on each
//!   retry.
//!
//! A 401 means the token itself was refused: it is invalidated and the call
//! retried once; a second 401 is revoked access. A 403 is IAM refusing a
//! valid token (the kill switch removed the binding): revoked access, no
//! refresh. See the GCS spec §9, "Keyless reader credentials".
//!
//! No token, key or file contents ever reach a log line.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use base64::Engine as _;
use base64::engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD};
use chrono::{DateTime, Utc};
use common::gcp_external_account::{
    ExternalAccountConfig, ExternalAccountTokenSource, TokenSourceSettings,
};
use common::secret::Secret;
use serde::Deserialize;

/// The public GCS endpoint (`BUCKET_BASE_URL` empty).
pub(crate) const DEFAULT_BASE_URL: &str = "https://storage.googleapis.com";
/// The OAuth scope of the reader's token.
const SCOPE: &str = "https://www.googleapis.com/auth/cloud-platform";
/// A self-signed JWT's lifetime, and when it is renewed (five minutes
/// early).
const JWT_LIFETIME: Duration = Duration::from_secs(3600);
const JWT_RENEW_AFTER: Duration = Duration::from_secs(3300);
/// Timeouts of every call. A download gets far longer overall, but must
/// keep receiving bytes (`READ_TIMEOUT`).
const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
const READ_TIMEOUT: Duration = Duration::from_secs(60);
const CALL_TIMEOUT: Duration = Duration::from_secs(60);
const DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(30 * 60);
/// Most pages one listing may take (1000 objects a page; the bucket holds
/// a handful), so a misbehaving server cannot loop us.
const MAX_LIST_PAGES: usize = 50;

/// Why a call failed.
#[derive(Debug)]
pub(crate) enum GcsError {
    /// Access is refused: 403, a second 401, or no usable credential.
    /// Never retried before `BUCKET_MAX_BACKOFF_SECS`.
    Revoked(String),
    /// The credential could not be minted for a transient reason (a token
    /// endpoint timed out): `errors_total{kind="auth"}`.
    Auth(String),
    /// Anything else: an HTTP status, a transport error, a malformed body.
    Failed(String),
}

impl std::fmt::Display for GcsError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Revoked(why) => write!(f, "bucket access revoked: {why}"),
            Self::Auth(why) => write!(f, "no bucket credential: {why}"),
            Self::Failed(why) => f.write_str(why),
        }
    }
}

impl std::error::Error for GcsError {}

/// One object at the bucket root, from a listing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ObjectMeta {
    pub name: String,
    pub size: u64,
    pub generation: i64,
    /// Big-endian CRC32C, decoded from GCS's base64.
    pub crc32c: Option<u32>,
    pub md5: Option<[u8; 16]>,
    pub time_created: DateTime<Utc>,
}

/// What a conditional delete did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Deleted {
    /// Deleted that generation.
    Yes,
    /// 404: already gone.
    Gone,
    /// 412: a newer generation replaced it; it stays.
    Newer,
}

/// A service-account JSON key's fields the JWT needs. No `Debug`: it holds
/// the private key.
#[derive(Deserialize)]
struct ServiceAccountKey {
    #[serde(rename = "type")]
    kind: String,
    client_email: String,
    private_key: String,
    private_key_id: String,
}

/// Where tokens come from.
pub(crate) enum Credentials {
    /// A JSON key, re-read on every refresh.
    Key {
        path: PathBuf,
        cached: std::sync::Mutex<Option<(Secret, Instant)>>,
    },
    /// An `external_account` configuration, loaded lazily (and again after
    /// a failure).
    ExternalAccount {
        path: PathBuf,
        source: tokio::sync::Mutex<Option<Arc<ExternalAccountTokenSource>>>,
        /// Tests: skip `check_google_endpoints` (a fake STS on localhost).
        allow_any_endpoint: bool,
    },
    /// A fixed token (tests).
    #[cfg(test)]
    Static(Secret),
}

impl std::fmt::Debug for Credentials {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Key { path, .. } => f.debug_struct("Key").field("path", path).finish(),
            Self::ExternalAccount { path, .. } => f
                .debug_struct("ExternalAccount")
                .field("path", path)
                .finish(),
            #[cfg(test)]
            Self::Static(_) => f.write_str("Static"),
        }
    }
}

impl Credentials {
    pub(crate) fn key(path: PathBuf) -> Self {
        Self::Key {
            path,
            cached: std::sync::Mutex::new(None),
        }
    }

    pub(crate) fn external_account(path: PathBuf) -> Self {
        Self::ExternalAccount {
            path,
            source: tokio::sync::Mutex::new(None),
            allow_any_endpoint: false,
        }
    }

    /// The token for the next call.
    async fn token(&self) -> Result<Secret, GcsError> {
        match self {
            Self::Key { path, cached } => {
                let now = Instant::now();
                {
                    let cached = cached
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner);
                    if let Some((token, renew_at)) = cached.as_ref()
                        && now < *renew_at
                    {
                        return Ok(token.clone());
                    }
                }
                let token = self_signed_jwt(path, SystemTime::now())?;
                *cached
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner) =
                    Some((token.clone(), now + JWT_RENEW_AFTER));
                Ok(token)
            }
            Self::ExternalAccount {
                path,
                source,
                allow_any_endpoint,
            } => {
                let source = {
                    let mut slot = source.lock().await;
                    if let Some(source) = slot.as_ref() {
                        Arc::clone(source)
                    } else {
                        let loaded = Arc::new(load_external_account(path, *allow_any_endpoint)?);
                        *slot = Some(Arc::clone(&loaded));
                        loaded
                    }
                };
                source.access_token().await.map_err(|err| {
                    if err.is_access_revoked() {
                        GcsError::Revoked(err.to_string())
                    } else {
                        GcsError::Auth(err.to_string())
                    }
                })
            }
            #[cfg(test)]
            Self::Static(token) => Ok(token.clone()),
        }
    }

    /// GCS answered 401 to `token`: forget it.
    async fn invalidate(&self, token: &Secret) {
        match self {
            Self::Key { cached, .. } => {
                let mut cached = cached
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                if cached
                    .as_ref()
                    .is_some_and(|(t, _)| t.expose() == token.expose())
                {
                    *cached = None;
                }
            }
            Self::ExternalAccount { source, .. } => {
                if let Some(source) = source.lock().await.as_ref() {
                    source.invalidate(token);
                }
            }
            #[cfg(test)]
            Self::Static(_) => {}
        }
    }
}

fn load_external_account(
    path: &Path,
    allow_any_endpoint: bool,
) -> Result<ExternalAccountTokenSource, GcsError> {
    let config = ExternalAccountConfig::from_file(path).map_err(|err| {
        GcsError::Revoked(format!(
            "the external_account configuration at {} could not be loaded: {err:#}",
            path.display()
        ))
    })?;
    if !allow_any_endpoint {
        config.check_google_endpoints().map_err(|err| {
            GcsError::Revoked(format!(
                "the external_account configuration names a non-Google endpoint: {err:#}"
            ))
        })?;
    }
    ExternalAccountTokenSource::new(config, TokenSourceSettings::default())
        .map_err(|err| GcsError::Auth(format!("{err:#}")))
}

/// A self-signed JWT from the key at `path`, issued at `now`. Any problem
/// with the file is revoked access; its contents are never logged.
fn self_signed_jwt(path: &Path, now: SystemTime) -> Result<Secret, GcsError> {
    let revoked = |why: &str| GcsError::Revoked(format!("reader key {}: {why}", path.display()));
    let raw =
        std::fs::read(path).map_err(|err| revoked(&format!("unreadable ({})", err.kind())))?;
    let key: ServiceAccountKey =
        serde_json::from_slice(&raw).map_err(|_| revoked("not a service-account JSON key"))?;
    if key.kind != "service_account" {
        return Err(revoked("not a service_account key"));
    }
    let der = pem_body(&key.private_key).ok_or_else(|| revoked("private_key is not PEM"))?;
    let pair = aws_lc_rs::signature::RsaKeyPair::from_pkcs8(&der)
        .map_err(|_| revoked("private_key is not a PKCS#8 RSA key"))?;
    let iat = now.duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs());
    let header = serde_json::json!({"alg": "RS256", "typ": "JWT", "kid": key.private_key_id});
    let claims = serde_json::json!({
        "iss": key.client_email,
        "sub": key.client_email,
        "scope": SCOPE,
        "iat": iat,
        "exp": iat + JWT_LIFETIME.as_secs(),
    });
    let message = format!(
        "{}.{}",
        URL_SAFE_NO_PAD.encode(header.to_string()),
        URL_SAFE_NO_PAD.encode(claims.to_string())
    );
    let mut signature = vec![0; pair.public_modulus_len()];
    pair.sign(
        &aws_lc_rs::signature::RSA_PKCS1_SHA256,
        &aws_lc_rs::rand::SystemRandom::new(),
        message.as_bytes(),
        &mut signature,
    )
    .map_err(|_| revoked("signing failed"))?;
    Ok(Secret::new(format!(
        "{message}.{}",
        URL_SAFE_NO_PAD.encode(signature)
    )))
}

/// The DER inside a `-----BEGIN PRIVATE KEY-----` PEM block.
fn pem_body(pem: &str) -> Option<Vec<u8>> {
    let start = pem.find("-----BEGIN PRIVATE KEY-----")? + "-----BEGIN PRIVATE KEY-----".len();
    let end = pem[start..].find("-----END PRIVATE KEY-----")? + start;
    let body: String = pem[start..end]
        .chars()
        .filter(|c| !c.is_whitespace())
        .collect();
    STANDARD.decode(body).ok()
}

/// Percent-encodes `name` as one path segment (everything but RFC 3986
/// unreserved characters), as the JSON API requires for object names.
fn encode_segment(name: &str) -> String {
    use std::fmt::Write as _;
    name.bytes()
        .fold(String::with_capacity(name.len()), |mut out, b| {
            if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.' | b'_' | b'~') {
                out.push(char::from(b));
            } else {
                let _ = write!(out, "%{b:02X}");
            }
            out
        })
}

/// The JSON API's object resource, as listed.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawObject {
    name: String,
    size: String,
    generation: String,
    #[serde(default)]
    crc32c: Option<String>,
    #[serde(default)]
    md5_hash: Option<String>,
    time_created: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ListPage {
    #[serde(default)]
    items: Vec<RawObject>,
    #[serde(default)]
    prefixes: Vec<String>,
    #[serde(default)]
    next_page_token: Option<String>,
}

/// One listing: the objects, and with a delimiter the "folders" below the
/// prefix (`prefixes`).
#[derive(Debug, Default)]
pub(crate) struct Listing {
    pub objects: Vec<ObjectMeta>,
    pub prefixes: Vec<String>,
}

impl RawObject {
    fn parse(self) -> Result<ObjectMeta, String> {
        let crc32c = self
            .crc32c
            .map(|b64| {
                let bytes = STANDARD.decode(b64).map_err(|_| "crc32c is not base64")?;
                let bytes: [u8; 4] = bytes.try_into().map_err(|_| "crc32c is not 4 bytes")?;
                Ok::<u32, &str>(u32::from_be_bytes(bytes))
            })
            .transpose()?;
        let md5 = self
            .md5_hash
            .map(|b64| {
                let bytes = STANDARD.decode(b64).map_err(|_| "md5Hash is not base64")?;
                bytes.try_into().map_err(|_| "md5Hash is not 16 bytes")
            })
            .transpose()?;
        Ok(ObjectMeta {
            size: self.size.parse().map_err(|_| "size is not a number")?,
            generation: self
                .generation
                .parse()
                .map_err(|_| "generation is not a number")?,
            crc32c,
            md5,
            time_created: DateTime::parse_from_rfc3339(&self.time_created)
                .map_err(|_| "timeCreated is not RFC 3339")?
                .with_timezone(&Utc),
            name: self.name,
        })
    }
}

/// The bucket, through the JSON API.
#[derive(Debug)]
pub(crate) struct GcsClient {
    http: reqwest::Client,
    base: String,
    bucket: String,
    credentials: Credentials,
}

impl GcsClient {
    pub(crate) fn new(
        base_url: Option<&str>,
        bucket: &str,
        credentials: Credentials,
    ) -> anyhow::Result<Self> {
        let http = reqwest::Client::builder()
            .connect_timeout(CONNECT_TIMEOUT)
            .read_timeout(READ_TIMEOUT)
            .user_agent(concat!(
                "distant-signal-schedule-ingest/",
                env!("CARGO_PKG_VERSION")
            ))
            .build()?;
        let base = base_url
            .filter(|url| !url.is_empty())
            .unwrap_or(DEFAULT_BASE_URL)
            .trim_end_matches('/')
            .to_string();
        Ok(Self {
            http,
            base,
            bucket: bucket.to_string(),
            credentials,
        })
    }

    pub(crate) fn bucket(&self) -> &str {
        &self.bucket
    }

    fn object_url(&self, name: &str) -> String {
        format!(
            "{}/storage/v1/b/{}/o/{}",
            self.base,
            encode_segment(&self.bucket),
            encode_segment(name)
        )
    }

    /// Sends the request `build` makes with a bearer token: a 401 is
    /// retried once with a fresh token, a second 401 or a 403 is
    /// [`GcsError::Revoked`]. Any other status is returned for the caller.
    async fn send(
        &self,
        what: &str,
        build: impl Fn(&reqwest::Client) -> reqwest::RequestBuilder,
    ) -> Result<reqwest::Response, GcsError> {
        for attempt in 0..2 {
            let token = self.credentials.token().await?;
            let response = build(&self.http)
                .bearer_auth(token.expose())
                .send()
                .await
                .map_err(|err| GcsError::Failed(format!("{what}: {}", without_url(&err))))?;
            match response.status().as_u16() {
                401 if attempt == 0 => {
                    tracing::info!(
                        call = what,
                        "GCS refused the access token (401); renewing it and retrying once"
                    );
                    self.credentials.invalidate(&token).await;
                }
                401 => {
                    return Err(GcsError::Revoked(format!(
                        "{what}: 401 after a fresh token"
                    )));
                }
                403 => {
                    return Err(GcsError::Revoked(format!(
                        "{what}: 403 (IAM refused the reader)"
                    )));
                }
                _ => return Ok(response),
            }
        }
        Err(GcsError::Revoked(format!("{what}: 401")))
    }

    /// Every object at the bucket root (`delimiter=/`, so nested names are
    /// never seen), following `nextPageToken`.
    pub(crate) async fn list_root(&self) -> Result<Vec<ObjectMeta>, GcsError> {
        Ok(self.list("", true, None).await?.objects)
    }

    /// Every object under `prefix` (only one level, and the folders below
    /// it as `prefixes`, with `delimiter`), from `start_offset` on (names
    /// sorting at or after it), following `nextPageToken`.
    pub(crate) async fn list(
        &self,
        prefix: &str,
        delimiter: bool,
        start_offset: Option<&str>,
    ) -> Result<Listing, GcsError> {
        let url = format!(
            "{}/storage/v1/b/{}/o",
            self.base,
            encode_segment(&self.bucket)
        );
        let mut listing = Listing::default();
        let mut page_token: Option<String> = None;
        for _ in 0..MAX_LIST_PAGES {
            let token = page_token.clone();
            let response = self
                .send("list", |http| {
                    let mut request = http.get(&url).timeout(CALL_TIMEOUT).query(&[
                        ("prefix", prefix),
                        (
                            "fields",
                            "items(name,size,generation,crc32c,md5Hash,timeCreated),prefixes,nextPageToken",
                        ),
                    ]);
                    if delimiter {
                        request = request.query(&[("delimiter", "/")]);
                    }
                    if let Some(start) = start_offset {
                        request = request.query(&[("startOffset", start)]);
                    }
                    if let Some(token) = &token {
                        request = request.query(&[("pageToken", token)]);
                    }
                    request
                })
                .await?;
            let status = response.status();
            if !status.is_success() {
                return Err(GcsError::Failed(format!("list: HTTP {status}")));
            }
            let page: ListPage = response
                .json()
                .await
                .map_err(|err| GcsError::Failed(format!("list: {}", without_url(&err))))?;
            for raw in page.items {
                listing.objects.push(
                    raw.parse()
                        .map_err(|why| GcsError::Failed(format!("list: {why}")))?,
                );
            }
            listing.prefixes.extend(page.prefixes);
            match page.next_page_token.filter(|t| !t.is_empty()) {
                Some(next) => page_token = Some(next),
                None => return Ok(listing),
            }
        }
        Err(GcsError::Failed(format!(
            "list: more than {MAX_LIST_PAGES} pages"
        )))
    }

    /// Starts downloading exactly `object`'s generation. The caller streams
    /// the body (`Response::chunk`).
    pub(crate) async fn download(
        &self,
        object: &ObjectMeta,
    ) -> Result<reqwest::Response, GcsError> {
        let url = self.object_url(&object.name);
        let generation = object.generation.to_string();
        let response = self
            .send("get", |http| {
                http.get(&url)
                    .timeout(DOWNLOAD_TIMEOUT)
                    .query(&[("alt", "media"), ("generation", generation.as_str())])
            })
            .await?;
        let status = response.status();
        if status.is_success() {
            Ok(response)
        } else {
            Err(GcsError::Failed(format!("get: HTTP {status}")))
        }
    }

    /// Deletes `generation` of `name`, only if it is still the live one
    /// (`ifGenerationMatch`).
    pub(crate) async fn delete(&self, name: &str, generation: i64) -> Result<Deleted, GcsError> {
        let url = self.object_url(name);
        let generation = generation.to_string();
        let response = self
            .send("delete", |http| {
                http.delete(&url)
                    .timeout(CALL_TIMEOUT)
                    .query(&[("ifGenerationMatch", generation.as_str())])
            })
            .await?;
        match response.status().as_u16() {
            200..=299 => Ok(Deleted::Yes),
            404 => Ok(Deleted::Gone),
            412 => Ok(Deleted::Newer),
            status => Err(GcsError::Failed(format!("delete: HTTP {status}"))),
        }
    }
}

/// A reqwest error's text without its URL (the object name in it is
/// attacker-controlled; it is logged separately, escaped).
fn without_url(err: &reqwest::Error) -> String {
    let mut text = err.to_string();
    if let Some(url) = err.url() {
        text = text.replace(url.as_str(), "<url>");
    }
    if err.is_timeout() {
        text.push_str(" (timeout)");
    }
    text
}

#[cfg(test)]
pub(crate) mod tests {
    use aws_lc_rs::encoding::AsDer as _;
    use wiremock::matchers::{header, method, path, query_param};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    use super::*;

    /// A throwaway RSA key, generated now, as a service-account JSON key.
    /// Never a real key.
    pub(crate) fn throwaway_key_json() -> (String, aws_lc_rs::rsa::KeyPair) {
        let pair = aws_lc_rs::rsa::KeyPair::generate(aws_lc_rs::rsa::KeySize::Rsa2048).unwrap();
        let der: aws_lc_rs::encoding::Pkcs8V1Der<'_> = pair.as_der().unwrap();
        let body = STANDARD.encode(der.as_ref());
        let mut pem = String::from("-----BEGIN PRIVATE KEY-----\n");
        for chunk in body.as_bytes().chunks(64) {
            pem.push_str(std::str::from_utf8(chunk).unwrap());
            pem.push('\n');
        }
        pem.push_str("-----END PRIVATE KEY-----\n");
        let json = serde_json::json!({
            "type": "service_account",
            "project_id": "example-project",
            "private_key_id": "0123456789abcdef",
            "private_key": pem,
            "client_email": "reader@example-project.iam.gserviceaccount.com",
        })
        .to_string();
        (json, pair)
    }

    pub(crate) fn object_json(name: &str, contents: &[u8], generation: i64) -> serde_json::Value {
        let crc = crc::Crc::<u32>::new(&crc::CRC_32_ISCSI).checksum(contents);
        let md5 = <md5::Md5 as md5::Digest>::digest(contents);
        serde_json::json!({
            "name": name,
            "size": contents.len().to_string(),
            "generation": generation.to_string(),
            "crc32c": STANDARD.encode(crc.to_be_bytes()),
            "md5Hash": STANDARD.encode(md5),
            "timeCreated": "2026-10-20T20:01:02.345Z",
        })
    }

    fn client(server: &MockServer) -> GcsClient {
        GcsClient::new(
            Some(&server.uri()),
            "example-ds-ingest",
            Credentials::Static(Secret::new("tok-1")),
        )
        .unwrap()
    }

    #[test]
    fn a_self_signed_jwt_is_rs256_with_the_keys_identity_and_verifies() {
        let (json, pair) = throwaway_key_json();
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("service-account.json");
        std::fs::write(&path, json).unwrap();
        let now = UNIX_EPOCH + Duration::from_secs(1_790_000_000);
        let jwt = self_signed_jwt(&path, now).unwrap();
        let parts: Vec<&str> = jwt.expose().split('.').collect();
        assert_eq!(parts.len(), 3);
        let header: serde_json::Value =
            serde_json::from_slice(&URL_SAFE_NO_PAD.decode(parts[0]).unwrap()).unwrap();
        assert_eq!(header["alg"], "RS256");
        assert_eq!(header["kid"], "0123456789abcdef");
        let claims: serde_json::Value =
            serde_json::from_slice(&URL_SAFE_NO_PAD.decode(parts[1]).unwrap()).unwrap();
        assert_eq!(
            claims["iss"],
            "reader@example-project.iam.gserviceaccount.com"
        );
        assert_eq!(claims["sub"], claims["iss"]);
        assert_eq!(claims["scope"], SCOPE);
        assert_eq!(claims["iat"], 1_790_000_000);
        assert_eq!(claims["exp"], 1_790_003_600);
        let public = aws_lc_rs::signature::KeyPair::public_key(&pair);
        aws_lc_rs::signature::UnparsedPublicKey::new(
            &aws_lc_rs::signature::RSA_PKCS1_2048_8192_SHA256,
            public.as_ref(),
        )
        .verify(
            format!("{}.{}", parts[0], parts[1]).as_bytes(),
            &URL_SAFE_NO_PAD.decode(parts[2]).unwrap(),
        )
        .unwrap();
    }

    #[test]
    fn a_missing_or_bad_key_is_revoked_access_and_never_echoed() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("service-account.json");
        let err = self_signed_jwt(&path, SystemTime::now()).unwrap_err();
        assert!(matches!(err, GcsError::Revoked(_)), "{err}");
        std::fs::write(&path, r#"{"type":"service_account","client_email":"a","private_key":"SECRETSTUFF","private_key_id":"k"}"#).unwrap();
        let err = self_signed_jwt(&path, SystemTime::now()).unwrap_err();
        assert!(matches!(err, GcsError::Revoked(_)));
        assert!(!err.to_string().contains("SECRETSTUFF"), "{err}");
    }

    #[test]
    fn object_names_are_encoded_as_one_segment() {
        assert_eq!(encode_segment("timetable_full.zip"), "timetable_full.zip");
        assert_eq!(encode_segment("a b/c?d#é"), "a%20b%2Fc%3Fd%23%C3%A9");
    }

    #[tokio::test]
    async fn listing_follows_pages_with_the_bearer_token_at_the_root() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/storage/v1/b/example-ds-ingest/o"))
            .and(query_param("delimiter", "/"))
            .and(query_param("pageToken", "p2"))
            .and(header("authorization", "Bearer tok-1"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "items": [object_json("CORPUSExtract.json.gz", b"xyz", 7)]
            })))
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/storage/v1/b/example-ds-ingest/o"))
            .and(query_param("delimiter", "/"))
            .and(header("authorization", "Bearer tok-1"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "items": [object_json("timetable_full.zip", b"abc", 5)],
                "prefixes": ["nested/"],
                "nextPageToken": "p2"
            })))
            .expect(1)
            .mount(&server)
            .await;
        let objects = client(&server).list_root().await.unwrap();
        let names: Vec<&str> = objects.iter().map(|o| o.name.as_str()).collect();
        assert_eq!(names, ["timetable_full.zip", "CORPUSExtract.json.gz"]);
        assert_eq!(objects[0].size, 3);
        assert_eq!(objects[0].generation, 5);
        assert_eq!(
            objects[0].crc32c,
            Some(crc::Crc::<u32>::new(&crc::CRC_32_ISCSI).checksum(b"abc"))
        );
        assert!(objects[0].md5.is_some());
    }

    #[tokio::test]
    async fn a_403_is_revoked_without_a_retry_and_a_401_retries_once() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(403))
            .expect(1)
            .mount(&server)
            .await;
        let err = client(&server).list_root().await.unwrap_err();
        assert!(matches!(err, GcsError::Revoked(_)), "{err}");
        server.verify().await;

        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(401))
            .expect(2)
            .mount(&server)
            .await;
        let err = client(&server).list_root().await.unwrap_err();
        assert!(matches!(err, GcsError::Revoked(_)), "{err}");
    }

    #[tokio::test]
    async fn a_key_401_renews_the_jwt_once_and_succeeds() {
        let (json, _) = throwaway_key_json();
        let dir = tempfile::tempdir().unwrap();
        let key = dir.path().join("service-account.json");
        std::fs::write(&key, json).unwrap();
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(401))
            .up_to_n_times(1)
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({})))
            .mount(&server)
            .await;
        let client = GcsClient::new(Some(&server.uri()), "b-1x", Credentials::key(key)).unwrap();
        assert!(client.list_root().await.unwrap().is_empty());
        let requests = server.received_requests().await.unwrap();
        assert_eq!(requests.len(), 2);
        let bearer = |i: usize| {
            requests[i]
                .headers
                .get("authorization")
                .unwrap()
                .to_str()
                .unwrap()
                .to_string()
        };
        assert!(bearer(1).starts_with("Bearer ey"), "a JWT");
    }

    #[tokio::test]
    async fn download_pins_the_generation_and_delete_maps_its_statuses() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/storage/v1/b/example-ds-ingest/o/timetable_full.zip"))
            .and(query_param("alt", "media"))
            .and(query_param("generation", "5"))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(b"abc".to_vec()))
            .mount(&server)
            .await;
        for (generation, status) in [("5", 204), ("6", 404), ("7", 412), ("8", 500)] {
            Mock::given(method("DELETE"))
                .and(path("/storage/v1/b/example-ds-ingest/o/timetable_full.zip"))
                .and(query_param("ifGenerationMatch", generation))
                .respond_with(ResponseTemplate::new(status))
                .mount(&server)
                .await;
        }
        let client = client(&server);
        let object = RawObject {
            name: "timetable_full.zip".into(),
            size: "3".into(),
            generation: "5".into(),
            crc32c: None,
            md5_hash: None,
            time_created: "2026-10-20T20:01:02Z".into(),
        }
        .parse()
        .unwrap();
        let body = client
            .download(&object)
            .await
            .unwrap()
            .bytes()
            .await
            .unwrap();
        assert_eq!(&body[..], b"abc");
        assert_eq!(
            client.delete("timetable_full.zip", 5).await.unwrap(),
            Deleted::Yes
        );
        assert_eq!(
            client.delete("timetable_full.zip", 6).await.unwrap(),
            Deleted::Gone
        );
        assert_eq!(
            client.delete("timetable_full.zip", 7).await.unwrap(),
            Deleted::Newer
        );
        assert!(matches!(
            client.delete("timetable_full.zip", 8).await.unwrap_err(),
            GcsError::Failed(_)
        ));
    }

    /// Keyless: the token comes from fake STS and IAM Credentials; a
    /// missing configuration is revoked access.
    #[tokio::test]
    async fn keyless_exchanges_the_projected_token_for_the_bearer() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/token"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "access_token": "federated",
                "issued_token_type": "urn:ietf:params:oauth:token-type:access_token",
                "token_type": "Bearer",
                "expires_in": 3600
            })))
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/v1/projects/-/serviceAccounts/reader@example-project.iam.gserviceaccount.com:generateAccessToken"))
            .and(header("authorization", "Bearer federated"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "accessToken": "reader-access",
                "expireTime": (Utc::now() + chrono::Duration::hours(1)).to_rfc3339()
            })))
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/storage/v1/b/example-ds-ingest/o"))
            .and(header("authorization", "Bearer reader-access"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({})))
            .expect(2)
            .mount(&server)
            .await;
        let dir = tempfile::tempdir().unwrap();
        let token_file = dir.path().join("token");
        std::fs::write(&token_file, "k8s.projected.jwt").unwrap();
        let config = dir.path().join("credential-config.json");
        let credentials = Credentials::ExternalAccount {
            path: config.clone(),
            source: tokio::sync::Mutex::new(None),
            allow_any_endpoint: true,
        };
        let client = GcsClient::new(Some(&server.uri()), "example-ds-ingest", credentials).unwrap();
        // No configuration yet: revoked, and nothing is called.
        assert!(matches!(
            client.list_root().await.unwrap_err(),
            GcsError::Revoked(_)
        ));
        std::fs::write(
            &config,
            serde_json::json!({
                "type": "external_account",
                "audience": "//iam.googleapis.com/projects/1/locations/global/workloadIdentityPools/p/providers/k",
                "subject_token_type": "urn:ietf:params:oauth:token-type:jwt",
                "token_url": format!("{}/v1/token", server.uri()),
                "service_account_impersonation_url": format!("{}/v1/projects/-/serviceAccounts/reader@example-project.iam.gserviceaccount.com:generateAccessToken", server.uri()),
                "credential_source": { "file": token_file, "format": { "type": "text" } }
            })
            .to_string(),
        )
        .unwrap();
        // Recovers without a restart, and the token is cached.
        client.list_root().await.unwrap();
        client.list_root().await.unwrap();
    }

    #[test]
    fn production_keyless_refuses_a_non_google_token_endpoint() {
        let dir = tempfile::tempdir().unwrap();
        let config = dir.path().join("credential-config.json");
        std::fs::write(
            &config,
            serde_json::json!({
                "type": "external_account",
                "audience": "//iam.googleapis.com/x",
                "subject_token_type": "urn:ietf:params:oauth:token-type:jwt",
                "token_url": "https://evil.example.com/v1/token",
                "credential_source": { "file": "/nonexistent" }
            })
            .to_string(),
        )
        .unwrap();
        assert!(matches!(
            load_external_account(&config, false).unwrap_err(),
            GcsError::Revoked(_)
        ));
    }
}
