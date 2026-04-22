//! WebVTT time-shifting.
//!
//! Given a WebVTT document, produce a shifted version where every cue
//! timestamp has `offset_seconds` subtracted. Cues that would end at or
//! before time 0 after the shift are dropped. All other content (header,
//! NOTE blocks, STYLE blocks, cue settings, cue payloads) is preserved.

use std::fmt::Write;

/// Shift every timestamp in `input` by `-offset_seconds`.
///
/// Returns a new WebVTT string. If `offset_seconds == 0.0`, returns
/// content equivalent to the input (re-normalized line endings).
pub fn shift_vtt(input: &str, offset_seconds: f64) -> String {
    let mut out = String::with_capacity(input.len());
    let mut lines = input.lines().peekable();
    let mut wrote_header = false;

    // Emit the WEBVTT header: the first non-empty line of a valid VTT file.
    // We always emit "WEBVTT" even if the source's header line has trailing text.
    while let Some(line) = lines.peek() {
        let trimmed = line.trim_start();
        if trimmed.is_empty() {
            lines.next();
            continue;
        }
        if trimmed.starts_with("WEBVTT") {
            out.push_str(line);
            out.push('\n');
            lines.next();
            wrote_header = true;
        } else {
            // Missing header — emit a synthetic one to keep output valid.
            out.push_str("WEBVTT\n");
            wrote_header = true;
        }
        break;
    }
    if !wrote_header {
        out.push_str("WEBVTT\n");
    }

    // Collect subsequent blocks (separated by blank lines) and process each.
    let mut block: Vec<String> = Vec::new();
    for line in lines {
        if line.is_empty() {
            if !block.is_empty() {
                process_block(&block, offset_seconds, &mut out);
                block.clear();
            }
            // Keep one blank line as separator only after we've written a block.
        } else {
            block.push(line.to_string());
        }
    }
    if !block.is_empty() {
        process_block(&block, offset_seconds, &mut out);
    }

    out
}

fn process_block(block: &[String], offset: f64, out: &mut String) {
    // Find the timestamp line (contains `-->`). NOTE/STYLE blocks have none.
    let ts_idx = block.iter().position(|l| l.contains("-->"));

    let Some(idx) = ts_idx else {
        // NOTE / STYLE / REGION — passthrough unchanged
        out.push('\n');
        for l in block {
            out.push_str(l);
            out.push('\n');
        }
        return;
    };

    let Some((start, end, tail)) = parse_timing_line(&block[idx]) else {
        // Malformed timing — skip the block to avoid emitting bad VTT.
        return;
    };

    let new_start = start - offset;
    let new_end = end - offset;
    if new_end <= 0.0 {
        return;
    }
    let clamped_start = new_start.max(0.0);

    out.push('\n');
    // Emit any lines before the timing line (cue identifier)
    for l in &block[..idx] {
        out.push_str(l);
        out.push('\n');
    }
    // Emit rewritten timing line
    let _ = write!(
        out,
        "{} --> {}",
        format_timestamp(clamped_start),
        format_timestamp(new_end)
    );
    if !tail.is_empty() {
        out.push(' ');
        out.push_str(tail);
    }
    out.push('\n');
    // Emit cue payload
    for l in &block[idx + 1..] {
        out.push_str(l);
        out.push('\n');
    }
}

/// Parse a VTT cue-timing line. Returns `(start_seconds, end_seconds, cue_settings)`.
fn parse_timing_line(line: &str) -> Option<(f64, f64, &str)> {
    // Find "-->" separator
    let arrow = line.find("-->")?;
    let (left, right_full) = line.split_at(arrow);
    let right = &right_full[3..]; // skip "-->"
    let start = parse_timestamp(left.trim())?;

    // Right side is: "<end>" or "<end> <cue settings>"
    let right = right.trim_start();
    // End timestamp terminates at the first whitespace
    let (end_str, tail) = match right.find(char::is_whitespace) {
        Some(i) => (&right[..i], right[i..].trim()),
        None => (right, ""),
    };
    let end = parse_timestamp(end_str.trim())?;
    Some((start, end, tail))
}

/// Parse a VTT timestamp. Accepts `hh:mm:ss.mmm` or `mm:ss.mmm`.
fn parse_timestamp(s: &str) -> Option<f64> {
    // Split on the last ':' to separate seconds.ms from leading h/m fields
    let (hms, frac) = match s.find('.') {
        Some(i) => (&s[..i], &s[i + 1..]),
        None => (s, "0"),
    };
    let parts: Vec<&str> = hms.split(':').collect();
    let (h, m, s_int) = match parts.as_slice() {
        [h, m, s] => (
            h.parse::<u64>().ok()?,
            m.parse::<u64>().ok()?,
            s.parse::<u64>().ok()?,
        ),
        [m, s] => (0u64, m.parse::<u64>().ok()?, s.parse::<u64>().ok()?),
        _ => return None,
    };
    let ms: u64 = frac.parse().ok()?;
    let ms_scale = 10u64.pow(frac.len() as u32).max(1);
    let seconds =
        h as f64 * 3600.0 + m as f64 * 60.0 + s_int as f64 + (ms as f64 / ms_scale as f64);
    Some(seconds)
}

/// Format seconds as `hh:mm:ss.mmm` (always 3 decimal places).
fn format_timestamp(seconds: f64) -> String {
    let s = seconds.max(0.0);
    let total_ms = (s * 1000.0).round() as u64;
    let h = total_ms / 3_600_000;
    let m = (total_ms % 3_600_000) / 60_000;
    let secs = (total_ms % 60_000) / 1000;
    let ms = total_ms % 1000;
    format!("{:02}:{:02}:{:02}.{:03}", h, m, secs, ms)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "WEBVTT

1
00:00:01.000 --> 00:00:02.500
Hello world

2
00:00:05.000 --> 00:00:06.000 line:90%
Second cue

3
00:01:00.000 --> 00:01:02.000
Later cue
";

    #[test]
    fn parse_timestamp_hh_mm_ss() {
        assert!((parse_timestamp("00:00:01.000").unwrap() - 1.0).abs() < 1e-6);
        assert!((parse_timestamp("01:02:03.500").unwrap() - 3723.5).abs() < 1e-6);
    }

    #[test]
    fn parse_timestamp_mm_ss() {
        assert!((parse_timestamp("00:01.500").unwrap() - 1.5).abs() < 1e-6);
        assert!((parse_timestamp("02:30.000").unwrap() - 150.0).abs() < 1e-6);
    }

    #[test]
    fn format_roundtrip() {
        for t in [0.0, 0.5, 1.0, 61.5, 3723.125] {
            let s = format_timestamp(t);
            let back = parse_timestamp(&s).unwrap();
            assert!((t - back).abs() < 1e-3, "t={} s={} back={}", t, s, back);
        }
    }

    #[test]
    fn zero_offset_preserves_cues() {
        let out = shift_vtt(SAMPLE, 0.0);
        assert!(out.starts_with("WEBVTT"));
        assert!(out.contains("Hello world"));
        assert!(out.contains("Second cue"));
        assert!(out.contains("Later cue"));
        assert!(out.contains("00:00:01.000 --> 00:00:02.500"));
    }

    #[test]
    fn positive_offset_shifts_backward_and_drops_early_cues() {
        let out = shift_vtt(SAMPLE, 3.0);
        assert!(
            !out.contains("Hello world"),
            "cue ending at 2.5s should be dropped when offset=3"
        );
        assert!(out.contains("Second cue"));
        // start 5.0 - 3 = 2.0, end 6.0 - 3 = 3.0
        assert!(out.contains("00:00:02.000 --> 00:00:03.000"));
        assert!(out.contains("line:90%"), "cue settings must be preserved");
        assert!(out.contains("Later cue"));
    }

    #[test]
    fn negative_offset_shifts_forward() {
        let out = shift_vtt(SAMPLE, -10.0);
        // start 1 - (-10) = 11, end 2.5 - (-10) = 12.5
        assert!(out.contains("00:00:11.000 --> 00:00:12.500"));
    }

    #[test]
    fn very_large_offset_drops_all_cues_but_keeps_header() {
        let out = shift_vtt(SAMPLE, 10_000.0);
        assert!(out.starts_with("WEBVTT"));
        assert!(!out.contains("Hello"));
        assert!(!out.contains("Second"));
        assert!(!out.contains("Later"));
    }

    #[test]
    fn preserves_note_blocks() {
        let with_note =
            "WEBVTT\n\nNOTE\nThis is a note\n\n1\n00:00:01.000 --> 00:00:02.000\nCue text\n";
        let out = shift_vtt(with_note, 0.0);
        assert!(out.contains("NOTE"));
        assert!(out.contains("This is a note"));
        assert!(out.contains("Cue text"));
    }

    #[test]
    fn handles_partial_cue_trim_to_zero() {
        let input = "WEBVTT\n\n1\n00:00:01.000 --> 00:00:05.000\nStraddling cue\n";
        let out = shift_vtt(input, 3.0);
        // start 1 - 3 = -2 (clamp to 0), end 5 - 3 = 2
        assert!(out.contains("00:00:00.000 --> 00:00:02.000"));
        assert!(out.contains("Straddling cue"));
    }

    #[test]
    fn synthetic_header_when_missing() {
        let input = "1\n00:00:01.000 --> 00:00:02.000\nNo header\n";
        let out = shift_vtt(input, 0.0);
        assert!(out.starts_with("WEBVTT"));
        assert!(out.contains("No header"));
    }

    #[test]
    fn preserves_voice_tags_in_payload() {
        let input = "WEBVTT\n\n1\n00:00:01.000 --> 00:00:02.000\n<v Speaker>Hello</v>\n";
        let out = shift_vtt(input, 0.0);
        assert!(out.contains("<v Speaker>Hello</v>"));
    }

    #[test]
    fn mm_ss_format_also_parsed() {
        let input = "WEBVTT\n\n1\n00:01.000 --> 00:02.000\nShort format\n";
        let out = shift_vtt(input, 0.0);
        assert!(out.contains("Short format"));
        assert!(out.contains("00:00:01.000 --> 00:00:02.000"));
    }
}
