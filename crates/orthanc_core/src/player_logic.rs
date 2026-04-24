//! Pure helper functions for the playback UI: track formatters and codec
//! capabilities. These are platform-agnostic so both the web and mobile
//! players consume the same logic.

use crate::api::{AudioTrack, SubtitleTrack};

/// User-facing label for a subtitle track. Includes title or language as the
/// base, plus chips for Forced / Default / External flags.
pub fn subtitle_label(t: &SubtitleTrack) -> String {
    let base = t
        .title
        .clone()
        .or_else(|| t.language.clone())
        .unwrap_or_else(|| format!("Subtitle #{}", t.id));
    let mut chips: Vec<&str> = Vec::new();
    if t.is_forced {
        chips.push("Forced");
    }
    if t.is_default {
        chips.push("Default");
    }
    if t.is_external {
        chips.push("External");
    }
    if chips.is_empty() {
        base
    } else {
        format!("{} · {}", base, chips.join(" · "))
    }
}

/// User-facing label for an audio track. Includes language/title as the base,
/// plus chips for codec, channel layout (Mono/Stereo/5.1/7.1), and Default.
pub fn audio_label(t: &AudioTrack) -> String {
    let base = t
        .language
        .clone()
        .or_else(|| t.title.clone())
        .unwrap_or_else(|| format!("Audio #{}", t.id));
    let mut chips: Vec<String> = Vec::new();
    if let Some(codec) = &t.codec {
        chips.push(codec.to_uppercase());
    }
    if let Some(ch) = t.channels {
        let pretty = match ch {
            1 => "Mono".to_string(),
            2 => "Stereo".to_string(),
            6 => "5.1".to_string(),
            8 => "7.1".to_string(),
            n => format!("{}ch", n),
        };
        chips.push(pretty);
    }
    if let Some(title) = &t.title
        && t.language.as_ref().map(|l| l != title).unwrap_or(true)
    {
        chips.push(title.clone());
    }
    if t.is_default {
        chips.push("Default".to_string());
    }
    if chips.is_empty() {
        base
    } else {
        format!("{} · {}", base, chips.join(" · "))
    }
}

/// Conservative codec capabilities for mobile clients. Sent in the stream-token
/// request so the server picks an appropriate transcode mode.
///
/// The lists are intentionally conservative — anything outside them gets
/// transcoded to a guaranteed-compatible format. Newer Android devices support
/// HEVC and Opus, but support varies by manufacturer; v1 trades some
/// transcoding cost for guaranteed compatibility.
pub struct Capabilities {
    pub video: Vec<String>,
    pub audio: Vec<String>,
    pub containers: Vec<String>,
}

#[cfg(target_os = "ios")]
pub fn mobile_capabilities() -> Capabilities {
    Capabilities {
        video: vec!["h264".into(), "hevc".into()],
        audio: vec!["aac".into(), "ac3".into(), "eac3".into()],
        containers: vec!["mp4".into(), "mov".into(), "m4v".into()],
    }
}

#[cfg(target_os = "android")]
pub fn mobile_capabilities() -> Capabilities {
    Capabilities {
        video: vec!["h264".into()],
        audio: vec!["aac".into()],
        containers: vec!["mp4".into()],
    }
}

/// Used during `dx serve --platform desktop` dev loops where the host is
/// macOS or Linux. Mirrors the iOS list since the mobile WebView surface is
/// closest to a Safari/WKWebView profile.
#[cfg(not(any(target_os = "ios", target_os = "android")))]
pub fn mobile_capabilities() -> Capabilities {
    Capabilities {
        video: vec!["h264".into(), "hevc".into()],
        audio: vec!["aac".into(), "ac3".into(), "eac3".into()],
        containers: vec!["mp4".into(), "mov".into(), "m4v".into()],
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::{AudioTrack, SubtitleTrack};

    fn sub(id: i64, lang: Option<&str>, forced: bool) -> SubtitleTrack {
        SubtitleTrack {
            id,
            language: lang.map(String::from),
            title: None,
            codec: None,
            is_default: false,
            is_forced: forced,
            is_external: false,
            delivery: "vtt".into(),
        }
    }

    #[test]
    fn subtitle_label_falls_back_to_id() {
        let t = sub(7, None, false);
        assert_eq!(subtitle_label(&t), "Subtitle #7");
    }

    #[test]
    fn subtitle_label_marks_forced() {
        let t = sub(1, Some("eng"), true);
        assert_eq!(subtitle_label(&t), "eng · Forced");
    }

    #[test]
    fn audio_label_handles_stereo() {
        let t = AudioTrack {
            id: 1,
            language: Some("eng".into()),
            title: None,
            codec: Some("aac".into()),
            channels: Some(2),
            sample_rate: None,
            bit_rate: None,
            is_default: true,
        };
        assert_eq!(audio_label(&t), "eng · AAC · Stereo · Default");
    }

    #[test]
    fn audio_label_5_1() {
        let t = AudioTrack {
            id: 1,
            language: Some("eng".into()),
            title: None,
            codec: Some("ac3".into()),
            channels: Some(6),
            sample_rate: None,
            bit_rate: None,
            is_default: false,
        };
        assert_eq!(audio_label(&t), "eng · AC3 · 5.1");
    }
}
