/// Format a byte count as a human-friendly string ("1.4 GB", "850 MB").
pub fn format_size(bytes: Option<i64>) -> String {
    match bytes {
        Some(b) if b >= 1_073_741_824 => format!("{:.1} GB", b as f64 / 1_073_741_824.0),
        Some(b) if b >= 1_048_576 => format!("{:.0} MB", b as f64 / 1_048_576.0),
        Some(b) => format!("{} KB", b / 1024),
        None => String::new(),
    }
}

/// Extract the year from a YYYY-MM-DD release date string.
pub fn format_year(release_date: &Option<String>) -> String {
    release_date
        .as_ref()
        .and_then(|d| d.get(..4))
        .unwrap_or("")
        .to_string()
}

/// Format a duration in seconds as "Hh Mm" or "Mm".
pub fn format_runtime(seconds: Option<i32>) -> String {
    match seconds {
        Some(s) if s > 0 => {
            let h = s / 3600;
            let m = (s % 3600) / 60;
            if h > 0 {
                format!("{}h {}m", h, m)
            } else {
                format!("{}m", m)
            }
        }
        _ => String::new(),
    }
}

/// Format seconds as "M:SS" or "H:MM:SS" — used by player UIs.
pub fn format_time(seconds: f64) -> String {
    if !seconds.is_finite() || seconds < 0.0 {
        return "0:00".to_string();
    }
    let total = seconds as u64;
    let h = total / 3600;
    let m = (total % 3600) / 60;
    let s = total % 60;
    if h > 0 {
        format!("{}:{:02}:{:02}", h, m, s)
    } else {
        format!("{}:{:02}", m, s)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn size_gigabytes() {
        assert_eq!(format_size(Some(2_147_483_648)), "2.0 GB");
    }

    #[test]
    fn size_none_is_empty() {
        assert_eq!(format_size(None), "");
    }

    #[test]
    fn year_extracts_first_four() {
        assert_eq!(format_year(&Some("2024-06-12".into())), "2024");
        assert_eq!(format_year(&None), "");
    }

    #[test]
    fn runtime_with_hours() {
        assert_eq!(format_runtime(Some(7325)), "2h 2m");
    }

    #[test]
    fn runtime_minutes_only() {
        assert_eq!(format_runtime(Some(540)), "9m");
    }

    #[test]
    fn time_short() {
        assert_eq!(format_time(75.0), "1:15");
    }

    #[test]
    fn time_long() {
        assert_eq!(format_time(3725.0), "1:02:05");
    }
}
