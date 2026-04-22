//! AniDB HTTP API client.
//! Uses the anime-titles dump for search and the HTTP API for detail.
//! Rate limited to 1 request per 2 seconds per AniDB rules.

use quick_xml::de::from_str;
use serde::Deserialize;
use std::io::Read;
use std::sync::Mutex;
use std::time::{Duration, Instant};
use tracing::{debug, warn};

const TITLES_URL: &str = "https://anidb.net/api/animetitles.xml.gz";
const API_URL: &str = "http://api.anidb.net:9001/httpapi";
const CLIENT_NAME: &str = "orthanc";
const CLIENT_VER: u32 = 1;
const RATE_LIMIT: Duration = Duration::from_secs(2);

pub struct AnidbClient {
    client: reqwest::Client,
    titles: Vec<AnimeTitleEntry>,
    last_request: Mutex<Instant>,
}

// ── Title dump types ──

#[derive(Debug, Clone)]
pub struct AnimeTitleEntry {
    pub aid: u64,
    pub titles: Vec<AnimeTitle>,
}

#[derive(Debug, Clone)]
pub struct AnimeTitle {
    pub name: String,
    pub lang: String,
    pub title_type: String,
}

// XML deserialization types for the title dump
#[derive(Debug, Deserialize)]
struct TitlesDump {
    #[serde(rename = "anime", default)]
    animes: Vec<TitlesDumpAnime>,
}

#[derive(Debug, Deserialize)]
struct TitlesDumpAnime {
    #[serde(rename = "@aid")]
    aid: u64,
    #[serde(rename = "title", default)]
    titles: Vec<TitlesDumpTitle>,
}

#[derive(Debug, Deserialize)]
struct TitlesDumpTitle {
    #[serde(rename = "@type", default)]
    title_type: String,
    #[serde(rename = "@xml:lang", default)]
    lang: String,
    #[serde(rename = "$text", default)]
    name: String,
}

// ── HTTP API detail types ──

#[derive(Debug, Deserialize)]
pub struct AnimeDetail {
    #[serde(rename = "@id")]
    pub id: Option<String>,
    #[serde(rename = "type", default)]
    pub anime_type: Option<String>,
    #[serde(rename = "episodecount", default)]
    pub episodecount: Option<u32>,
    #[serde(rename = "startdate", default)]
    pub startdate: Option<String>,
    #[serde(rename = "enddate", default)]
    pub enddate: Option<String>,
    #[serde(rename = "description", default)]
    pub description: Option<String>,
    #[serde(rename = "picture", default)]
    pub picture: Option<String>,
    #[serde(rename = "ratings", default)]
    pub ratings: Option<AnimeRatings>,
    #[serde(rename = "episodes", default)]
    pub episodes: Option<AnimeEpisodes>,
    #[serde(rename = "tags", default)]
    pub tags: Option<AnimeTags>,
}

#[derive(Debug, Deserialize)]
pub struct AnimeRatings {
    #[serde(rename = "permanent", default)]
    pub permanent: Option<RatingValue>,
}

#[derive(Debug, Deserialize)]
pub struct RatingValue {
    #[serde(rename = "$text", default)]
    pub value: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct AnimeEpisodes {
    #[serde(rename = "episode", default)]
    pub episodes: Vec<AnimeEpisode>,
}

#[derive(Debug, Deserialize)]
pub struct AnimeEpisode {
    #[serde(rename = "@id")]
    pub id: Option<String>,
    #[serde(rename = "epno")]
    pub epno: Option<EpisodeNumber>,
    #[serde(rename = "length", default)]
    pub length: Option<String>,
    #[serde(rename = "airdate", default)]
    pub airdate: Option<String>,
    #[serde(rename = "title", default)]
    pub titles: Vec<EpisodeTitle>,
    #[serde(rename = "summary", default)]
    pub summary: Option<String>,
    #[serde(rename = "rating", default)]
    pub rating: Option<RatingValue>,
}

#[derive(Debug, Deserialize)]
pub struct EpisodeNumber {
    #[serde(rename = "@type")]
    pub eptype: Option<String>,
    #[serde(rename = "$text", default)]
    pub value: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct EpisodeTitle {
    #[serde(rename = "@xml:lang", default)]
    pub lang: String,
    #[serde(rename = "$text", default)]
    pub name: String,
}

#[derive(Debug, Deserialize)]
pub struct AnimeTags {
    #[serde(rename = "tag", default)]
    pub tags: Vec<AnimeTag>,
}

#[derive(Debug, Deserialize)]
pub struct AnimeTag {
    #[serde(rename = "name", default)]
    pub name: Option<String>,
    #[serde(rename = "@weight", default)]
    pub weight: Option<String>,
}

impl AnimeEpisode {
    /// Get the English title, falling back to the first available.
    pub fn english_title(&self) -> Option<&str> {
        self.titles
            .iter()
            .find(|t| t.lang == "en")
            .or_else(|| self.titles.first())
            .map(|t| t.name.as_str())
    }

    /// Parse the episode number (regular episodes are type "1", specials are "2", etc.)
    pub fn regular_episode_number(&self) -> Option<u32> {
        let epno = self.epno.as_ref()?;
        // Type "1" = regular, "2" = special, "3" = credit, etc.
        if epno.eptype.as_deref() != Some("1") {
            return None;
        }
        epno.value.as_ref()?.parse().ok()
    }

    pub fn runtime_seconds(&self) -> Option<i32> {
        self.length.as_ref()?.parse::<i32>().ok().map(|m| m * 60)
    }
}

impl AnidbClient {
    /// Create a new AniDB client. Downloads and parses the title dump for search.
    pub async fn new() -> Result<Self, anyhow::Error> {
        let client = reqwest::Client::builder()
            .user_agent("orthanc/1.0")
            .build()?;

        let titles = download_title_dump(&client).await?;

        Ok(Self {
            client,
            titles,
            last_request: Mutex::new(Instant::now() - RATE_LIMIT),
        })
    }

    /// Enforce the 1-request-per-2-seconds rate limit.
    async fn rate_limit(&self) {
        let wait = {
            let mut last = self.last_request.lock().unwrap();
            let elapsed = last.elapsed();
            if elapsed < RATE_LIMIT {
                let wait = RATE_LIMIT - elapsed;
                *last = Instant::now() + wait;
                Some(wait)
            } else {
                *last = Instant::now();
                None
            }
        };
        if let Some(wait) = wait {
            debug!("AniDB rate limit: waiting {}ms", wait.as_millis());
            tokio::time::sleep(wait).await;
        }
    }

    /// Search for an anime by title. Returns (aid, matched_title) pairs.
    pub fn search(&self, query: &str) -> Vec<(u64, String)> {
        let query_lower = query.to_lowercase();
        let mut results: Vec<(u64, String, usize)> = Vec::new();

        for entry in &self.titles {
            for title in &entry.titles {
                let title_lower = title.name.to_lowercase();
                if title_lower == query_lower {
                    // Exact match — highest priority
                    results.push((entry.aid, title.name.clone(), 0));
                    break;
                } else if title_lower.contains(&query_lower) || query_lower.contains(&title_lower) {
                    results.push((entry.aid, title.name.clone(), 1));
                    break;
                }
            }
        }

        results.sort_by_key(|r| r.2);
        results
            .into_iter()
            .map(|(aid, name, _)| (aid, name))
            .take(10)
            .collect()
    }

    /// Get full anime detail from the HTTP API.
    pub async fn anime_detail(&self, aid: u64) -> Result<AnimeDetail, anyhow::Error> {
        self.rate_limit().await;

        let url = format!(
            "{}?request=anime&client={}&clientver={}&protover=1&aid={}",
            API_URL, CLIENT_NAME, CLIENT_VER, aid
        );
        debug!("AniDB anime detail: {}", aid);

        let resp = self.client.get(&url).send().await?;
        let bytes = resp.bytes().await?;

        // AniDB returns gzip-compressed responses — try to decompress
        let text = if bytes.starts_with(&[0x1f, 0x8b]) {
            let mut decoder = flate2::read::GzDecoder::new(&bytes[..]);
            let mut s = String::new();
            decoder.read_to_string(&mut s)?;
            s
        } else {
            String::from_utf8_lossy(&bytes).to_string()
        };

        // Check for error response
        if text.contains("<error>") {
            return Err(anyhow::anyhow!("AniDB error: {}", text));
        }

        let detail: AnimeDetail = from_str(&text)
            .map_err(|e| anyhow::anyhow!("Failed to parse AniDB XML for aid {}: {}", aid, e))?;

        Ok(detail)
    }

    /// AniDB image URL for poster. `picture` is just the filename.
    pub fn image_url(picture: &str) -> String {
        format!("https://cdn.anidb.net/images/main/{}", picture)
    }

    /// Download an image.
    pub async fn download_image(&self, picture: &str) -> Result<Vec<u8>, anyhow::Error> {
        self.rate_limit().await;
        let url = Self::image_url(picture);
        let bytes = self.client.get(&url).send().await?.bytes().await?;
        Ok(bytes.to_vec())
    }
}

/// Download and parse the AniDB anime-titles dump.
async fn download_title_dump(
    client: &reqwest::Client,
) -> Result<Vec<AnimeTitleEntry>, anyhow::Error> {
    debug!("Downloading AniDB title dump...");
    let resp = client
        .get(TITLES_URL)
        .header("User-Agent", "orthanc/1.0")
        .send()
        .await?;

    let bytes = resp.bytes().await?;

    // Decompress gzip
    let mut decoder = flate2::read::GzDecoder::new(&bytes[..]);
    let mut xml = String::new();
    decoder.read_to_string(&mut xml)?;

    debug!("Parsing AniDB title dump ({} bytes)...", xml.len());

    let dump: TitlesDump =
        from_str(&xml).map_err(|e| anyhow::anyhow!("Failed to parse AniDB titles XML: {}", e))?;

    let entries: Vec<AnimeTitleEntry> = dump
        .animes
        .into_iter()
        .map(|a| AnimeTitleEntry {
            aid: a.aid,
            titles: a
                .titles
                .into_iter()
                .map(|t| AnimeTitle {
                    name: t.name,
                    lang: t.lang,
                    title_type: t.title_type,
                })
                .collect(),
        })
        .collect();

    debug!("Loaded {} anime titles from AniDB dump", entries.len());
    Ok(entries)
}

/// Clean AniDB description text:
/// - Strip `http://anidb.net/...` links, keeping the bracketed display text
///   e.g. `http://anidb.net/cr59303 [Akutami Gege]` → `Akutami Gege`
/// - Replace backticks with apostrophes
/// - Trim "Source: ..." lines from the end
pub fn clean_description(text: &str) -> String {
    use regex::Regex;
    use std::sync::LazyLock;

    static RE_ANIDB_LINK: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"http://anidb\.net/\S+\s*\[([^\]]+)\]").unwrap());
    static RE_BARE_LINK: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"http://anidb\.net/\S+").unwrap());

    let mut s = RE_ANIDB_LINK.replace_all(text, "$1").to_string();
    s = RE_BARE_LINK.replace_all(&s, "").to_string();
    s = s.replace('`', "'");

    // Trim trailing "Source: ..." or "Note: ..." lines
    if let Some(pos) = s.rfind("\nSource:").or_else(|| s.rfind("\nNote:")) {
        s.truncate(pos);
    }

    s.trim().to_string()
}
