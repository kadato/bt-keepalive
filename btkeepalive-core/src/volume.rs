//! Volume formatting and parsing.

use crate::config::DEFAULT_VOLUME;

/// Human-readable percent string.
///
/// `for_input` omits the `%` suffix for the settings text field.
#[must_use]
pub fn format_volume_label(volume: f64, for_input: bool) -> String {
    let pct = volume * 100.0;
    if for_input {
        if pct >= 1.0 {
            return format_g(pct);
        }
        return format!("{pct:.6}")
            .trim_end_matches('0')
            .trim_end_matches('.')
            .to_string();
    }
    let body = format!("{pct:.4}")
        .trim_end_matches('0')
        .trim_end_matches('.')
        .to_string();
    format!("{body}%")
}

/// Shortest round-trip display for values >= 1, like the `{v:g}` format.
fn format_g(v: f64) -> String {
    if v.fract() == 0.0 && v.abs() < 1e15 {
        format!("{}", v as i64)
    } else {
        format!("{v}")
    }
}

/// Clamp invalid values to the default; otherwise keep the exact gain.
#[must_use]
pub fn normalize_volume(volume: f64) -> f64 {
    if !volume.is_finite() || volume <= 0.0 || volume > 1.0 {
        DEFAULT_VOLUME
    } else {
        volume
    }
}

/// Parse input for a field labeled with `%`: `1` means 1%.
/// Returns linear gain in (0, 1], or `None` when invalid.
#[must_use]
pub fn parse_volume_percent(text: &str) -> Option<f64> {
    let cleaned = text.trim().replace('%', "").trim().to_string();
    if cleaned.is_empty() {
        return None;
    }
    let value: f64 = cleaned.parse().ok()?;
    if !value.is_finite() || value <= 0.0 || value > 100.0 {
        return None;
    }
    Some(value / 100.0)
}

/// Parse free-form input. Returns linear gain in (0, 1], or `None`.
#[must_use]
pub fn parse_volume_text(text: &str) -> Option<f64> {
    let raw = text.trim();
    if raw.is_empty() {
        return None;
    }
    let has_percent = raw.contains('%');
    let cleaned = raw.replace('%', "").trim().to_string();
    if cleaned.is_empty() {
        return None;
    }
    let value: f64 = cleaned.parse().ok()?;
    if !value.is_finite() {
        return None;
    }
    let gain = if has_percent {
        value / 100.0
    } else if value > 0.0 && value < 1.0 {
        value
    } else if value >= 1.0 {
        value / 100.0
    } else {
        return None;
    };
    if !gain.is_finite() || gain <= 0.0 || gain > 1.0 {
        return None;
    }
    Some(gain)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn approx(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-9
    }

    #[test]
    fn percent_cases() {
        for (text, expected) in [
            ("2", 0.02),
            ("2%", 0.02),
            ("0.5", 0.005),
            ("100", 1.0),
            ("0.01", 0.0001),
        ] {
            let v = parse_volume_percent(text).unwrap();
            assert!(approx(v, expected), "{text} -> {v}");
        }
    }

    #[test]
    fn percent_invalid() {
        for text in ["", "abc", "-1", "101", "0"] {
            assert_eq!(parse_volume_percent(text), None, "{text}");
        }
    }

    #[test]
    fn freeform_cases() {
        for (text, expected) in [("50%", 0.5), ("0.5", 0.5), ("2", 0.02)] {
            let v = parse_volume_text(text).unwrap();
            assert!(approx(v, expected), "{text} -> {v}");
        }
    }

    #[test]
    fn labels() {
        assert_eq!(format_volume_label(0.02, false), "2%");
        assert_eq!(format_volume_label(0.0001, false), "0.01%");
        assert_eq!(format_volume_label(0.02, true), "2");
        assert_eq!(format_volume_label(1.0, true), "100");
    }

    #[test]
    fn normalize_clamps() {
        assert_eq!(normalize_volume(-1.0), DEFAULT_VOLUME);
        assert_eq!(normalize_volume(2.0), DEFAULT_VOLUME);
        assert_eq!(normalize_volume(f64::NAN), DEFAULT_VOLUME);
        assert!(approx(normalize_volume(0.05), 0.05));
    }
}
