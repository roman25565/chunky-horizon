//! The command's input: numbers with Chunky's suffixes, relative radii and
//! world names.
use crate::selection::MAX_COORD;

/// `Input.tryDoubleSuffixed`: `16`, `2.5k` (thousands), `20c` (chunks, x16),
/// `3r` (regions, x512); case does not matter.
pub fn number(s: &str) -> Option<f64> {
    let s = s.trim();
    let (digits, mul) = match s.chars().last()?.to_ascii_lowercase() {
        'c' => (&s[..s.len() - 1], 16.0),
        'r' => (&s[..s.len() - 1], 512.0),
        'k' => (&s[..s.len() - 1], 1000.0),
        _ => (s, 1.0),
    };
    let v: f64 = digits.parse().ok()?;
    v.is_finite().then_some(v * mul)
}

/// A centre coordinate: a number no further than 30,000,000 from 0.
pub fn coordinate(s: &str) -> Option<f64> {
    number(s).filter(|v| v.abs() <= MAX_COORD)
}

/// A radius: a number, or `+d`/`-d` relative to `current`; never negative,
/// at most 30,000,000.
pub fn radius(s: &str, current: f64) -> Option<f64> {
    let s = s.trim();
    let v = if let Some(rest) = s.strip_prefix('+') {
        current + number(rest)?
    } else if let Some(rest) = s.strip_prefix('-') {
        current - number(rest)?
    } else {
        number(s)?
    };
    (v >= 0.0 && v <= MAX_COORD).then_some(v)
}

/// The dimensions Horizon has, by any of their names: `overworld`, `world`,
/// `minecraft:overworld`, `nether`, `world_nether`, `end`, `world_the_end`...
pub fn world(s: &str) -> Option<&'static str> {
    let s = s.to_ascii_lowercase();
    let s = s.strip_prefix("minecraft:").unwrap_or(&s);
    match s {
        "overworld" | "world" => Some("minecraft:overworld"),
        "the_nether" | "nether" | "world_nether" => Some("minecraft:the_nether"),
        "the_end" | "end" | "world_the_end" => Some("minecraft:the_end"),
        _ => None,
    }
}

pub const WORLDS: &str = "minecraft:overworld, minecraft:the_nether, minecraft:the_end";

/// Chunky's `Formatting.number`: `DecimalFormat("#.##")`.
pub fn fmt_number(v: f64) -> String {
    let s = format!("{:.2}", v);
    let s = s.trim_end_matches('0').trim_end_matches('.');
    if s == "-0" { "0".into() } else { s.into() }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numbers() {
        assert_eq!(number("200"), Some(200.0));
        assert_eq!(number("20c"), Some(320.0));
        assert_eq!(number("2R"), Some(1024.0));
        assert_eq!(number("1.5k"), Some(1500.0));
        assert_eq!(number("abc"), None);
        assert_eq!(radius("+100", 500.0), Some(600.0));
        assert_eq!(radius("-100", 500.0), Some(400.0));
        assert_eq!(radius("-600", 500.0), None);
        assert_eq!(radius("40000000", 0.0), None);
        assert_eq!(coordinate("-30000001"), None);
        assert_eq!(world("NETHER"), Some("minecraft:the_nether"));
        assert_eq!(world("minecraft:the_end"), Some("minecraft:the_end"));
        assert_eq!(world("mars"), None);
        assert_eq!(fmt_number(500.0), "500");
        assert_eq!(fmt_number(12.345), "12.35");
        assert_eq!(fmt_number(-0.001), "0");
    }
}
