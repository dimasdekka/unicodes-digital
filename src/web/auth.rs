//! Auth admin — port `src/lib/admin-auth.ts` + `src/lib/admin-api.ts`
//! (versi TS).
//!
//! Login sederhana pakai cookie: cookie berisi sha256(ADMIN_PASSWORD),
//! httpOnly, 7 hari. Tanpa ADMIN_PASSWORD di env, halaman admin tidak bisa
//! dibuka siapa pun (fail closed).
//!
//! Perbedaan kecil dari TS: perbandingan cookie memakai perbandingan
//! constant-time (TS memakai `===`); perilaku login/logout identik.

use axum::http::HeaderMap;
use sha2::{Digest, Sha256};

use crate::config::Config;

/// Nama cookie admin (sama seperti TS).
pub const ADMIN_COOKIE: &str = "unicodes_admin";

/// Masa berlaku cookie login admin: 7 hari (sama seperti TS).
const COOKIE_MAX_AGE_SECS: i64 = 7 * 24 * 3600;

/// sha256 hex dari password.
pub fn hash_password(password: &str) -> String {
    hex::encode(Sha256::digest(password.as_bytes()))
}

/// sha256(ADMIN_PASSWORD), atau None kalau env belum diisi.
pub fn admin_password_hash(cfg: &Config) -> Option<String> {
    if cfg.admin_password.is_empty() {
        None
    } else {
        Some(hash_password(&cfg.admin_password))
    }
}

fn ct_eq(a: &str, b: &str) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for (x, y) in a.as_bytes().iter().zip(b.as_bytes().iter()) {
        diff |= x ^ y;
    }
    diff == 0
}

/// true kalau nilai cookie == sha256(ADMIN_PASSWORD).
pub fn is_admin_cookie_valid(cfg: &Config, value: Option<&str>) -> bool {
    let Some(value) = value else {
        return false;
    };
    let Some(expected) = admin_password_hash(cfg) else {
        return false;
    };
    ct_eq(value, &expected)
}

/// Baca satu cookie dari header `Cookie` mentah.
pub fn cookie_value(headers: &HeaderMap, name: &str) -> Option<String> {
    let header = headers.get(axum::http::header::COOKIE)?.to_str().ok()?;
    for part in header.split(';') {
        let part = part.trim();
        let Some((k, v)) = part.split_once('=') else {
            continue;
        };
        if k.trim() == name {
            return Some(v.trim().to_string());
        }
    }
    None
}

/// true kalau request membawa cookie admin yang valid.
/// Dipakai semua route `/api/admin/*` — tanpa cookie -> 401.
pub fn is_admin_request(cfg: &Config, headers: &HeaderMap) -> bool {
    is_admin_cookie_valid(cfg, cookie_value(headers, ADMIN_COOKIE).as_deref())
}

/// Nilai header `Set-Cookie` untuk login berhasil.
pub fn login_set_cookie_header(value: &str) -> String {
    format!("{ADMIN_COOKIE}={value}; HttpOnly; Path=/; Max-Age={COOKIE_MAX_AGE_SECS}; SameSite=Lax")
}

/// Nilai header `Set-Cookie` untuk logout (hapus cookie).
pub fn logout_set_cookie_header() -> String {
    format!("{ADMIN_COOKIE}=; HttpOnly; Path=/; Max-Age=0; SameSite=Lax")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg_with_password() -> Config {
        let mut cfg = Config::from_env().unwrap();
        cfg.admin_password = "test-admin-pw".to_string();
        cfg
    }

    fn headers_with_cookie(cookie: &str) -> HeaderMap {
        let mut h = HeaderMap::new();
        h.insert(
            axum::http::header::COOKIE,
            cookie.parse().unwrap(),
        );
        h
    }

    #[test]
    fn hash_is_sha256_hex_deterministic() {
        // sha256("test-admin-pw") — nilai ini dipakai harness TS (S25)
        // sebagai cookie admin.
        let h = hash_password("test-admin-pw");
        assert_eq!(h.len(), 64);
        assert!(h.chars().all(|c| c.is_ascii_hexdigit()));
        assert_eq!(h, hash_password("test-admin-pw"));
        assert_ne!(h, hash_password("test-admin-pw2"));
        // Nilai sha256("test-admin-pw") — dicek via sha256sum saat
        // penulisan tes; ini juga nilai cookie admin di harness TS (S25).
        assert_eq!(
            h,
            "bbbdb91bf1f3c5d5c377903f45c4bd03d83be1220e4eaccd0adea733b378eef8"
        );
    }

    #[test]
    fn admin_cookie_validates() {
        let cfg = cfg_with_password();
        let good = hash_password("test-admin-pw");
        assert!(is_admin_cookie_valid(&cfg, Some(&good)));
        assert!(!is_admin_cookie_valid(&cfg, Some("0".repeat(64).as_str())));
        assert!(!is_admin_cookie_valid(&cfg, Some("")));
        assert!(!is_admin_cookie_valid(&cfg, None));
        // Tanpa password di env: fail closed.
        let mut no_pw = Config::from_env().unwrap();
        no_pw.admin_password = String::new();
        assert!(admin_password_hash(&no_pw).is_none());
        assert!(!is_admin_cookie_valid(&no_pw, Some(&good)));
    }

    #[test]
    fn cookie_value_parses() {
        let h = headers_with_cookie("foo=bar; unicodes_admin=abc123; other=x");
        assert_eq!(cookie_value(&h, "unicodes_admin").as_deref(), Some("abc123"));
        assert_eq!(cookie_value(&h, "foo").as_deref(), Some("bar"));
        assert_eq!(cookie_value(&h, "tidak-ada"), None);
        let empty = HeaderMap::new();
        assert_eq!(cookie_value(&empty, "unicodes_admin"), None);
    }

    #[test]
    fn is_admin_request_needs_valid_cookie() {
        let cfg = cfg_with_password();
        let good = format!("{ADMIN_COOKIE}={}", hash_password("test-admin-pw"));
        assert!(is_admin_request(&cfg, &headers_with_cookie(&good)));
        assert!(!is_admin_request(&cfg, &headers_with_cookie("unicodes_admin=salah")));
        assert!(!is_admin_request(&cfg, &HeaderMap::new()));
    }

    #[test]
    fn login_logout_cookie_headers() {
        let login = login_set_cookie_header("abc123");
        assert!(login.starts_with("unicodes_admin=abc123;"));
        assert!(login.contains("HttpOnly"));
        assert!(login.contains("Path=/"));
        assert!(login.contains("Max-Age=604800")); // 7 hari
        assert!(login.contains("SameSite=Lax"));
        let logout = logout_set_cookie_header();
        assert!(logout.contains("unicodes_admin=;"));
        assert!(logout.contains("Max-Age=0"));
    }
}
