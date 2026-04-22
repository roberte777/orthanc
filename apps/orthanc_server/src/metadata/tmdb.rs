//! TMDB (The Movie Database) API client.
//! API docs: https://developer.themoviedb.org/reference

use serde::Deserialize;
use tracing::debug;

const BASE_URL: &str = "https://api.themoviedb.org/3";
const IMAGE_BASE: &str = "https://image.tmdb.org/t/p";

pub struct TmdbClient {
    api_key: String,
    client: reqwest::Client,
}

// ── Response types ──

#[derive(Debug, Deserialize)]
pub struct SearchResults<T> {
    pub results: Vec<T>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct MovieSearchResult {
    pub id: u64,
    pub title: String,
    pub original_title: Option<String>,
    pub overview: Option<String>,
    pub release_date: Option<String>,
    pub poster_path: Option<String>,
    pub backdrop_path: Option<String>,
    pub vote_average: Option<f64>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct TvSearchResult {
    pub id: u64,
    pub name: String,
    pub original_name: Option<String>,
    pub overview: Option<String>,
    pub first_air_date: Option<String>,
    pub poster_path: Option<String>,
    pub backdrop_path: Option<String>,
    pub vote_average: Option<f64>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct MovieDetail {
    pub id: u64,
    pub title: String,
    pub original_title: Option<String>,
    pub overview: Option<String>,
    pub release_date: Option<String>,
    pub runtime: Option<u32>,
    pub poster_path: Option<String>,
    pub backdrop_path: Option<String>,
    pub vote_average: Option<f64>,
    pub tagline: Option<String>,
    pub imdb_id: Option<String>,
    pub genres: Vec<Genre>,
    pub credits: Option<Credits>,
    pub content_ratings: Option<ContentRatings>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct TvDetail {
    pub id: u64,
    pub name: String,
    pub original_name: Option<String>,
    pub overview: Option<String>,
    pub first_air_date: Option<String>,
    pub poster_path: Option<String>,
    pub backdrop_path: Option<String>,
    pub vote_average: Option<f64>,
    pub tagline: Option<String>,
    pub genres: Vec<Genre>,
    pub seasons: Vec<TvSeasonSummary>,
    pub credits: Option<Credits>,
    pub content_ratings: Option<ContentRatings>,
    pub external_ids: Option<ExternalIds>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct TvSeasonSummary {
    pub id: u64,
    pub season_number: u32,
    pub name: String,
    pub overview: Option<String>,
    pub poster_path: Option<String>,
    pub episode_count: Option<u32>,
    pub air_date: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct TvSeasonDetail {
    pub id: u64,
    pub season_number: u32,
    pub name: String,
    pub overview: Option<String>,
    pub poster_path: Option<String>,
    pub episodes: Vec<TvEpisode>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct TvEpisode {
    pub id: u64,
    pub episode_number: u32,
    pub name: String,
    pub overview: Option<String>,
    pub air_date: Option<String>,
    pub runtime: Option<u32>,
    pub still_path: Option<String>,
    pub vote_average: Option<f64>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Genre {
    pub id: u64,
    pub name: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Credits {
    pub cast: Vec<CastMember>,
    pub crew: Vec<CrewMember>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct CastMember {
    pub id: u64,
    pub name: String,
    pub character: Option<String>,
    pub order: Option<u32>,
    pub profile_path: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct CrewMember {
    pub id: u64,
    pub name: String,
    pub job: String,
    pub department: Option<String>,
    pub profile_path: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ContentRatings {
    pub results: Vec<ContentRating>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ContentRating {
    pub iso_3166_1: String,
    pub rating: Option<String>,
    // Movie release_dates format
    pub release_dates: Option<Vec<ReleaseDate>>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ReleaseDate {
    pub certification: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ExternalIds {
    pub imdb_id: Option<String>,
    pub tvdb_id: Option<u64>,
}

impl TmdbClient {
    pub fn new(api_key: &str) -> Self {
        Self {
            api_key: api_key.to_string(),
            client: reqwest::Client::new(),
        }
    }

    /// Build a full image URL. `size` examples: "w500", "w780", "original"
    pub fn image_url(path: &str, size: &str) -> String {
        format!("{}/{}{}", IMAGE_BASE, size, path)
    }

    // ── Movie endpoints ──

    pub async fn search_movie(
        &self,
        query: &str,
        year: Option<u16>,
    ) -> Result<Vec<MovieSearchResult>, anyhow::Error> {
        let mut url = format!(
            "{}/search/movie?api_key={}&query={}&include_adult=false",
            BASE_URL,
            self.api_key,
            urlencod(query)
        );
        if let Some(y) = year {
            url.push_str(&format!("&year={}", y));
        }
        debug!("TMDB search movie: {}", query);
        let resp: SearchResults<MovieSearchResult> =
            self.client.get(&url).send().await?.json().await?;
        Ok(resp.results)
    }

    pub async fn movie_detail(&self, tmdb_id: u64) -> Result<MovieDetail, anyhow::Error> {
        let url = format!(
            "{}/movie/{}?api_key={}&append_to_response=credits,release_dates",
            BASE_URL, tmdb_id, self.api_key
        );
        debug!("TMDB movie detail: {}", tmdb_id);
        let resp: MovieDetail = self.client.get(&url).send().await?.json().await?;
        Ok(resp)
    }

    // ── TV endpoints ──

    pub async fn search_tv(
        &self,
        query: &str,
        year: Option<u16>,
    ) -> Result<Vec<TvSearchResult>, anyhow::Error> {
        let mut url = format!(
            "{}/search/tv?api_key={}&query={}&include_adult=false",
            BASE_URL,
            self.api_key,
            urlencod(query)
        );
        if let Some(y) = year {
            url.push_str(&format!("&first_air_date_year={}", y));
        }
        debug!("TMDB search TV: {}", query);
        let resp: SearchResults<TvSearchResult> =
            self.client.get(&url).send().await?.json().await?;
        Ok(resp.results)
    }

    pub async fn tv_detail(&self, tmdb_id: u64) -> Result<TvDetail, anyhow::Error> {
        let url = format!(
            "{}/tv/{}?api_key={}&append_to_response=credits,content_ratings,external_ids",
            BASE_URL, tmdb_id, self.api_key
        );
        debug!("TMDB TV detail: {}", tmdb_id);
        let resp: TvDetail = self.client.get(&url).send().await?.json().await?;
        Ok(resp)
    }

    pub async fn tv_season_detail(
        &self,
        tv_id: u64,
        season_number: u32,
    ) -> Result<Option<TvSeasonDetail>, anyhow::Error> {
        let url = format!(
            "{}/tv/{}/season/{}?api_key={}",
            BASE_URL, tv_id, season_number, self.api_key
        );
        debug!("TMDB TV season detail: {} S{}", tv_id, season_number);
        let resp = self.client.get(&url).send().await?;
        if !resp.status().is_success() {
            debug!(
                "TMDB season not found: {} S{} ({})",
                tv_id,
                season_number,
                resp.status()
            );
            return Ok(None);
        }
        let detail: TvSeasonDetail = resp.json().await?;
        Ok(Some(detail))
    }

    // ── Image download ──

    pub async fn download_image(&self, path: &str, size: &str) -> Result<Vec<u8>, anyhow::Error> {
        let url = Self::image_url(path, size);
        let bytes = self.client.get(&url).send().await?.bytes().await?;
        Ok(bytes.to_vec())
    }
}

/// Simple URL encoding for query parameters.
fn urlencod(s: &str) -> String {
    s.replace(' ', "+")
        .replace('&', "%26")
        .replace('=', "%3D")
        .replace('?', "%3F")
        .replace('#', "%23")
}
