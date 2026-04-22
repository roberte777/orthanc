//! Classify a subtitle stream by its delivery method.

use crate::models::media_stream::MediaStream;

/// How a subtitle track can be delivered to the client.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeliveryMethod {
    /// Convertible to WebVTT — served via `/api/subtitles/{id}.vtt`.
    Vtt,
    /// Bitmap formats or unsupported codecs — must be burned into the video.
    BurnRequired,
    /// Unknown / uninterpretable — hide from the UI.
    Unsupported,
}

impl DeliveryMethod {
    pub fn as_str(&self) -> &'static str {
        match self {
            DeliveryMethod::Vtt => "vtt",
            DeliveryMethod::BurnRequired => "burn_required",
            DeliveryMethod::Unsupported => "unsupported",
        }
    }
}

/// Classify a MediaStream row. Non-subtitle streams always return Unsupported.
pub fn classify(stream: &MediaStream) -> DeliveryMethod {
    if stream.stream_type != "subtitle" {
        return DeliveryMethod::Unsupported;
    }
    match stream.codec.as_deref().map(str::to_lowercase).as_deref() {
        Some("subrip" | "srt" | "webvtt" | "vtt" | "mov_text" | "ass" | "ssa" | "text") => {
            DeliveryMethod::Vtt
        }
        Some("hdmv_pgs_subtitle" | "pgs" | "pgssub") => DeliveryMethod::BurnRequired,
        Some("dvd_subtitle" | "dvdsub" | "vobsub") => DeliveryMethod::BurnRequired,
        Some("dvb_subtitle" | "dvbsub") => DeliveryMethod::BurnRequired,
        _ => DeliveryMethod::Unsupported,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stream(stream_type: &str, codec: Option<&str>) -> MediaStream {
        MediaStream {
            id: 1,
            media_item_id: 1,
            stream_index: 0,
            stream_type: stream_type.to_string(),
            codec: codec.map(str::to_string),
            language: None,
            title: None,
            is_default: false,
            is_forced: false,
            width: None,
            height: None,
            aspect_ratio: None,
            frame_rate: None,
            bit_depth: None,
            color_space: None,
            channels: None,
            sample_rate: None,
            bit_rate: None,
            is_external: false,
            external_file_path: None,
        }
    }

    #[test]
    fn text_codecs_are_vtt() {
        for codec in ["subrip", "webvtt", "ass", "ssa", "mov_text", "text"] {
            assert_eq!(
                classify(&stream("subtitle", Some(codec))),
                DeliveryMethod::Vtt,
                "codec {}",
                codec
            );
        }
    }

    #[test]
    fn case_insensitive() {
        assert_eq!(
            classify(&stream("subtitle", Some("SubRip"))),
            DeliveryMethod::Vtt
        );
        assert_eq!(
            classify(&stream("subtitle", Some("ASS"))),
            DeliveryMethod::Vtt
        );
    }

    #[test]
    fn bitmap_codecs_require_burn() {
        for codec in [
            "hdmv_pgs_subtitle",
            "pgs",
            "dvd_subtitle",
            "vobsub",
            "dvb_subtitle",
        ] {
            assert_eq!(
                classify(&stream("subtitle", Some(codec))),
                DeliveryMethod::BurnRequired,
                "codec {}",
                codec
            );
        }
    }

    #[test]
    fn unknown_codec_unsupported() {
        assert_eq!(
            classify(&stream("subtitle", Some("weirdformat"))),
            DeliveryMethod::Unsupported
        );
        assert_eq!(
            classify(&stream("subtitle", None)),
            DeliveryMethod::Unsupported
        );
    }

    #[test]
    fn non_subtitle_always_unsupported() {
        assert_eq!(
            classify(&stream("video", Some("h264"))),
            DeliveryMethod::Unsupported
        );
        assert_eq!(
            classify(&stream("audio", Some("aac"))),
            DeliveryMethod::Unsupported
        );
    }
}
