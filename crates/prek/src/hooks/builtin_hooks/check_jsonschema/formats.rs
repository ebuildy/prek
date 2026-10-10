//! Format checks matching upstream check-jsonschema.
//!
//! Upstream passes the `jsonschema` Draft 2020-12 `FormatChecker` for every draft. With the
//! dependencies it installs, that checker only enforces `date`, `email`, `idn-email`,
//! `idn-hostname`, `ipv4`, `ipv6`, `regex` and `uuid`. Upstream adds its own `date-time` and
//! `time`. Every other format (`uri`, `hostname`, `duration`, ...) is accepted unchecked, so
//! the crate's stricter built-in checks are replaced with checks that accept everything.

use std::net::{Ipv4Addr, Ipv6Addr};

use jsonschema::ValidationOptions;

use super::RegexVariant;

/// Format names upstream accepts in `--disable-formats`, plus `*` for all of them.
pub(super) const DISABLE_FORMATS_CHOICES: &[&str] = &[
    "*",
    "date",
    "date-time",
    "duration",
    "email",
    "hostname",
    "idn-email",
    "idn-hostname",
    "ipv4",
    "ipv6",
    "iri",
    "iri-reference",
    "json-pointer",
    "regex",
    "relative-json-pointer",
    "time",
    "uri",
    "uri-reference",
    "uri-template",
    "uuid",
];

/// Formats that upstream never checks with its default dependencies.
const UNCHECKED: &[&str] = &[
    "duration",
    "hostname",
    "iri",
    "iri-reference",
    "json-pointer",
    "relative-json-pointer",
    "uri",
    "uri-reference",
    "uri-template",
];

/// Applies upstream format behavior to the validator options.
pub(super) fn configure<'a>(
    mut options: ValidationOptions<'a>,
    disabled: &[String],
    regex_variant: RegexVariant,
) -> ValidationOptions<'a> {
    if disabled.iter().any(|name| name == "*") {
        return options.should_validate_formats(false);
    }
    options = options
        .should_validate_formats(true)
        .with_format("date", is_date)
        .with_format("date-time", is_date_time)
        .with_format("time", is_time)
        .with_format("email", is_email)
        .with_format("idn-email", is_email)
        .with_format("ipv4", is_ipv4)
        .with_format("ipv6", is_ipv6)
        .with_format("uuid", is_uuid);
    if regex_variant == RegexVariant::Python {
        options = options.with_format("regex", is_python_regex);
    }
    for name in UNCHECKED
        .iter()
        .copied()
        .chain(disabled.iter().map(String::as_str))
    {
        options = options.with_format(name, |_: &str| true);
    }
    options
}

fn is_email(value: &str) -> bool {
    value.contains('@')
}

fn is_ipv4(value: &str) -> bool {
    value.parse::<Ipv4Addr>().is_ok()
}

fn is_ipv6(value: &str) -> bool {
    value.parse::<Ipv6Addr>().is_ok()
}

/// Python's `UUID()` accepts the hex digits with any hyphens, and upstream then requires
/// hyphens at the canonical positions.
fn is_uuid(value: &str) -> bool {
    let bytes = value.as_bytes();
    let hyphens_ok = [8, 13, 18, 23]
        .iter()
        .all(|&index| bytes.get(index) == Some(&b'-'));
    let hex: Vec<u8> = bytes.iter().copied().filter(|&b| b != b'-').collect();
    hyphens_ok && hex.len() == 32 && hex.iter().all(u8::is_ascii_hexdigit)
}

/// `YYYY-MM-DD` with a real calendar day, like `date.fromisoformat` behind a strict regex.
fn is_date(value: &str) -> bool {
    let bytes = value.as_bytes();
    bytes.len() == 10 && bytes[4] == b'-' && bytes[7] == b'-' && valid_date(bytes)
}

/// RFC 3339 date-time, ported from upstream `formats/implementations/rfc3339.py`.
fn is_date_time(value: &str) -> bool {
    let bytes = value.as_bytes();
    if bytes.len() < 20 || bytes[4] != b'-' || bytes[7] != b'-' {
        return false;
    }
    if !matches!(bytes[10], b'T' | b't') || !valid_date(&bytes[..10]) {
        return false;
    }
    is_time(&value[11..])
}

/// `HH:MM:SS[.frac]` followed by `Z` or an offset, ported from upstream `iso8601_time.py`.
fn is_time(value: &str) -> bool {
    let bytes = value.as_bytes();
    if bytes.len() < 9 || bytes[2] != b':' || bytes[5] != b':' {
        return false;
    }
    let (Some(hour), Some(minute), Some(second)) = (
        two_digits(&bytes[0..2]),
        two_digits(&bytes[3..5]),
        two_digits(&bytes[6..8]),
    ) else {
        return false;
    };
    if hour > 23 || minute > 59 || second > 59 {
        return false;
    }
    let mut rest = &bytes[8..];
    if let [b'.' | b',', fraction @ ..] = rest {
        let digits = fraction.iter().take_while(|b| b.is_ascii_digit()).count();
        if digits == 0 {
            return false;
        }
        rest = &fraction[digits..];
    }
    match rest {
        [b'Z' | b'z'] => true,
        [b'+' | b'-', h1, h2, b':', m1, m2] => {
            matches!((two_digits(&[*h1, *h2]), two_digits(&[*m1, *m2])), (Some(h), Some(m)) if h <= 23 && m <= 59)
        }
        _ => false,
    }
}

/// Checks `YYYY-MM-DD` digits and day-of-month bounds, including leap years.
fn valid_date(bytes: &[u8]) -> bool {
    let year = match (two_digits(&bytes[0..2]), two_digits(&bytes[2..4])) {
        (Some(high), Some(low)) => u32::from(high) * 100 + u32::from(low),
        _ => return false,
    };
    let (Some(month), Some(day)) = (two_digits(&bytes[5..7]), two_digits(&bytes[8..10])) else {
        return false;
    };
    let max_day = match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if year % 4 == 0 && (year % 100 != 0 || year % 400 == 0) => 29,
        2 => 28,
        _ => return false,
    };
    year >= 1 && (1..=max_day).contains(&day)
}

fn two_digits(bytes: &[u8]) -> Option<u8> {
    match bytes {
        [a, b] if a.is_ascii_digit() && b.is_ascii_digit() => Some((a - b'0') * 10 + (b - b'0')),
        _ => None,
    }
}

/// Python `re` syntax. fancy-regex accepts JavaScript named groups `(?<name>...)`, which
/// Python rejects, so those are refused explicitly.
fn is_python_regex(value: &str) -> bool {
    !has_js_named_group(value) && fancy_regex::Regex::new(value).is_ok()
}

fn has_js_named_group(pattern: &str) -> bool {
    let bytes = pattern.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        match bytes[index] {
            b'\\' => index += 2,
            b'(' if bytes[index + 1..].starts_with(b"?<")
                && !matches!(bytes.get(index + 3), Some(b'=' | b'!')) =>
            {
                return true;
            }
            _ => index += 1,
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    // Ported from upstream tests/unit/formats/test_rfc3339.py.
    #[test]
    fn date_time_cases() {
        for good in [
            "2018-12-31T23:59:59Z",
            "2018-12-31t23:59:59Z",
            "2018-12-31t23:59:59z",
            "2018-12-31T23:59:59+00:00",
            "2018-12-31T23:59:59-00:00",
        ] {
            assert!(is_date_time(good), "{good}");
        }
        for bad in [
            "2018-12-31T23:59:59",
            "2018-12-31T23:59:59+00:00Z",
            "2018-12-31 23:59:59",
            "2020-13-01T00:00:00Z",
            "2020-00-01T00:00:00Z",
            "2020-01-00T00:00:00Z",
            "2020-01-32T00:00:00Z",
        ] {
            assert!(!is_date_time(bad), "{bad}");
        }
    }

    #[test]
    fn date_time_fractional_seconds() {
        for precision in 0..20 {
            let fraction = "7".repeat(precision.max(1));
            for offset in ["Z", "+00:00", "-00:00", "+23:59"] {
                let value = format!("2018-12-31T23:59:59.{fraction}{offset}");
                assert!(is_date_time(&value), "{value}");
                let value = format!("23:59:59.{fraction}{offset}");
                assert!(is_time(&value), "{value}");
            }
        }
    }

    #[test]
    fn date_time_day_bounds() {
        for (month, max_day) in [
            (1, 31),
            (3, 31),
            (4, 30),
            (5, 31),
            (6, 30),
            (7, 31),
            (8, 31),
            (9, 30),
            (10, 31),
            (11, 30),
        ] {
            assert!(is_date_time(&format!(
                "2020-{month:02}-{max_day:02}T00:00:00Z"
            )));
            assert!(!is_date_time(&format!(
                "2020-{month:02}-{:02}T00:00:00Z",
                max_day + 1
            )));
        }
        for (year, max_day) in [(2018, 28), (2016, 29), (2400, 29), (2500, 28)] {
            assert!(is_date_time(&format!("{year}-02-{max_day:02}T00:00:00Z")));
            assert!(!is_date_time(&format!(
                "{year}-02-{:02}T00:00:00Z",
                max_day + 1
            )));
        }
    }

    // Ported from upstream tests/unit/formats/test_time.py.
    #[test]
    fn time_cases() {
        for good in ["12:34:56Z", "23:59:59z", "23:59:59+00:00", "01:59:59-00:00"] {
            assert!(is_time(good), "{good}");
        }
        for bad in [
            "12:34:56",
            "23:59:60Z",
            "23:59:59+24:00",
            "01:59:59-00:60",
            "01:01:00:00:60",
        ] {
            assert!(!is_time(bad), "{bad}");
        }
    }

    #[test]
    fn python_format_checker_semantics() {
        assert!(is_email("a@"));
        assert!(is_email("@"));
        assert!(!is_email("nope"));
        assert!(!is_ipv4("1.2.3"));
        assert!(is_ipv4("1.2.3.4"));
        assert!(!is_ipv6("::g"));
        assert!(is_ipv6("::1"));
        assert!(!is_uuid("x"));
        assert!(is_uuid("123e4567-e89b-12d3-a456-426614174000"));
        assert!(!is_uuid("{123e4567-e89b-12d3-a456-426614174000}"));
        assert!(is_date("2021-10-28"));
        assert!(!is_date("2021-13-01"));
        assert!(!is_date("foo"));
        assert!(!is_date("0000-01-01"));
    }

    #[test]
    fn python_regex_rejects_js_named_groups() {
        assert!(is_python_regex("ab*c"));
        assert!(!is_python_regex("a(b*c"));
        assert!(!is_python_regex("a(?<captured>)bc"));
        assert!(is_python_regex("a(?P<captured>)bc"));
        assert!(is_python_regex("(?<=a)b"));
        assert!(is_python_regex(r"\(?<x"));
    }
}
