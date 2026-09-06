//! Small utilities shared across the shell: percent-decoding, URI path
//! encoding and pseudo-random identifier generation (no extra crates).

/// Decode `%XX` escapes in a URI path segment sequence. `+` is left as-is
/// (this is path decoding, not form decoding).
pub fn percent_decode(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(bytes.len());
    let mut i = 0usize;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            let high = hex_value(bytes[i + 1]);
            let low = hex_value(bytes[i + 2]);
            if let (Some(h), Some(l)) = (high, low) {
                out.push(h * 16 + l);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn hex_value(c: u8) -> Option<u8> {
    match c {
        b'0'..=b'9' => Some(c - b'0'),
        b'a'..=b'f' => Some(c - b'a' + 10),
        b'A'..=b'F' => Some(c - b'A' + 10),
        _ => None,
    }
}

/// Percent-encode a filesystem path so it can be embedded as the `path`
/// component of `UriComponents` (VS Code `URI.revive` expects the internal,
/// encoded form). `/` is kept as a separator.
pub fn encode_uri_path(path: &str) -> String {
    let mut out = String::with_capacity(path.len());
    for &b in path.as_bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' => out.push(b as char),
            b'-' | b'_' | b'.' | b'~' | b'/' => out.push(b as char),
            _ => out.push_str(&format!("%{:02X}", b)),
        }
    }
    out
}

/// Generate a UUIDv4-shaped identifier from a xorshift PRNG seeded with
/// time + pid. Not cryptographically strong; used for machine/session ids
/// where uniqueness is all that matters.
pub fn random_uuid_v4() -> String {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0x1234_5678_9ABC_DEF0);
    let mut state: u64 = nanos ^ ((std::process::id() as u64) << 32) ^ 0x9E37_79B9_7F4A_7C15;
    if state == 0 {
        state = 0x853C_49E6_748F_EA9B;
    }

    let mut words = [0u32; 4];
    for word in words.iter_mut() {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        *word = (state >> 32) as u32;
    }

    // version 4 + variant bits
    words[2] = (words[2] & 0x0FFF_FFFF) | 0x4000_0000;
    words[3] = (words[3] & 0x3FFF_FFFF) | 0x8000_0000;

    format!(
        "{:08x}-{:04x}-{:04x}-{:04x}-{:04x}{:04x}{:04x}",
        words[0],
        words[1] >> 16,
        words[2] >> 16,
        words[3] >> 16,
        words[1] & 0xFFFF,
        words[2] & 0xFFFF,
        words[3] & 0xFFFF
    )
}

/// Current unix time formatted as `secs.nanos` for log lines.
pub fn unix_timestamp() -> String {
    let d = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    format!("{}.{:09}", d.as_secs(), d.subsec_nanos())
}

/// Wall-clock timestamp for logger files: `("YYYY-MM-DD HH:MM:SS", counter)`
/// mirroring the spdlog line format VS Code log files use
/// (`[2024-01-02 03:04:05.006]`).
pub fn format_log_timestamp() -> (String, u64) {
    let d = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    let secs = d.as_secs();
    let millis = d.subsec_millis() as u64;

    // Civil-from-days conversion (Howard Hinnant's algorithm).
    let days = (secs / 86_400) as i64;
    let secs_of_day = secs % 86_400;
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = if month <= 2 { y + 1 } else { y };

    (
        format!(
            "{:04}-{:02}-{:02} {:02}:{:02}:{:02}",
            year,
            month,
            day,
            secs_of_day / 3600,
            (secs_of_day % 3600) / 60,
            secs_of_day % 60
        ),
        millis,
    )
}
