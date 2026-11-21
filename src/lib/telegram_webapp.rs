//! Validasi Telegram Web App initData (server-side) — port
//! `src/lib/telegram-webapp.ts`.
//!
//! Algoritma resmi Telegram (https://core.telegram.org/bots/webapps#validating-data-received-via-the-mini-app):
//!   1. Ambil semua field initData kecuali `hash`, urutkan alfabetis,
//!      gabung jadi data_check_string "key=<value>" dipisah "\n".
//!   2. secret_key = HMAC-SHA256(bot_token) dengan key "WebAppData".
//!   3. hash = HMAC-SHA256(data_check_string) dengan key secret_key (hex).
//!   4. Bandingkan dengan field `hash` + auth_date maks 1 hari.
//!
//! Dipakai POST /api/orders saat checkout dari Mini App — initData palsu
//! (mis. buyerRef tg orang lain) ditolak 401 di sana.

use hmac::{Hmac, Mac};
use serde::Deserialize;
use sha2::Sha256;

type HmacSha256 = Hmac<Sha256>;

/// Pengguna Telegram dari `initData` Web App — cerminan payload Telegram;
/// tidak semua field dibaca (dipakai validasi + tes).
#[allow(dead_code)]
#[derive(Debug, Clone, Deserialize)]
pub struct TelegramWebAppUser {
    pub id: i64,
    pub first_name: Option<String>,
    pub last_name: Option<String>,
    pub username: Option<String>,
    pub language_code: Option<String>,
    pub is_premium: Option<bool>,
}

/// Hasil validasi `initData` — `auth_date` dipakai tes (memastikan
/// nilai diteruskan); jalur produksi hanya memakai cabang `Ok`/`BadHash`.
#[allow(dead_code)]
#[derive(Debug, Clone)]
pub enum InitDataValidation {
    Ok {
        user: TelegramWebAppUser,
        auth_date: i64,
    },
    BadInitData,
    MissingHash,
    MissingAuthDate,
    Expired,
    BadHash,
    BadUser,
}

impl InitDataValidation {
    /// Alasan string persis TS: bad_init_data / missing_hash /
    /// missing_auth_date / expired / bad_hash / bad_user. None kalau valid.
    pub fn reason(&self) -> Option<&'static str> {
        match self {
            InitDataValidation::Ok { .. } => None,
            InitDataValidation::BadInitData => Some("bad_init_data"),
            InitDataValidation::MissingHash => Some("missing_hash"),
            InitDataValidation::MissingAuthDate => Some("missing_auth_date"),
            InitDataValidation::Expired => Some("expired"),
            InitDataValidation::BadHash => Some("bad_hash"),
            InitDataValidation::BadUser => Some("bad_user"),
        }
    }
}

/// auth_date tidak lebih dari 1 hari.
pub const MAX_AGE_SECONDS: i64 = 24 * 60 * 60;

/// Decode satu komponen `application/x-www-form-urlencoded`:
/// `+` → spasi, `%XX` → byte.
fn form_decode(s: &str) -> String {
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

/// Parse query string `k=v&k=v` menjadi pasangan (key, value) ter-decode.
fn parse_query(init_data: &str) -> Vec<(String, String)> {
    init_data
        .split('&')
        .filter(|p| !p.is_empty())
        .map(|pair| {
            let (k, v) = match pair.split_once('=') {
                Some((k, v)) => (k, v),
                None => (pair, ""),
            };
            (form_decode(k), form_decode(v))
        })
        .collect()
}

/// Perbandingan byte constant-time (akumulasi XOR): waktu eksekusi tidak
/// bergantung pada posisi byte pertama yang berbeda — cerminan
/// `crypto.timingSafeEqual` di TS. Mengembalikan false bila panjang
/// berbeda; aman di sini karena hash yang dibandingkan selalu 64 karakter
/// hex (panjang tetap, bukan secret), jadi early-return panjang tidak
/// membocorkan apa pun tentang nilai hash.
fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for i in 0..a.len() {
        diff |= a[i] ^ b[i];
    }
    diff == 0
}

pub fn validate_init_data(init_data: &str, bot_token: &str) -> InitDataValidation {
    if init_data.is_empty() {
        return InitDataValidation::BadInitData;
    }
    let params = parse_query(init_data);
    let hash = params
        .iter()
        .find(|(k, _)| k == "hash")
        .map(|(_, v)| v.clone());
    let Some(hash) = hash else {
        return InitDataValidation::MissingHash;
    };
    let auth_date_raw = params
        .iter()
        .find(|(k, _)| k == "auth_date")
        .map(|(_, v)| v.clone());
    let auth_date: i64 = match auth_date_raw.and_then(|s| s.parse().ok()) {
        Some(n) => n,
        None => return InitDataValidation::MissingAuthDate,
    };
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    if now - auth_date > MAX_AGE_SECONDS {
        return InitDataValidation::Expired;
    }
    // data_check_string: semua field kecuali hash, "key=<value>", urut alfabetis.
    let mut pairs: Vec<String> = params
        .iter()
        .filter(|(k, _)| k != "hash")
        .map(|(k, v)| format!("{k}={v}"))
        .collect();
    pairs.sort();
    let data_check_string = pairs.join("\n");

    let mut mac = HmacSha256::new_from_slice(b"WebAppData")
        .expect("HMAC-SHA256 menerima key apa pun");
    mac.update(bot_token.as_bytes());
    let secret_key = mac.finalize().into_bytes();

    let mut mac2 = HmacSha256::new_from_slice(&secret_key)
        .expect("HMAC-SHA256 menerima key apa pun");
    mac2.update(data_check_string.as_bytes());
    let computed = hex::encode(mac2.finalize().into_bytes());

    // Bandingkan constant-time (cerminan crypto.timingSafeEqual di TS):
    // hash input di-lowercase dulu, lalu XOR terakumulasi — bukan == yang
    // berhenti di byte pertama yang berbeda.
    let ok = constant_time_eq(computed.as_bytes(), hash.to_lowercase().as_bytes());
    if !ok {
        return InitDataValidation::BadHash;
    }
    let user_raw = params
        .iter()
        .find(|(k, _)| k == "user")
        .map(|(_, v)| v.clone())
        .unwrap_or_default();
    let user: TelegramWebAppUser = match serde_json::from_str(&user_raw) {
        Ok(u) => u,
        Err(_) => return InitDataValidation::BadUser,
    };
    InitDataValidation::Ok {
        user,
        auth_date,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hmac::{Hmac, Mac};
    use sha2::Sha256;

    const TEST_TOKEN: &str = "123456:TESTTOKEN";

    /// Bangun initData valid — rumus yang sama seperti harness TS (S15).
    fn build_init_data(token: &str, fields: &[(&str, &str)], auth_date: Option<i64>) -> String {
        let mut all: Vec<(String, String)> = fields
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        let auth = auth_date.unwrap_or_else(|| {
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs() as i64
        });
        all.push(("auth_date".to_string(), auth.to_string()));
        let mut pairs: Vec<String> = all.iter().map(|(k, v)| format!("{k}={v}")).collect();
        pairs.sort();
        let mut mac = Hmac::<Sha256>::new_from_slice(b"WebAppData").unwrap();
        mac.update(token.as_bytes());
        let secret = mac.finalize().into_bytes();
        let mut mac2 = Hmac::<Sha256>::new_from_slice(&secret).unwrap();
        mac2.update(pairs.join("\n").as_bytes());
        let hash = hex::encode(mac2.finalize().into_bytes());
        let mut params: Vec<(String, String)> = all;
        params.push(("hash".to_string(), hash));
        params
            .iter()
            .map(|(k, v)| format!("{}={}", urlencoding(k), urlencoding(v)))
            .collect::<Vec<_>>()
            .join("&")
    }

    fn user_json() -> String {
        serde_json::json!({"id": 111, "first_name": "Budi", "username": "budi"}).to_string()
    }

    #[test]
    fn valid_init_data_returns_user() {
        // S15: initData benar -> ok + user 111.
        let data = build_init_data(TEST_TOKEN, &[("query_id", "q1"), ("user", &user_json())], None);
        match validate_init_data(&data, TEST_TOKEN) {
            InitDataValidation::Ok { user, .. } => {
                assert_eq!(user.id, 111);
                assert_eq!(user.first_name.as_deref(), Some("Budi"));
                assert_eq!(user.username.as_deref(), Some("budi"));
            }
            other => panic!("harusnya Ok, dapat {:?}", other.reason()),
        }
    }

    #[test]
    fn tampered_init_data_is_bad_hash() {
        let data = build_init_data(TEST_TOKEN, &[("query_id", "q1"), ("user", &user_json())], None);
        let tampered = data.replace("Budi", "Bado");
        assert_eq!(
            validate_init_data(&tampered, TEST_TOKEN).reason(),
            Some("bad_hash")
        );
    }

    #[test]
    fn expired_init_data() {
        let old = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64
            - 90000; // 25 jam lalu (> 24 jam)
        let data = build_init_data(TEST_TOKEN, &[("query_id", "q1"), ("user", &user_json())], Some(old));
        assert_eq!(validate_init_data(&data, TEST_TOKEN).reason(), Some("expired"));
    }

    #[test]
    fn missing_hash() {
        let auth = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        let data = format!("query_id=q1&user={}&auth_date={auth}", urlencoding(&user_json()));
        assert_eq!(validate_init_data(&data, TEST_TOKEN).reason(), Some("missing_hash"));
    }

    fn urlencoding(s: &str) -> String {
        // Encode minimal untuk query string (dipakai tes missing_hash).
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

    #[test]
    fn constant_time_eq_behaves() {
        assert!(constant_time_eq(b"abc", b"abc"));
        assert!(constant_time_eq(b"", b""));
        assert!(!constant_time_eq(b"abc", b"abd")); // beda 1 byte
        assert!(!constant_time_eq(b"abc", b"ab")); // beda panjang
        assert!(!constant_time_eq(b"ab", b"abc"));
    }

    #[test]
    fn one_char_hash_change_is_bad_hash() {
        // Hash benar diubah SATU karakter hex -> tetap BadHash.
        let data = build_init_data(TEST_TOKEN, &[("query_id", "q1"), ("user", &user_json())], None);
        let hash = data.split("hash=").nth(1).unwrap().to_string();
        let mut chars: Vec<char> = hash.chars().collect();
        chars[0] = if chars[0] == 'a' { 'b' } else { 'a' };
        let bad: String = chars.into_iter().collect();
        let tampered = data.replace(&hash, &bad);
        assert_eq!(
            validate_init_data(&tampered, TEST_TOKEN).reason(),
            Some("bad_hash")
        );
    }

    #[test]
    fn correct_hash_still_ok() {
        // Hash benar (dibangun build_init_data) tetap Ok setelah
        // perbandingan diganti constant-time.
        let data = build_init_data(TEST_TOKEN, &[("query_id", "q1"), ("user", &user_json())], None);
        match validate_init_data(&data, TEST_TOKEN) {
            InitDataValidation::Ok { user, .. } => assert_eq!(user.id, 111),
            other => panic!("harusnya Ok, dapat {:?}", other.reason()),
        }
    }

    #[test]
    fn different_length_hash_is_bad_hash() {
        // Hash dipotong (beda panjang) -> ditolak BadHash.
        let data = build_init_data(TEST_TOKEN, &[("query_id", "q1"), ("user", &user_json())], None);
        let hash = data.split("hash=").nth(1).unwrap().to_string();
        let short = &hash[..32];
        let tampered = data.replace(&hash, short);
        assert_eq!(
            validate_init_data(&tampered, TEST_TOKEN).reason(),
            Some("bad_hash")
        );
        // Hash kepanjangan juga ditolak.
        let long = format!("{hash}00");
        let tampered_long = data.replace(&hash, &long);
        assert_eq!(
            validate_init_data(&tampered_long, TEST_TOKEN).reason(),
            Some("bad_hash")
        );
    }

    #[test]
    fn wrong_token_is_bad_hash() {
        let data = build_init_data(TEST_TOKEN, &[("query_id", "q1"), ("user", &user_json())], None);
        assert_eq!(
            validate_init_data(&data, "999:WRONG").reason(),
            Some("bad_hash")
        );
    }

    #[test]
    fn empty_and_missing_auth_date() {
        assert_eq!(validate_init_data("", TEST_TOKEN).reason(), Some("bad_init_data"));
        // Tanpa auth_date: hash dihitung tanpa field itu, tapi validasi
        // menolak sebelum cek hash.
        let data = build_init_data(TEST_TOKEN, &[("query_id", "q1"), ("user", &user_json())], None);
        let no_auth: String = data
            .split('&')
            .filter(|p| !p.starts_with("auth_date="))
            .collect::<Vec<_>>()
            .join("&");
        // Hash jadi tidak cocok karena auth_date hilang dari data_check_string
        // — tapi alasan yang dikembalikan harus missing_auth_date (dicek
        // duluan, sebelum hash).
        assert_eq!(
            validate_init_data(&no_auth, TEST_TOKEN).reason(),
            Some("missing_auth_date")
        );
    }

    #[test]
    fn bad_user_json() {
        let data = build_init_data(TEST_TOKEN, &[("query_id", "q1"), ("user", "bukan-json")], None);
        assert_eq!(validate_init_data(&data, TEST_TOKEN).reason(), Some("bad_user"));
    }
}
