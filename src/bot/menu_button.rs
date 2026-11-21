//! Tombol Menu bot Telegram (tombol biru "Menu" di pojok kiri bawah chat).
//! Port `src/bot/menu-button.ts` + `set-menu-button.ts` (versi TS).
//!
//! Target: Menu Button type web_app "Buka Toko" yang membuka Mini App /tg.
//!
//! JANGAN PANGGIL `set_web_app_menu_button()` SEBELUM DEPLOY SELESAI:
//! URL Web App harus domain PUBLIK (https://<domain>/tg). Selama
//! `NEXT_PUBLIC_APP_URL` masih localhost / belum diisi, fungsi ini sengaja
//! Err — tombol menu tidak boleh menunjuk ke URL yang tidak bisa dibuka
//! pembeli dari HP-nya.
//!
//! Setelah deploy: isi `NEXT_PUBLIC_APP_URL=https://<domain>` di env,
//! lalu jalankan sekali (fungsi ini idempotent, boleh diulang kapan saja).
//! Di versi TS ini CLI `npx tsx src/bot/set-menu-button.ts` —
//! di Rust belum ada subcommand CLI di `main.rs` (catat untuk koordinator:
//! perlu `cargo run -- set-menu-button` atau pemanggilan sekali jalan).

use teloxide::prelude::*;
use teloxide::types::{MenuButton, WebAppInfo};

use crate::config::Config;

/// URL Web App (`<app_url>/tg`), atau `None` kalau belum publik (dev lokal).
/// Cerminan `getWebAppUrl()` di TS.
pub fn get_web_app_url(app_url: &str) -> Option<String> {
    let base = app_url.trim().trim_end_matches('/');
    if base.is_empty() {
        return None;
    }
    // Belum publik (dev lokal) — sembunyikan tombol Web App.
    let lower = base.to_ascii_lowercase();
    let host = lower
        .strip_prefix("https://")
        .or_else(|| lower.strip_prefix("http://"))
        .unwrap_or(&lower);
    let host = host.split('/').next().unwrap_or(host);
    let host = host.split(':').next().unwrap_or(host);
    if host == "localhost" || host == "127.0.0.1" {
        return None;
    }
    Some(format!("{base}/tg"))
}

/// Set Menu Button bot jadi Web App "Buka Toko" (sekali jalan, idempotent).
/// Sengaja belum dipanggil dari mana pun: butuh subcommand CLI sekali jalan
/// di `main.rs` (lihat header modul) — sampai ada, fungsi ini menunggu.
#[allow(dead_code)]
pub async fn set_web_app_menu_button(cfg: &Config) -> anyhow::Result<String> {
    let url = get_web_app_url(&cfg.app_url).ok_or_else(|| {
        anyhow::anyhow!(
            "NEXT_PUBLIC_APP_URL belum menunjuk domain publik — isi dulu setelah deploy, lalu jalankan set-menu-button"
        )
    })?;
    let bot = Bot::new(cfg.telegram_bot_token.clone());
    let web_app = WebAppInfo {
        url: reqwest::Url::parse(&url)?,
    };
    bot.set_chat_menu_button()
        .menu_button(MenuButton::WebApp {
            text: "Buka Toko".to_string(),
            web_app,
        })
        .await?;
    Ok(format!("Menu Button -> {url}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn localhost_has_no_web_app_url() {
        assert_eq!(get_web_app_url("http://localhost:3100"), None);
        assert_eq!(get_web_app_url("http://127.0.0.1:3100/"), None);
        assert_eq!(get_web_app_url(""), None);
    }

    #[test]
    fn public_url_gets_tg_path() {
        assert_eq!(
            get_web_app_url("https://example.com").as_deref(),
            Some("https://example.com/tg")
        );
        assert_eq!(
            get_web_app_url("https://example.com/").as_deref(),
            Some("https://example.com/tg")
        );
    }

    #[tokio::test]
    async fn set_menu_button_rejects_localhost() {
        // S17: setWebAppMenuButton menolak sebelum ada domain publik.
        // (Tidak ada HTTP: ditolak di validasi URL, sebelum Bot dibuat.)
        let mut cfg = Config::from_env().unwrap();
        cfg.app_url = "http://localhost:3100".to_string();
        assert!(set_web_app_menu_button(&cfg).await.is_err());
        cfg.app_url = String::new();
        assert!(set_web_app_menu_button(&cfg).await.is_err());
    }
}

