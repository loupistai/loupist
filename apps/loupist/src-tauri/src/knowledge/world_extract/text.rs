//! Deterministic text helpers the worker relied on: the character-trigram
//! fallback vector, Python's string normalizers, its datetime text forms and
//! the label order a two-item Python set had under `PYTHONHASHSEED=0`.

use chrono::{DateTime, FixedOffset, NaiveDate, NaiveDateTime, NaiveTime, TimeZone, Utc};
use unicode_normalization::UnicodeNormalization;

/// blake2b's salt is at most 16 bytes; a fixed one makes one spelling one vector.
const FALLBACK_SALT: &[u8] = b"loupist.fb1";

/// A non-empty vector with a non-zero component; cosine rejects anything else.
pub(crate) fn usable_vector(vector: &[f32]) -> bool {
    !vector.is_empty() && vector.iter().any(|value| *value != 0.0)
}

/// Deterministic unit vector for text the English model cannot encode.
///
/// Character trigrams of the normalized spelling share a bucket, so identical
/// and near-identical spellings land near each other. It carries no meaning
/// across scripts: a Korean name is not near its English translation.
pub(crate) fn lexical_fallback_vector(text: &str, dimension: usize) -> Vec<f32> {
    let dimension = dimension.max(1);
    let folded = casefold(&text.nfkc().collect::<String>());
    let normalized = folded.split_whitespace().collect::<Vec<_>>().join(" ");
    let padded: Vec<char> = format!(" {normalized} ").chars().collect();
    let mut accum = vec![0.0_f64; dimension];
    for start in 0..padded.len().saturating_sub(2) {
        let trigram: String = padded[start..start + 3].iter().collect();
        let digest = blake2b(trigram.as_bytes(), FALLBACK_SALT, 16);
        let bucket = u64::from_le_bytes(digest[..8].try_into().expect("eight bytes"));
        let index = (bucket % dimension as u64) as usize;
        accum[index] += if digest[8] & 1 == 1 { 1.0 } else { -1.0 };
    }
    let norm = accum.iter().map(|value| value * value).sum::<f64>().sqrt();
    if norm == 0.0 {
        let mut vector = vec![0.0_f32; dimension];
        vector[0] = 1.0;
        return vector;
    }
    accum.iter().map(|value| (value / norm) as f32).collect()
}

/// Replace an all-zero host vector with the trigram fallback, as the worker did.
pub(crate) fn replace_unusable(text: &str, vector: Vec<f32>, dimension: usize) -> Vec<f32> {
    if usable_vector(&vector) {
        vector
    } else {
        let width = if vector.is_empty() {
            dimension
        } else {
            vector.len()
        };
        lexical_fallback_vector(text, width)
    }
}

/// Python's `str.casefold()` for the characters that differ from lowercase in
/// practice; everything else is Unicode lowercase.
pub(crate) fn casefold(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for ch in text.chars() {
        match ch {
            'ß' | 'ẞ' => out.push_str("ss"),
            'ς' => out.push('σ'),
            'ſ' => out.push('s'),
            _ => out.extend(ch.to_lowercase()),
        }
    }
    out
}

/// Graphiti's `_normalize_string_exact`: lowercase, collapse whitespace, strip.
pub(crate) fn normalize_exact(text: &str) -> String {
    text.to_lowercase()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// Graphiti's `truncate_at_sentence` (lengths in characters).
pub(crate) fn truncate_at_sentence(text: &str, max_chars: usize) -> String {
    let chars: Vec<char> = text.chars().collect();
    if text.is_empty() || chars.len() <= max_chars {
        return text.to_string();
    }
    let truncated = &chars[..max_chars];
    // r'[.!?](?:\s|$)' over the truncated text: the last match's end.
    let mut last_end = None;
    let mut index = 0;
    while index < truncated.len() {
        if matches!(truncated[index], '.' | '!' | '?') {
            if index + 1 == truncated.len() {
                last_end = Some(index + 1);
            } else if truncated[index + 1].is_whitespace() {
                last_end = Some(index + 2);
                index += 1;
            }
        }
        index += 1;
    }
    match last_end {
        Some(end) => chars[..end]
            .iter()
            .collect::<String>()
            .trim_end()
            .to_string(),
        None => truncated.iter().collect::<String>().trim_end().to_string(),
    }
}

/// Python slicing `text[:limit]` by code points.
pub(crate) fn prefix_chars(text: &str, limit: usize) -> String {
    text.chars().take(limit).collect()
}

// --- Datetimes --------------------------------------------------------------

/// Python 3.12 `datetime.fromisoformat` for the forms models and the host send.
pub(crate) fn from_isoformat(text: &str) -> Option<DateTime<FixedOffset>> {
    let text = text.trim();
    if let Ok(parsed) = DateTime::parse_from_rfc3339(text) {
        return Some(parsed);
    }
    let (date_part, rest) = match text.find(['T', ' ']) {
        Some(index) if index >= 8 => (&text[..index], Some(&text[index + 1..])),
        _ => (text, None),
    };
    let date = NaiveDate::parse_from_str(date_part, "%Y-%m-%d")
        .or_else(|_| NaiveDate::parse_from_str(date_part, "%Y%m%d"))
        .ok()?;
    let Some(rest) = rest else {
        return Some(naive_utc(date.and_hms_opt(0, 0, 0)?, None));
    };
    let (time_part, offset) = split_offset(rest)?;
    let time = parse_time(time_part)?;
    Some(naive_utc(NaiveDateTime::new(date, time), offset))
}

fn naive_utc(value: NaiveDateTime, offset: Option<FixedOffset>) -> DateTime<FixedOffset> {
    match offset {
        Some(offset) => offset
            .from_local_datetime(&value)
            .single()
            .unwrap_or_else(|| Utc.from_utc_datetime(&value).fixed_offset()),
        // A naive value is read as UTC (Graphiti's ensure_utc).
        None => Utc.from_utc_datetime(&value).fixed_offset(),
    }
}

fn split_offset(text: &str) -> Option<(&str, Option<FixedOffset>)> {
    if let Some(time) = text.strip_suffix('Z').or_else(|| text.strip_suffix('z')) {
        return Some((time, FixedOffset::east_opt(0)));
    }
    if let Some(index) = text.rfind(['+', '-']) {
        let sign = if &text[index..index + 1] == "-" {
            -1
        } else {
            1
        };
        let digits: String = text[index + 1..].chars().filter(|c| *c != ':').collect();
        if digits.len() < 2 || !digits.chars().all(|c| c.is_ascii_digit()) {
            return None;
        }
        let hours: i32 = digits[..2].parse().ok()?;
        let minutes: i32 = if digits.len() >= 4 {
            digits[2..4].parse().ok()?
        } else {
            0
        };
        let seconds: i32 = if digits.len() >= 6 {
            digits[4..6].parse().ok()?
        } else {
            0
        };
        let offset = FixedOffset::east_opt(sign * (hours * 3600 + minutes * 60 + seconds))?;
        return Some((&text[..index], Some(offset)));
    }
    Some((text, None))
}

fn parse_time(text: &str) -> Option<NaiveTime> {
    for format in [
        "%H:%M:%S%.f",
        "%H:%M:%S",
        "%H:%M",
        "%H%M%S%.f",
        "%H%M%S",
        "%H%M",
        "%H",
    ] {
        if let Ok(time) = NaiveTime::parse_from_str(text, format) {
            return Some(time);
        }
    }
    None
}

/// Graphiti's `ensure_utc`.
pub(crate) fn to_utc(value: &DateTime<FixedOffset>) -> DateTime<FixedOffset> {
    value.with_timezone(&Utc).fixed_offset()
}

/// Python `datetime.isoformat()` for an aware value.
pub(crate) fn isoformat(value: &DateTime<FixedOffset>) -> String {
    python_datetime(value, 'T')
}

/// Python `str(datetime)` for an aware value.
pub(crate) fn python_str(value: &DateTime<FixedOffset>) -> String {
    python_datetime(value, ' ')
}

fn python_datetime(value: &DateTime<FixedOffset>, separator: char) -> String {
    let micros = value.timestamp_subsec_micros();
    let pattern = format!("%Y-%m-%d{separator}%H:%M:%S");
    let base = value.format(&pattern).to_string();
    let fraction = if micros == 0 {
        String::new()
    } else {
        format!(".{micros:06}")
    };
    let offset = value.offset().local_minus_utc();
    let sign = if offset < 0 { '-' } else { '+' };
    let absolute = offset.abs();
    let mut zone = format!("{sign}{:02}:{:02}", absolute / 3600, (absolute % 3600) / 60);
    if absolute % 60 != 0 {
        zone.push_str(&format!(":{:02}", absolute % 60));
    }
    format!("{base}{fraction}{zone}")
}

// --- Python set order -----------------------------------------------------

fn siphash13(bytes: &[u8]) -> u64 {
    let (k0, k1) = (0_u64, 0_u64);
    let mut v0 = k0 ^ 0x736f_6d65_7073_6575;
    let mut v1 = k1 ^ 0x646f_7261_6e64_6f6d;
    let mut v2 = k0 ^ 0x6c79_6765_6e65_7261;
    let mut v3 = k1 ^ 0x7465_6462_7974_6573;
    let round = |v0: &mut u64, v1: &mut u64, v2: &mut u64, v3: &mut u64| {
        *v0 = v0.wrapping_add(*v1);
        *v2 = v2.wrapping_add(*v3);
        *v1 = v1.rotate_left(13) ^ *v0;
        *v3 = v3.rotate_left(16) ^ *v2;
        *v0 = v0.rotate_left(32);
        *v2 = v2.wrapping_add(*v1);
        *v0 = v0.wrapping_add(*v3);
        *v1 = v1.rotate_left(17) ^ *v2;
        *v3 = v3.rotate_left(21) ^ *v0;
        *v2 = v2.rotate_left(32);
    };
    let mut chunks = bytes.chunks_exact(8);
    for chunk in &mut chunks {
        let word = u64::from_le_bytes(chunk.try_into().expect("eight bytes"));
        v3 ^= word;
        round(&mut v0, &mut v1, &mut v2, &mut v3);
        v0 ^= word;
    }
    let mut tail = [0_u8; 8];
    let remainder = chunks.remainder();
    tail[..remainder.len()].copy_from_slice(remainder);
    let last = ((bytes.len() as u64) << 56) | u64::from_le_bytes(tail);
    v3 ^= last;
    round(&mut v0, &mut v1, &mut v2, &mut v3);
    v0 ^= last;
    v2 ^= 0xff;
    for _ in 0..3 {
        round(&mut v0, &mut v1, &mut v2, &mut v3);
    }
    (v0 ^ v1) ^ (v2 ^ v3)
}

/// CPython's `hash(str)` with randomization off, for ASCII text.
fn python_hash(text: &str) -> u64 {
    let hash = siphash13(text.as_bytes());
    if hash as i64 == -1 {
        (-2_i64) as u64
    } else {
        hash
    }
}

/// `list({'Entity', name})` in a worker started with `PYTHONHASHSEED=0`.
///
/// Graphiti built a new node's labels from that set, and the order reached the
/// edge, dedupe and summary prompts. Non-ASCII names fall back to `Entity` first.
pub(crate) fn new_node_labels(type_name: &str) -> Vec<String> {
    if type_name == "Entity" {
        return vec!["Entity".into()];
    }
    if !type_name.is_ascii() {
        return vec!["Entity".into(), type_name.into()];
    }
    const MASK: u64 = 7;
    let mut table: [Option<&str>; 8] = [None; 8];
    for key in ["Entity", type_name] {
        let hash = python_hash(key);
        let mut perturb = hash;
        let mut index = hash & MASK;
        loop {
            match table[index as usize] {
                None => {
                    table[index as usize] = Some(key);
                    break;
                }
                Some(existing) if existing == key => break,
                Some(_) => {
                    perturb >>= 5;
                    index = (index.wrapping_mul(5).wrapping_add(1).wrapping_add(perturb)) & MASK;
                }
            }
        }
    }
    table.iter().flatten().map(|key| key.to_string()).collect()
}

// --- BLAKE2b ------------------------------------------------------------------

const BLAKE2B_IV: [u64; 8] = [
    0x6a09_e667_f3bc_c908,
    0xbb67_ae85_84ca_a73b,
    0x3c6e_f372_fe94_f82b,
    0xa54f_f53a_5f1d_36f1,
    0x510e_527f_ade6_82d1,
    0x9b05_688c_2b3e_6c1f,
    0x1f83_d9ab_fb41_bd6b,
    0x5be0_cd19_137e_2179,
];

const SIGMA: [[usize; 16]; 12] = [
    [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15],
    [14, 10, 4, 8, 9, 15, 13, 6, 1, 12, 0, 2, 11, 7, 5, 3],
    [11, 8, 12, 0, 5, 2, 15, 13, 10, 14, 3, 6, 7, 1, 9, 4],
    [7, 9, 3, 1, 13, 12, 11, 14, 2, 6, 5, 10, 4, 0, 15, 8],
    [9, 0, 5, 7, 2, 4, 10, 15, 14, 1, 11, 12, 6, 8, 3, 13],
    [2, 12, 6, 10, 0, 11, 8, 3, 4, 13, 7, 5, 15, 14, 1, 9],
    [12, 5, 1, 15, 14, 13, 4, 10, 0, 7, 6, 3, 9, 2, 8, 11],
    [13, 11, 7, 14, 12, 1, 3, 9, 5, 0, 15, 4, 8, 6, 2, 10],
    [6, 15, 14, 9, 11, 3, 0, 8, 12, 2, 13, 7, 1, 4, 10, 5],
    [10, 2, 8, 4, 7, 6, 1, 5, 15, 11, 9, 14, 3, 12, 13, 0],
    [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15],
    [14, 10, 4, 8, 9, 15, 13, 6, 1, 12, 0, 2, 11, 7, 5, 3],
];

fn compress(state: &mut [u64; 8], block: &[u8; 128], counter: u128, last: bool) {
    let mut message = [0_u64; 16];
    for (index, word) in message.iter_mut().enumerate() {
        *word = u64::from_le_bytes(block[index * 8..index * 8 + 8].try_into().expect("word"));
    }
    let mut v = [0_u64; 16];
    v[..8].copy_from_slice(state);
    v[8..].copy_from_slice(&BLAKE2B_IV);
    v[12] ^= counter as u64;
    v[13] ^= (counter >> 64) as u64;
    if last {
        v[14] = !v[14];
    }
    fn mix(v: &mut [u64; 16], a: usize, b: usize, c: usize, d: usize, x: u64, y: u64) {
        v[a] = v[a].wrapping_add(v[b]).wrapping_add(x);
        v[d] = (v[d] ^ v[a]).rotate_right(32);
        v[c] = v[c].wrapping_add(v[d]);
        v[b] = (v[b] ^ v[c]).rotate_right(24);
        v[a] = v[a].wrapping_add(v[b]).wrapping_add(y);
        v[d] = (v[d] ^ v[a]).rotate_right(16);
        v[c] = v[c].wrapping_add(v[d]);
        v[b] = (v[b] ^ v[c]).rotate_right(63);
    }
    for sigma in &SIGMA {
        mix(&mut v, 0, 4, 8, 12, message[sigma[0]], message[sigma[1]]);
        mix(&mut v, 1, 5, 9, 13, message[sigma[2]], message[sigma[3]]);
        mix(&mut v, 2, 6, 10, 14, message[sigma[4]], message[sigma[5]]);
        mix(&mut v, 3, 7, 11, 15, message[sigma[6]], message[sigma[7]]);
        mix(&mut v, 0, 5, 10, 15, message[sigma[8]], message[sigma[9]]);
        mix(&mut v, 1, 6, 11, 12, message[sigma[10]], message[sigma[11]]);
        mix(&mut v, 2, 7, 8, 13, message[sigma[12]], message[sigma[13]]);
        mix(&mut v, 3, 4, 9, 14, message[sigma[14]], message[sigma[15]]);
    }
    for index in 0..8 {
        state[index] ^= v[index] ^ v[index + 8];
    }
}

/// Unkeyed BLAKE2b (RFC 7693) with a salt, as `hashlib.blake2b(salt=...)`.
pub(crate) fn blake2b(input: &[u8], salt: &[u8], digest_size: usize) -> Vec<u8> {
    let mut state = BLAKE2B_IV;
    state[0] ^= 0x0101_0000 ^ digest_size as u64;
    let mut salt_block = [0_u8; 16];
    salt_block[..salt.len().min(16)].copy_from_slice(&salt[..salt.len().min(16)]);
    state[4] ^= u64::from_le_bytes(salt_block[..8].try_into().expect("salt"));
    state[5] ^= u64::from_le_bytes(salt_block[8..].try_into().expect("salt"));
    let mut counter: u128 = 0;
    let mut chunks = input.chunks(128).peekable();
    if input.is_empty() {
        compress(&mut state, &[0_u8; 128], 0, true);
    }
    while let Some(chunk) = chunks.next() {
        let mut block = [0_u8; 128];
        block[..chunk.len()].copy_from_slice(chunk);
        counter += chunk.len() as u128;
        compress(&mut state, &block, counter, chunks.peek().is_none());
    }
    state
        .iter()
        .flat_map(|word| word.to_le_bytes())
        .take(digest_size)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blake2b_matches_reference_vectors() {
        // RFC 7693 appendix A: BLAKE2b-512("abc").
        let digest = blake2b(b"abc", b"", 64);
        assert_eq!(
            hex::encode(digest),
            "ba80a53f981c4d0d6a2797b69f12f6e94c212f14685ac4b74b12bb6fdbffa2d17d87c5392aab792dc252d5de4533cc9518d38aa8dbf1925ab92386edd4009923"
        );
    }

    #[test]
    fn sentence_truncation_matches_graphiti() {
        assert_eq!(truncate_at_sentence("One. Two. Three", 9), "One. Two.");
        assert_eq!(truncate_at_sentence("abcdefghij", 4), "abcd");
        assert_eq!(truncate_at_sentence("short", 10), "short");
    }

    #[test]
    fn python_datetime_text_forms() {
        let value = from_isoformat("2026-03-01T09:30:00.120000+09:00").unwrap();
        assert_eq!(isoformat(&value), "2026-03-01T09:30:00.120000+09:00");
        assert_eq!(
            python_str(&to_utc(&value)),
            "2026-03-01 00:30:00.120000+00:00"
        );
        assert_eq!(
            isoformat(&from_isoformat("2026-03-01").unwrap()),
            "2026-03-01T00:00:00+00:00"
        );
        assert_eq!(
            isoformat(&from_isoformat("2026-03-01T00:00:00Z").unwrap()),
            "2026-03-01T00:00:00+00:00"
        );
        assert!(from_isoformat("March 2026").is_none());
    }
}
