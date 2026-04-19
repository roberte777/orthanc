//! Language-code parsing for subtitle sidecar filenames.
//!
//! Accepts ISO 639-1 (2-char), ISO 639-2/3 (3-char), and common English names.
//! Always emits the 2-char ISO 639-1 code when possible.

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Token {
    /// A language code — normalized to ISO 639-1 (2-char) when known,
    /// otherwise the input lowercased.
    Language(String),
    Forced,
    Sdh,
    Cc,
    Default,
    /// An uninterpreted token — likely a track title like "Commentary" or "Director".
    Unknown(String),
}

/// Classify a single dot-separated suffix token.
pub fn classify_token(raw: &str) -> Option<Token> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    let lower = trimmed.to_lowercase();

    match lower.as_str() {
        "forced" => return Some(Token::Forced),
        "sdh" | "hi" => return Some(Token::Sdh),
        "cc" | "closedcaption" | "closedcaptions" => return Some(Token::Cc),
        "default" => return Some(Token::Default),
        _ => {}
    }

    if let Some(code) = resolve_language(&lower) {
        return Some(Token::Language(code.to_string()));
    }

    Some(Token::Unknown(trimmed.to_string()))
}

/// Map a candidate language token to its ISO 639-1 code.
///
/// Accepts 2-char, 3-char, or English name. Returns None if not a known language.
pub fn resolve_language(candidate: &str) -> Option<&'static str> {
    let c = candidate.to_lowercase();
    LANG_TABLE
        .iter()
        .find(|entry| entry.iso1 == c || entry.iso2 == c || entry.iso3 == c || entry.name == c)
        .map(|entry| entry.iso1)
}

struct LangEntry {
    iso1: &'static str,
    iso2: &'static str, // ISO 639-2/B (bibliographic) or /T where applicable
    iso3: &'static str, // ISO 639-3 (often same as iso2)
    name: &'static str,
}

const LANG_TABLE: &[LangEntry] = &[
    LangEntry { iso1: "en", iso2: "eng", iso3: "eng", name: "english" },
    LangEntry { iso1: "es", iso2: "spa", iso3: "spa", name: "spanish" },
    LangEntry { iso1: "fr", iso2: "fre", iso3: "fra", name: "french" },
    LangEntry { iso1: "de", iso2: "ger", iso3: "deu", name: "german" },
    LangEntry { iso1: "it", iso2: "ita", iso3: "ita", name: "italian" },
    LangEntry { iso1: "pt", iso2: "por", iso3: "por", name: "portuguese" },
    LangEntry { iso1: "nl", iso2: "dut", iso3: "nld", name: "dutch" },
    LangEntry { iso1: "ja", iso2: "jpn", iso3: "jpn", name: "japanese" },
    LangEntry { iso1: "ko", iso2: "kor", iso3: "kor", name: "korean" },
    LangEntry { iso1: "zh", iso2: "chi", iso3: "zho", name: "chinese" },
    LangEntry { iso1: "ru", iso2: "rus", iso3: "rus", name: "russian" },
    LangEntry { iso1: "ar", iso2: "ara", iso3: "ara", name: "arabic" },
    LangEntry { iso1: "hi", iso2: "hin", iso3: "hin", name: "hindi" },
    LangEntry { iso1: "pl", iso2: "pol", iso3: "pol", name: "polish" },
    LangEntry { iso1: "tr", iso2: "tur", iso3: "tur", name: "turkish" },
    LangEntry { iso1: "sv", iso2: "swe", iso3: "swe", name: "swedish" },
    LangEntry { iso1: "no", iso2: "nor", iso3: "nor", name: "norwegian" },
    LangEntry { iso1: "da", iso2: "dan", iso3: "dan", name: "danish" },
    LangEntry { iso1: "fi", iso2: "fin", iso3: "fin", name: "finnish" },
    LangEntry { iso1: "el", iso2: "gre", iso3: "ell", name: "greek" },
    LangEntry { iso1: "cs", iso2: "cze", iso3: "ces", name: "czech" },
    LangEntry { iso1: "hu", iso2: "hun", iso3: "hun", name: "hungarian" },
    LangEntry { iso1: "ro", iso2: "rum", iso3: "ron", name: "romanian" },
    LangEntry { iso1: "he", iso2: "heb", iso3: "heb", name: "hebrew" },
    LangEntry { iso1: "th", iso2: "tha", iso3: "tha", name: "thai" },
    LangEntry { iso1: "vi", iso2: "vie", iso3: "vie", name: "vietnamese" },
    LangEntry { iso1: "id", iso2: "ind", iso3: "ind", name: "indonesian" },
    LangEntry { iso1: "ms", iso2: "may", iso3: "msa", name: "malay" },
    LangEntry { iso1: "uk", iso2: "ukr", iso3: "ukr", name: "ukrainian" },
    LangEntry { iso1: "bg", iso2: "bul", iso3: "bul", name: "bulgarian" },
    LangEntry { iso1: "hr", iso2: "hrv", iso3: "hrv", name: "croatian" },
    LangEntry { iso1: "sr", iso2: "srp", iso3: "srp", name: "serbian" },
    LangEntry { iso1: "sk", iso2: "slo", iso3: "slk", name: "slovak" },
    LangEntry { iso1: "sl", iso2: "slv", iso3: "slv", name: "slovenian" },
    LangEntry { iso1: "ca", iso2: "cat", iso3: "cat", name: "catalan" },
    LangEntry { iso1: "lt", iso2: "lit", iso3: "lit", name: "lithuanian" },
    LangEntry { iso1: "lv", iso2: "lav", iso3: "lav", name: "latvian" },
    LangEntry { iso1: "et", iso2: "est", iso3: "est", name: "estonian" },
    LangEntry { iso1: "fa", iso2: "per", iso3: "fas", name: "persian" },
    LangEntry { iso1: "bn", iso2: "ben", iso3: "ben", name: "bengali" },
    LangEntry { iso1: "ta", iso2: "tam", iso3: "tam", name: "tamil" },
];

/// Summary of all tokens parsed from a sidecar filename suffix.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Suffix {
    pub language: Option<String>,
    pub is_forced: bool,
    pub is_sdh: bool,
    pub is_default: bool,
    /// Tokens that were not recognized — kept for generating a title.
    pub unknown: Vec<String>,
}

impl Suffix {
    /// Parse the dot-separated part of a filename between the video stem and the extension.
    ///
    /// E.g. for `movie.en.forced.srt` with video stem `movie`, the input is `"en.forced"`.
    pub fn parse(suffix: &str) -> Self {
        let mut out = Suffix::default();
        if suffix.is_empty() {
            return out;
        }
        for raw in suffix.split('.') {
            match classify_token(raw) {
                Some(Token::Language(code)) => out.language = Some(code),
                Some(Token::Forced) => out.is_forced = true,
                Some(Token::Sdh) | Some(Token::Cc) => out.is_sdh = true,
                Some(Token::Default) => out.is_default = true,
                Some(Token::Unknown(s)) => out.unknown.push(s),
                None => {}
            }
        }
        out
    }

    /// Build a display title from the parsed suffix, or None if nothing notable.
    pub fn title(&self) -> Option<String> {
        let mut parts: Vec<String> = Vec::new();
        if let Some(l) = &self.language {
            parts.push(language_display_name(l).to_string());
        }
        if self.is_forced {
            parts.push("Forced".to_string());
        }
        if self.is_sdh {
            parts.push("SDH".to_string());
        }
        parts.extend(self.unknown.iter().cloned());
        if parts.is_empty() {
            None
        } else {
            Some(parts.join(" · "))
        }
    }
}

/// Render a language code as a readable English name. Falls back to the code itself.
pub fn language_display_name(code: &str) -> &str {
    let c = code.to_lowercase();
    for entry in LANG_TABLE {
        if entry.iso1 == c {
            return match entry.iso1 {
                "en" => "English",
                "es" => "Spanish",
                "fr" => "French",
                "de" => "German",
                "it" => "Italian",
                "pt" => "Portuguese",
                "nl" => "Dutch",
                "ja" => "Japanese",
                "ko" => "Korean",
                "zh" => "Chinese",
                "ru" => "Russian",
                "ar" => "Arabic",
                "hi" => "Hindi",
                "pl" => "Polish",
                "tr" => "Turkish",
                "sv" => "Swedish",
                "no" => "Norwegian",
                "da" => "Danish",
                "fi" => "Finnish",
                "el" => "Greek",
                "cs" => "Czech",
                "hu" => "Hungarian",
                "ro" => "Romanian",
                "he" => "Hebrew",
                "th" => "Thai",
                "vi" => "Vietnamese",
                "id" => "Indonesian",
                "ms" => "Malay",
                "uk" => "Ukrainian",
                "bg" => "Bulgarian",
                "hr" => "Croatian",
                "sr" => "Serbian",
                "sk" => "Slovak",
                "sl" => "Slovenian",
                "ca" => "Catalan",
                "lt" => "Lithuanian",
                "lv" => "Latvian",
                "et" => "Estonian",
                "fa" => "Persian",
                "bn" => "Bengali",
                "ta" => "Tamil",
                _ => code,
            };
        }
    }
    code
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_two_char_code() {
        assert_eq!(classify_token("en"), Some(Token::Language("en".into())));
        assert_eq!(classify_token("es"), Some(Token::Language("es".into())));
    }

    #[test]
    fn classifies_three_char_code_bibliographic() {
        assert_eq!(classify_token("eng"), Some(Token::Language("en".into())));
        assert_eq!(classify_token("fre"), Some(Token::Language("fr".into())));
        assert_eq!(classify_token("ger"), Some(Token::Language("de".into())));
    }

    #[test]
    fn classifies_three_char_code_terminological() {
        assert_eq!(classify_token("fra"), Some(Token::Language("fr".into())));
        assert_eq!(classify_token("deu"), Some(Token::Language("de".into())));
    }

    #[test]
    fn classifies_english_name() {
        assert_eq!(classify_token("english"), Some(Token::Language("en".into())));
        assert_eq!(classify_token("Spanish"), Some(Token::Language("es".into())));
    }

    #[test]
    fn classifies_case_insensitive() {
        assert_eq!(classify_token("ENG"), Some(Token::Language("en".into())));
        assert_eq!(classify_token("ENGLISH"), Some(Token::Language("en".into())));
    }

    #[test]
    fn classifies_flags() {
        assert_eq!(classify_token("forced"), Some(Token::Forced));
        assert_eq!(classify_token("FORCED"), Some(Token::Forced));
        assert_eq!(classify_token("sdh"), Some(Token::Sdh));
        assert_eq!(classify_token("cc"), Some(Token::Cc));
        assert_eq!(classify_token("default"), Some(Token::Default));
    }

    #[test]
    fn empty_returns_none() {
        assert_eq!(classify_token(""), None);
        assert_eq!(classify_token("   "), None);
    }

    #[test]
    fn unknown_token_kept_as_unknown() {
        match classify_token("commentary") {
            Some(Token::Unknown(s)) => assert_eq!(s, "commentary"),
            other => panic!("expected Unknown, got {:?}", other),
        }
    }

    #[test]
    fn suffix_parse_order_independent() {
        let a = Suffix::parse("en.forced");
        let b = Suffix::parse("forced.en");
        assert_eq!(a.language, Some("en".into()));
        assert!(a.is_forced);
        assert_eq!(b.language, Some("en".into()));
        assert!(b.is_forced);
    }

    #[test]
    fn suffix_parse_handles_multiple_flags() {
        let s = Suffix::parse("en.forced.sdh");
        assert_eq!(s.language, Some("en".into()));
        assert!(s.is_forced);
        assert!(s.is_sdh);
    }

    #[test]
    fn suffix_parse_unknown_tokens_preserved() {
        let s = Suffix::parse("en.commentary");
        assert_eq!(s.language, Some("en".into()));
        assert_eq!(s.unknown, vec!["commentary".to_string()]);
    }

    #[test]
    fn suffix_parse_empty() {
        let s = Suffix::parse("");
        assert!(s.language.is_none());
        assert!(!s.is_forced);
        assert!(!s.is_sdh);
        assert!(s.unknown.is_empty());
    }

    #[test]
    fn title_reflects_parsed_tokens() {
        let s = Suffix::parse("en.forced");
        assert_eq!(s.title().as_deref(), Some("English · Forced"));

        let s = Suffix::parse("en.sdh");
        assert_eq!(s.title().as_deref(), Some("English · SDH"));

        let s = Suffix::parse("en");
        assert_eq!(s.title().as_deref(), Some("English"));

        let s = Suffix::parse("");
        assert_eq!(s.title(), None);
    }

    #[test]
    fn title_keeps_unknown_tokens() {
        let s = Suffix::parse("en.commentary");
        assert_eq!(s.title().as_deref(), Some("English · commentary"));
    }
}
