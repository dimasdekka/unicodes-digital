//! Util web: parsing body JSON-atau-form (route admin menerima keduanya,
//! seperti TS) + format tanggal WIB (tanpa chrono-tz: geser manual +7 jam
//! dari UTC, nama bulan Indonesia seperti `toLocaleString("id-ID")`).

use axum::http::HeaderMap;
use chrono::{Datelike, Timelike};

/// Percent-encode untuk nilai query string (`?error=...`).
pub fn url_encode(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

/// Decode satu komponen `application/x-www-form-urlencoded`:
/// `+` → spasi, `%XX` → byte.
pub fn url_decode(s: &str) -> String {
    let mut out = Vec::with_capacity(s.len());
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'+' => {
                out.push(b' ');
                i += 1;
            }
            b'%' if i + 2 < bytes.len() => {
                let hex = std::str::from_utf8(&bytes[i + 1..i + 3]).unwrap_or("");
                if let Ok(b) = u8::from_str_radix(hex, 16) {
                    out.push(b);
                    i += 3;
                } else {
                    out.push(bytes[i]);
                    i += 1;
                }
            }
            b => {
                out.push(b);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Body request yang sudah dipilah: JSON atau form-urlencoded.
#[derive(Debug, Clone)]
pub enum ParsedBody {
    Json(serde_json::Value),
    Form(Vec<(String, String)>),
}

impl ParsedBody {
    /// Ambil satu field (dari JSON string/number, atau dari form).
    pub fn get(&self, key: &str) -> Option<String> {
        match self {
            ParsedBody::Json(v) => v.get(key).and_then(|x| match x {
                serde_json::Value::String(s) => Some(s.clone()),
                serde_json::Value::Number(n) => Some(n.to_string()),
                _ => None,
            }),
            ParsedBody::Form(pairs) => pairs
                .iter()
                .find(|(k, _)| k == key)
                .map(|(_, v)| v.clone()),
        }
    }

    pub fn is_form(&self) -> bool {
        matches!(self, ParsedBody::Form(_))
    }
}

pub fn is_form_post(headers: &HeaderMap) -> bool {
    let ct = headers
        .get(axum::http::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    ct.contains("form-urlencoded") || ct.contains("multipart/form-data")
}

/// Pilah body berdasarkan Content-Type. Form multipart (upload file)
/// tidak dipakai route mana pun — diperlakukan sebagai form kosong.
pub fn parse_body(headers: &HeaderMap, bytes: &[u8]) -> ParsedBody {
    let ct = headers
        .get(axum::http::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    if ct.contains("application/json") {
        let v: serde_json::Value = serde_json::from_slice(bytes).unwrap_or_default();
        return ParsedBody::Json(v);
    }
    if ct.contains("form-urlencoded") {
        let text = String::from_utf8_lossy(bytes);
        let pairs = text
            .split('&')
            .filter(|p| !p.is_empty())
            .map(|pair| {
                let (k, v) = match pair.split_once('=') {
                    Some((k, v)) => (k, v),
                    None => (pair, ""),
                };
                (url_decode(k), url_decode(v))
            })
            .collect();
        return ParsedBody::Form(pairs);
    }
    // Tanpa Content-Type yang dikenal: coba JSON dulu, lalu form.
    if let Ok(v) = serde_json::from_slice::<serde_json::Value>(bytes) {
        if v.is_object() {
            return ParsedBody::Json(v);
        }
    }
    ParsedBody::Form(Vec::new())
}

const BULAN_ID: [&str; 12] = [
    "Jan", "Feb", "Mar", "Apr", "Mei", "Jun", "Jul", "Agu", "Sep", "Okt", "Nov", "Des",
];

fn parse_wib(iso: &str) -> Option<chrono::DateTime<chrono::FixedOffset>> {
    let dt = chrono::DateTime::parse_from_rfc3339(iso).ok()?;
    Some(dt.with_timezone(&chrono::FixedOffset::east_opt(7 * 3600)?))
}

/// "21 Nov 2026, 16:07" (WIB) — cerminan `toLocaleString("id-ID",
/// { timeZone: "Asia/Jakarta", day, month: "short", hour, minute })`.
/// ISO rusak -> kembalikan apa adanya.
pub fn wib_datetime(iso: &str) -> String {
    let Some(dt) = parse_wib(iso) else {
        return iso.to_string();
    };
    format!(
        "{:02} {} {:04} {:02}:{:02}",
        dt.day(),
        BULAN_ID[(dt.month() as usize - 1).min(11)],
        dt.year(),
        dt.hour(),
        dt.minute(),
    )
}

/// "21 Nov 2026" (WIB).
pub fn wib_date(iso: &str) -> String {
    let Some(dt) = parse_wib(iso) else {
        return iso.to_string();
    };
    format!(
        "{:02} {} {:04}",
        dt.day(),
        BULAN_ID[(dt.month() as usize - 1).min(11)],
        dt.year(),
    )
}
