use regex::Regex;
use std::sync::LazyLock;

use super::walker::FileInfo;

#[derive(Debug, Clone)]
pub enum ParsedMedia {
    Movie {
        title: String,
        year: Option<u16>,
        part: Option<u32>,
    },
    Episode {
        show_name: String,
        season: u32,
        episode: u32,
        episode_title: Option<String>,
        year: Option<u16>,
    },
    Unknown {
        filename: String,
    },
}

// ── Regex patterns (compiled once) ──

// TV: "Show Name (2020) - S03E01 - Episode Title [tags]"  (Sonarr style)
static RE_TV_SONARR: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)^(.+?)\s*(?:\((\d{4})\))?\s*-\s*S(\d+)E(\d+)\s*(?:-\s*(.+?))?(?:\s*\[.*)?$")
        .unwrap()
});

// TV: "Show.Name.S01E02.Episode.Title.720p.mkv" (dotted style)
static RE_TV_DOTTED: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)^(.+?)\.S(\d+)E(\d+)\.?(.*?)(?:\.\d{3,4}p|\[|\.\w{2,4}$)")
        .unwrap()
});

// TV: generic fallback - anything with SxxExx
static RE_TV_GENERIC: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)S(\d+)E(\d+)").unwrap()
});

// Year in parentheses: "(2024)"
static RE_YEAR_PAREN: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"\((\d{4})\)").unwrap()
});

// Year in dots: ".2024." or ".2024" at end
static RE_YEAR_DOT: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"[\.\s](\d{4})(?:[\.\s]|$)").unwrap()
});

// Multi-part: Part1, Part2, Disc1, Disc2, CD1, CD2 etc.
static RE_MULTI_PART: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)(?:part|disc|cd|pt)[\s._-]*(\d+)").unwrap()
});

// Quality/tag markers - everything from here on is metadata, not title
static RE_QUALITY_MARKER: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)[\.\s\[]\s*(?:\d{3,4}p|WEBDL|WEB-DL|WEBRip|BluRay|BDRip|HDRip|HDTV|DVDRip|IMAX|Proper|Repack|REMUX|DV\s|HDR|Atmos|EAC3|AAC|DTS|x264|x265|h264|h265|HEVC|AVC)\b").unwrap()
});

// External ID tags: {tmdb-12345}, {tvdb-12345}, {imdb-tt12345}
static RE_EXTERNAL_ID: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"\{(?:tmdb|tvdb|imdb)-[^\}]+\}").unwrap()
});

// Release group at end: -GroupName
static RE_RELEASE_GROUP: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"-[A-Za-z][A-Za-z0-9]+$").unwrap()
});

/// Parse a media file based on its filename and the library type context.
pub fn parse_media_file(file_info: &FileInfo, library_type: &str) -> ParsedMedia {
    let filename = file_info
        .path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("");

    // Also get the parent folder name for context (e.g., show name from folder)
    let parent_name = file_info
        .path
        .parent()
        .and_then(|p| p.file_name())
        .and_then(|s| s.to_str())
        .unwrap_or("");

    // Grandparent folder (for TV: show folder when inside Season XX/)
    let grandparent_name = file_info
        .path
        .parent()
        .and_then(|p| p.parent())
        .and_then(|p| p.file_name())
        .and_then(|s| s.to_str())
        .unwrap_or("");

    if library_type == "tv_shows" || RE_TV_GENERIC.is_match(filename) {
        if let Some(ep) = parse_episode(filename, parent_name, grandparent_name) {
            return ep;
        }
    }

    if library_type == "movies" || !RE_TV_GENERIC.is_match(filename) {
        return parse_movie(filename, parent_name);
    }

    ParsedMedia::Unknown {
        filename: filename.to_string(),
    }
}

fn parse_episode(
    filename: &str,
    parent_name: &str,
    grandparent_name: &str,
) -> Option<ParsedMedia> {
    // Try Sonarr-style: "SHOW NAME (2020) - S03E01 - Episode Title [tags]"
    if let Some(caps) = RE_TV_SONARR.captures(filename) {
        let raw_name = caps.get(1).unwrap().as_str().trim();
        let year = caps.get(2).and_then(|m| m.as_str().parse().ok());
        let season: u32 = caps[3].parse().ok()?;
        let episode: u32 = caps[4].parse().ok()?;
        let episode_title = caps.get(5).map(|m| clean_episode_title(m.as_str()));

        return Some(ParsedMedia::Episode {
            show_name: clean_title(raw_name),
            season,
            episode,
            episode_title,
            year,
        });
    }

    // Try dotted style: "Show.Name.S01E02.Episode.Title.720p.mkv"
    if let Some(caps) = RE_TV_DOTTED.captures(filename) {
        let raw_name = caps.get(1).unwrap().as_str();
        let season: u32 = caps[2].parse().ok()?;
        let episode: u32 = caps[3].parse().ok()?;
        let ep_title_raw = caps.get(4).map(|m| m.as_str()).unwrap_or("");

        let show_name = clean_dotted_name(raw_name);
        let episode_title = if ep_title_raw.is_empty() {
            None
        } else {
            Some(clean_dotted_name(ep_title_raw))
        };

        // Try to get year from show name
        let year = RE_YEAR_PAREN
            .captures(&show_name)
            .and_then(|c| c.get(1))
            .and_then(|m| m.as_str().parse().ok());

        let show_name = RE_YEAR_PAREN.replace(&show_name, "").trim().to_string();

        return Some(ParsedMedia::Episode {
            show_name,
            season,
            episode,
            episode_title,
            year,
        });
    }

    // Generic fallback: just find SxxExx anywhere
    if let Some(caps) = RE_TV_GENERIC.captures(filename) {
        let season: u32 = caps[1].parse().ok()?;
        let episode: u32 = caps[2].parse().ok()?;

        // Derive show name from folder structure
        let show_name = if parent_name.to_lowercase().starts_with("season") {
            // We're in a "Season XX" folder, show name is grandparent
            extract_show_name_from_folder(grandparent_name)
        } else {
            // We're directly inside the show folder
            extract_show_name_from_folder(parent_name)
        };

        let year = RE_YEAR_PAREN
            .captures(&show_name)
            .and_then(|c| c.get(1))
            .and_then(|m| m.as_str().parse().ok());
        let show_name = RE_YEAR_PAREN.replace(&show_name, "").trim().to_string();

        return Some(ParsedMedia::Episode {
            show_name,
            season,
            episode,
            episode_title: None,
            year,
        });
    }

    None
}

fn parse_movie(filename: &str, parent_name: &str) -> ParsedMedia {
    // Try to extract from the parent folder first (more reliable for organized libraries)
    // Parent folder: "Movie Name (2024)" or "Movie Name (2024) {tmdb-12345}"
    let (folder_title, folder_year) = parse_movie_folder(parent_name);

    // Parse the filename itself
    let cleaned = strip_tags(filename);

    // Check for multi-part
    let part = RE_MULTI_PART
        .captures(&cleaned)
        .and_then(|c| c.get(1))
        .and_then(|m| m.as_str().parse().ok());

    // If the folder gives us a good title + year, use that
    if !folder_title.is_empty() && folder_year.is_some() {
        return ParsedMedia::Movie {
            title: folder_title,
            year: folder_year,
            part,
        };
    }

    // Otherwise parse from filename
    let (title, year) = extract_title_year(&cleaned);

    if !title.is_empty() {
        ParsedMedia::Movie { title, year, part }
    } else {
        ParsedMedia::Unknown {
            filename: filename.to_string(),
        }
    }
}

fn parse_movie_folder(folder_name: &str) -> (String, Option<u16>) {
    if folder_name.is_empty() {
        return (String::new(), None);
    }

    let mut name = folder_name.to_string();

    // Remove external IDs: {tmdb-12345}
    name = RE_EXTERNAL_ID.replace_all(&name, "").trim().to_string();

    // Extract year
    let year = RE_YEAR_PAREN
        .captures(&name)
        .and_then(|c| c.get(1))
        .and_then(|m| m.as_str().parse().ok());

    // Remove year for clean title
    let title = RE_YEAR_PAREN.replace(&name, "").trim().to_string();
    let title = title.trim_end_matches(" -").trim().to_string();

    (title, year)
}

fn extract_title_year(cleaned: &str) -> (String, Option<u16>) {
    // Try year in parentheses first
    if let Some(caps) = RE_YEAR_PAREN.captures(cleaned) {
        let year: Option<u16> = caps.get(1).and_then(|m| m.as_str().parse().ok());
        let pos = caps.get(0).unwrap().start();
        let title = clean_dotted_name(&cleaned[..pos]);
        return (title, year);
    }

    // Try year in dots
    if let Some(caps) = RE_YEAR_DOT.captures(cleaned) {
        let year: Option<u16> = caps.get(1).and_then(|m| m.as_str().parse().ok());
        let pos = caps.get(0).unwrap().start();
        let title = clean_dotted_name(&cleaned[..pos]);
        return (title, year);
    }

    // No year found - clean the whole thing as a title
    let title = clean_dotted_name(cleaned);
    (title, None)
}

/// Strip quality tags, release groups, and bracket metadata from a filename.
fn strip_tags(filename: &str) -> String {
    let mut result = filename.to_string();

    // Remove external IDs
    result = RE_EXTERNAL_ID.replace_all(&result, "").to_string();

    // Remove bracketed sections: [WEBDL-1080p][EAC3 2.0][x264]
    let re_brackets = Regex::new(r"\[[^\]]*\]").unwrap();
    result = re_brackets.replace_all(&result, "").to_string();

    // Remove release group at end
    result = RE_RELEASE_GROUP.replace(&result, "").to_string();

    // Remove quality markers and everything after
    if let Some(m) = RE_QUALITY_MARKER.find(&result) {
        result = result[..m.start()].to_string();
    }

    result.trim().to_string()
}

/// Convert "Show.Name.Here" or "Show Name Here" to "Show Name Here"
fn clean_dotted_name(name: &str) -> String {
    name.replace('.', " ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// Clean a title string (trim whitespace, remove trailing hyphens/dots)
fn clean_title(name: &str) -> String {
    name.trim()
        .trim_end_matches(&['-', '.', ' '][..])
        .trim()
        .to_string()
}

/// Clean up an episode title from tag residue
fn clean_episode_title(raw: &str) -> String {
    let cleaned = strip_tags(raw);
    clean_dotted_name(&cleaned)
}

/// Extract show name from a folder like "Show Name (2020) {tvdb-12345}"
fn extract_show_name_from_folder(folder: &str) -> String {
    let mut name = folder.to_string();
    name = RE_EXTERNAL_ID.replace_all(&name, "").trim().to_string();
    name = name.trim_end_matches(" -").trim().to_string();
    name
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn make_file_info(path: &str) -> FileInfo {
        FileInfo {
            path: PathBuf::from(path),
            size: 1000,
            modified: None,
            created: None,
        }
    }

    #[test]
    fn test_sonarr_tv_naming() {
        let fi = make_file_info(
            "/tv/Jujutsu Kaisen (2020) {tvdb-377543}/Season 03/JUJUTSU KAISEN (2020) - S03E01 - Execution [WEBDL-1080p][EAC3 2.0][x264]-Kitsune.mkv",
        );
        let parsed = parse_media_file(&fi, "tv_shows");
        match parsed {
            ParsedMedia::Episode {
                show_name,
                season,
                episode,
                episode_title,
                year,
            } => {
                assert_eq!(show_name, "JUJUTSU KAISEN");
                assert_eq!(season, 3);
                assert_eq!(episode, 1);
                assert_eq!(episode_title, Some("Execution".to_string()));
                assert_eq!(year, Some(2020));
            }
            other => panic!("Expected Episode, got {:?}", other),
        }
    }

    #[test]
    fn test_movie_with_tags() {
        let fi = make_file_info(
            "/movies/Avatar - Fire and Ash (2025)/Avatar Fire and Ash (2025) {tmdb-83533} [WEBDL-2160p][DV HDR10Plus][EAC3 Atmos 5.1][h265]-BYNDR.mkv",
        );
        let parsed = parse_media_file(&fi, "movies");
        match parsed {
            ParsedMedia::Movie { title, year, part } => {
                assert_eq!(title, "Avatar - Fire and Ash");
                assert_eq!(year, Some(2025));
                assert!(part.is_none());
            }
            other => panic!("Expected Movie, got {:?}", other),
        }
    }

    #[test]
    fn test_dotted_movie() {
        let fi = make_file_info("/movies/Dune.Part.Two.2024.IMAX.2160p.WEB-DL.mkv");
        let parsed = parse_media_file(&fi, "movies");
        match parsed {
            ParsedMedia::Movie { title, year, .. } => {
                assert_eq!(title, "Dune Part Two");
                assert_eq!(year, Some(2024));
            }
            other => panic!("Expected Movie, got {:?}", other),
        }
    }
}
