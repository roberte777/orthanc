//! TheTVDB v4 API client.
//! API docs: https://thetvdb.github.io/v4-api/
//!
//! Attribution: TheTVDB requires that any product surfacing metadata from this
//! API display a direct link to thetvdb.com. We surface this in the library
//! settings UI next to the TVDB toggle.
//!
//! Rate limiting: TVDB does not publish strict per-second limits, but we keep a
//! minimum spacing between requests and back off on HTTP 429 so a bulk scan
//! doesn't get the embedded subscriber key throttled or banned.

use serde::Deserialize;
use std::sync::Mutex;
use std::time::{Duration, Instant};
use tokio::sync::Mutex as AsyncMutex;
use tracing::{debug, warn};

const BASE_URL: &str = "https://api4.thetvdb.com/v4";
/// Minimum spacing between requests — ~5 req/s, well under any reasonable limit.
const MIN_REQUEST_SPACING: Duration = Duration::from_millis(200);
/// Refresh the JWT well before its ~30-day expiry (matches Jellyfin's 25-day rule).
const TOKEN_REFRESH_AFTER: Duration = Duration::from_secs(25 * 24 * 60 * 60);
/// When we get a 429, wait this long before retrying.
const BACKOFF_ON_429: Duration = Duration::from_secs(5);

pub struct TvdbClient {
    api_key: String,
    client: reqwest::Client,
    /// Cached JWT + when it was issued. Guarded by an async mutex so concurrent
    /// scans don't each kick off a login.
    token: AsyncMutex<Option<CachedToken>>,
    /// Timestamp of the last request for polite rate limiting.
    last_request: Mutex<Instant>,
}

struct CachedToken {
    jwt: String,
    issued: Instant,
}

// ── Response envelope ──

#[derive(Debug, Deserialize)]
struct Envelope<T> {
    data: T,
}

#[derive(Debug, Deserialize)]
struct LoginData {
    token: String,
}

// ── Search ──

#[derive(Debug, Clone, Deserialize)]
pub struct SearchResult {
    #[serde(default)]
    pub tvdb_id: Option<String>,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default, rename = "type")]
    pub result_type: Option<String>,
    #[serde(default)]
    pub year: Option<String>,
    #[serde(default)]
    pub overview: Option<String>,
    #[serde(default)]
    pub image_url: Option<String>,
}

// ── Series ──

#[derive(Debug, Clone, Deserialize)]
pub struct SeriesExtended {
    pub id: u64,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub overview: Option<String>,
    #[serde(default)]
    pub image: Option<String>,
    #[serde(default)]
    pub score: Option<f64>,
    #[serde(default)]
    pub year: Option<String>,
    #[serde(default)]
    pub first_aired: Option<String>,
    #[serde(default)]
    pub genres: Vec<TvdbGenre>,
    #[serde(default)]
    pub artworks: Vec<Artwork>,
    #[serde(default)]
    pub seasons: Vec<SeasonSummary>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct TvdbGenre {
    #[serde(default)]
    pub name: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Artwork {
    #[serde(default)]
    pub image: Option<String>,
    /// TVDB artwork type IDs: 2 = series poster, 3 = series background,
    /// 7 = season poster, 14 = clear logo, etc.
    #[serde(default, rename = "type")]
    pub art_type: Option<u32>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SeasonSummary {
    pub id: u64,
    #[serde(default)]
    pub number: Option<u32>,
    #[serde(default)]
    pub image: Option<String>,
    /// TVDB season order type id: 1 = default/aired, 2 = DVD, 3 = absolute, etc.
    #[serde(default, rename = "type")]
    pub season_type: Option<SeasonType>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SeasonType {
    #[serde(default)]
    pub id: Option<u32>,
    #[serde(default, rename = "type")]
    pub type_name: Option<String>,
}

// ── Episodes ──

#[derive(Debug, Clone, Deserialize)]
pub struct SeriesEpisodesData {
    #[serde(default)]
    pub episodes: Vec<Episode>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Episode {
    pub id: u64,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub overview: Option<String>,
    #[serde(default)]
    pub season_number: Option<u32>,
    #[serde(default)]
    pub number: Option<u32>,
    #[serde(default)]
    pub runtime: Option<u32>,
    #[serde(default)]
    pub aired: Option<String>,
    #[serde(default)]
    pub image: Option<String>,
}

// ── Movies ──

#[derive(Debug, Clone, Deserialize)]
pub struct MovieExtended {
    pub id: u64,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub overview: Option<String>,
    #[serde(default)]
    pub image: Option<String>,
    #[serde(default)]
    pub score: Option<f64>,
    #[serde(default)]
    pub year: Option<String>,
    #[serde(default)]
    pub runtime: Option<u32>,
    #[serde(default)]
    pub genres: Vec<TvdbGenre>,
    #[serde(default)]
    pub artworks: Vec<Artwork>,
}

impl TvdbClient {
    pub fn new(api_key: &str) -> Self {
        Self {
            api_key: api_key.to_string(),
            client: reqwest::Client::builder()
                .user_agent("orthanc/1.0")
                .build()
                .unwrap_or_else(|_| reqwest::Client::new()),
            token: AsyncMutex::new(None),
            last_request: Mutex::new(Instant::now() - MIN_REQUEST_SPACING),
        }
    }

    /// Polite rate limit: ensure at least `MIN_REQUEST_SPACING` between requests.
    async fn rate_limit(&self) {
        let wait = {
            let mut last = self.last_request.lock().unwrap();
            let elapsed = last.elapsed();
            if elapsed < MIN_REQUEST_SPACING {
                let wait = MIN_REQUEST_SPACING - elapsed;
                *last = Instant::now() + wait;
                Some(wait)
            } else {
                *last = Instant::now();
                None
            }
        };
        if let Some(wait) = wait {
            tokio::time::sleep(wait).await;
        }
    }

    /// Get a valid JWT, logging in or refreshing as needed.
    async fn get_token(&self) -> Result<String, anyhow::Error> {
        let mut guard = self.token.lock().await;
        if let Some(ref t) = *guard {
            if t.issued.elapsed() < TOKEN_REFRESH_AFTER {
                return Ok(t.jwt.clone());
            }
        }

        self.rate_limit().await;
        let url = format!("{}/login", BASE_URL);
        debug!("TVDB login");
        let resp = self
            .client
            .post(&url)
            .json(&serde_json::json!({ "apikey": self.api_key }))
            .send()
            .await?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(anyhow::anyhow!("TVDB login failed ({}): {}", status, body));
        }

        let env: Envelope<LoginData> = resp.json().await?;
        let jwt = env.data.token;
        *guard = Some(CachedToken {
            jwt: jwt.clone(),
            issued: Instant::now(),
        });
        Ok(jwt)
    }

    /// GET a JSON envelope; on 401 refresh the token and retry once; on 429 back off once.
    async fn get_json<T: for<'de> Deserialize<'de>>(&self, path: &str) -> Result<T, anyhow::Error> {
        for attempt in 0..2u8 {
            self.rate_limit().await;
            let token = self.get_token().await?;
            let url = format!("{}{}", BASE_URL, path);
            let resp = self
                .client
                .get(&url)
                .bearer_auth(&token)
                .send()
                .await?;

            let status = resp.status();
            if status == reqwest::StatusCode::UNAUTHORIZED && attempt == 0 {
                // Force re-login
                *self.token.lock().await = None;
                continue;
            }
            if status == reqwest::StatusCode::TOO_MANY_REQUESTS && attempt == 0 {
                warn!("TVDB rate limited, backing off {}s", BACKOFF_ON_429.as_secs());
                tokio::time::sleep(BACKOFF_ON_429).await;
                continue;
            }
            if !status.is_success() {
                let body = resp.text().await.unwrap_or_default();
                return Err(anyhow::anyhow!("TVDB {} failed ({}): {}", path, status, body));
            }
            let env: Envelope<T> = resp.json().await?;
            return Ok(env.data);
        }
        Err(anyhow::anyhow!("TVDB request to {} failed after retry", path))
    }

    pub async fn search_series(&self, query: &str) -> Result<Vec<SearchResult>, anyhow::Error> {
        let path = format!("/search?query={}&type=series&limit=10", urlencode(query));
        debug!("TVDB search series: {}", query);
        self.get_json(&path).await
    }

    pub async fn search_movie(&self, query: &str) -> Result<Vec<SearchResult>, anyhow::Error> {
        let path = format!("/search?query={}&type=movie&limit=10", urlencode(query));
        debug!("TVDB search movie: {}", query);
        self.get_json(&path).await
    }

    pub async fn series_extended(&self, id: u64) -> Result<SeriesExtended, anyhow::Error> {
        let path = format!("/series/{}/extended?short=true", id);
        debug!("TVDB series extended: {}", id);
        self.get_json(&path).await
    }

    pub async fn series_episodes(&self, id: u64) -> Result<SeriesEpisodesData, anyhow::Error> {
        // season-type "default" = aired order, the most common for file naming.
        let path = format!("/series/{}/episodes/default?page=0", id);
        debug!("TVDB series episodes: {}", id);
        self.get_json(&path).await
    }

    pub async fn movie_extended(&self, id: u64) -> Result<MovieExtended, anyhow::Error> {
        let path = format!("/movies/{}/extended?short=true", id);
        debug!("TVDB movie extended: {}", id);
        self.get_json(&path).await
    }

    /// Download an artwork URL (TVDB artwork URLs are absolute).
    pub async fn download_image(&self, url: &str) -> Result<Vec<u8>, anyhow::Error> {
        self.rate_limit().await;
        let bytes = self.client.get(url).send().await?.bytes().await?;
        Ok(bytes.to_vec())
    }
}

fn urlencode(s: &str) -> String {
    s.replace(' ', "+")
        .replace('&', "%26")
        .replace('=', "%3D")
        .replace('#', "%23")
        .replace('?', "%3F")
}
