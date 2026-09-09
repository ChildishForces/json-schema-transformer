// Format validation helpers embedded into generated Rust output.
//
// This file is NOT compiled as part of the json-schema-transformer crates; it
// is pasted verbatim (via include_str!) into generated code and must be fully
// self-contained, depending only on `std` and the `regex` crate.
//
// The predicates port the acceptance semantics of the Swift emitter's format
// wrapper types (crates/json-schema-transformer/src/emit/swift/*.swift),
// including their known quirks, so that generated Rust and generated Swift
// agree on what validates. Deliberate divergences from Swift are noted inline.
//
// All items are prefixed `_jst_` to avoid colliding with generated type names.

/// Compile-once regex cache. `pattern` must be a valid regex literal.
fn _jst_regex(
    cell: &'static std::sync::OnceLock<regex::Regex>,
    pattern: &str,
) -> &'static regex::Regex {
    cell.get_or_init(|| regex::Regex::new(pattern).expect("invalid built-in format regex"))
}

/// `email` — port of Swift `EmailAddress`: whole-string match of
/// `[^@\s]+@[^@\s]+\.[^@\s]+` (exactly one `@`, at least one dot after it,
/// no whitespace). Not a full RFC 5322 grammar.
pub fn _jst_is_email(s: &str) -> bool {
    static RE: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    _jst_regex(&RE, r"^[^@\s]+@[^@\s]+\.[^@\s]+$").is_match(s)
}

/// `hostname` — port of Swift `HostnameString`: non-empty, at most 253
/// characters, and every dot-separated label is 1-63 chars of ASCII
/// letters/digits/hyphen, not starting or ending with a hyphen.
///
/// Quirk preserved from Swift: `split(separator: ".")` omits empty
/// subsequences, so empty labels are skipped rather than rejected
/// (e.g. "a..b", "example.com." and "." are accepted).
pub fn _jst_is_hostname(s: &str) -> bool {
    if s.is_empty() || s.chars().count() > 253 {
        return false;
    }
    s.split('.').filter(|label| !label.is_empty()).all(|label| {
        label.chars().count() <= 63
            && label
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-')
            && !label.starts_with('-')
            && !label.ends_with('-')
    })
}

/// `ipv4` — port of Swift `IPv4Address`: four dot-separated decimal octets in
/// 0..=255, canonical form only (leading zeros, `+`/`-` signs rejected via the
/// round-trip check, exactly as in Swift).
///
/// Quirk preserved from Swift: empty segments are omitted by the split, so
/// e.g. "1..2.3.4" and "127.0.0.1." still yield four octets and are accepted.
/// (std's `Ipv4Addr::from_str` rejects those, so it is not used here.)
pub fn _jst_is_ipv4(s: &str) -> bool {
    let parts: Vec<&str> = s.split('.').filter(|p| !p.is_empty()).collect();
    parts.len() == 4
        && parts.iter().all(|p| match p.parse::<i64>() {
            Ok(n) => (0..=255).contains(&n) && n.to_string() == *p,
            Err(_) => false,
        })
}

/// `ipv6` — port of Swift `IPv6Address`: 1-8 colon-separated groups of 1-4 hex
/// digits, with `::` shorthand allowed (at most 7 explicit groups when
/// present). No zone-ID (`%eth0`) and no embedded-IPv4 (`::ffff:1.2.3.4`)
/// support, exactly like the Swift code. (std's `Ipv6Addr::from_str` accepts
/// embedded IPv4 and rejects some strings Swift accepts, so it is not used.)
///
/// Quirks preserved from Swift: only the first `::` is treated specially and
/// empty groups are omitted by the inner splits, so e.g. "1::2::3" and a
/// single stray leading/trailing ":" are accepted.
pub fn _jst_is_ipv6(s: &str) -> bool {
    let stripped = s.to_lowercase();
    let groups: Vec<&str> = if stripped.contains("::") {
        let (left, right) = match stripped.split_once("::") {
            Some(halves) => halves,
            None => return false,
        };
        let l: Vec<&str> = left.split(':').filter(|g| !g.is_empty()).collect();
        let r: Vec<&str> = right.split(':').filter(|g| !g.is_empty()).collect();
        if l.len() + r.len() > 7 {
            return false;
        }
        l.into_iter().chain(r).collect()
    } else {
        let g: Vec<&str> = stripped.split(':').filter(|g| !g.is_empty()).collect();
        if g.len() != 8 {
            return false;
        }
        g
    };
    groups
        .iter()
        .all(|g| (1..=4).contains(&g.len()) && g.bytes().all(|b| b.is_ascii_hexdigit()))
}

/// `ipv4` OR `ipv6` — port of Swift `IPAddress`.
pub fn _jst_is_ip(s: &str) -> bool {
    _jst_is_ipv4(s) || _jst_is_ipv6(s)
}

/// `time` — port of Swift `TimeString`: whole-string, case-insensitive match
/// of `HH:MM:SS[.fraction][Z|+HH:MM|-HH:MM]`. As in Swift, the offset is
/// OPTIONAL and no numeric range checks are performed (so "24:99:99" passes);
/// RFC 3339 full-time is stricter, but parity with Swift is intentional.
pub fn _jst_is_time(s: &str) -> bool {
    static RE: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    _jst_regex(
        &RE,
        r"(?i)^[0-9]{2}:[0-9]{2}:[0-9]{2}(\.[0-9]+)?(Z|[+-][0-9]{2}:[0-9]{2})?$",
    )
    .is_match(s)
}

/// `date` — RFC 3339 full-date (`YYYY-MM-DD`) with a real calendar check
/// (month 1-12, day valid for the month, Gregorian leap years). Swift maps
/// this format to Foundation `Date` and Kotlin does not validate it, so this
/// is the pure-predicate implementation of the intended RFC 3339 rule.
pub fn _jst_is_date(s: &str) -> bool {
    static RE: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    let re = _jst_regex(&RE, r"^([0-9]{4})-([0-9]{2})-([0-9]{2})$");
    let caps = match re.captures(s) {
        Some(c) => c,
        None => return false,
    };
    let year: i64 = caps[1].parse().unwrap_or(0);
    let month: i64 = caps[2].parse().unwrap_or(0);
    let day: i64 = caps[3].parse().unwrap_or(0);
    let leap = (year % 4 == 0 && year % 100 != 0) || year % 400 == 0;
    let days_in_month = match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 => {
            if leap {
                29
            } else {
                28
            }
        }
        _ => return false,
    };
    (1..=days_in_month).contains(&day)
}

/// `date-time` — RFC 3339 date-time as full-date `T`/`t` full-time, reusing
/// `_jst_is_date` and the Swift-ported `_jst_is_time`. Leap seconds (and any
/// other seconds value) are accepted and the offset is optional, inheriting
/// the Swift time semantics.
pub fn _jst_is_date_time(s: &str) -> bool {
    match s.split_once(['T', 't']) {
        Some((date, time)) => _jst_is_date(date) && _jst_is_time(time),
        None => false,
    }
}

/// `duration` — port of Swift `DurationString`: whole-string match of the
/// ISO 8601 duration form `P[nY][nM][nD][T[nH][nM][nS]]` (fraction allowed
/// only on seconds), rejecting the bare "P". As in Swift, week durations
/// ("P4W") are NOT accepted and vacuous forms like "PT" are accepted.
pub fn _jst_is_duration(s: &str) -> bool {
    static RE: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    s != "P"
        && _jst_regex(
            &RE,
            r"^P([0-9]+Y)?([0-9]+M)?([0-9]+D)?(T([0-9]+H)?([0-9]+M)?([0-9]+(\.[0-9]+)?S)?)?$",
        )
        .is_match(s)
}

/// `base64` — port of Swift `Base64String` (`Data(base64Encoded:)` with
/// default options): length a multiple of 4, standard alphabet
/// (`A-Za-z0-9+/`), at most two `=` padding characters and only at the end.
/// The empty string is valid. Trailing-bit canonicality is not checked,
/// matching Foundation.
pub fn _jst_is_base64(s: &str) -> bool {
    let bytes = s.as_bytes();
    if bytes.len() % 4 != 0 {
        return false;
    }
    let pad = bytes.iter().rev().take_while(|&&b| b == b'=').count();
    if pad > 2 {
        return false;
    }
    bytes[..bytes.len() - pad]
        .iter()
        .all(|&b| b.is_ascii_alphanumeric() || b == b'+' || b == b'/')
}

/// `uuid` — port of the Swift emitter's Foundation `UUID(uuidString:)`
/// acceptance rule: canonical 8-4-4-4-12 hexadecimal form, case-insensitive.
/// No version/variant restrictions.
pub fn _jst_is_uuid(s: &str) -> bool {
    static RE: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    _jst_regex(
        &RE,
        r"^[0-9A-Fa-f]{8}-[0-9A-Fa-f]{4}-[0-9A-Fa-f]{4}-[0-9A-Fa-f]{4}-[0-9A-Fa-f]{12}$",
    )
    .is_match(s)
}

/// `uri` — RFC 3986 absolute-URI shape: a `scheme:` (ALPHA followed by
/// ALPHA/DIGIT/`+`/`-`/`.`) then any run of unreserved / reserved / valid
/// percent-encoded characters. This is stricter than the Swift emitter's
/// Foundation `URL(string:)` (which also accepts relative references such as
/// "abc"); relative references, whitespace and backslashes are rejected here,
/// matching the JSON Schema `uri` (not `uri-reference`) intent.
pub fn _jst_is_uri(s: &str) -> bool {
    static RE: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    _jst_regex(
        &RE,
        r"^[A-Za-z][A-Za-z0-9+.\-]*:(?:%[0-9A-Fa-f]{2}|[A-Za-z0-9\-._~:/?#\[\]@!$&'()*+,;=])*$",
    )
    .is_match(s)
}
