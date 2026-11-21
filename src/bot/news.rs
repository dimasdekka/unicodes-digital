//! Bot news Unicodes Digital (@unicodesdigitalbot) — MINIMAL.
//! Port `src/bot/news-bot.ts` (39 baris).
//!
//! Tugasnya cuma dua:
//!   1. Menjawab /start (dan pesan lain) dengan info: bot ini memposting
//!      semua info transaksi; belanja tetap di bot toko.
//!   2. (Bukan di sini) Posting berita transaksi ke channel — itu dilakukan
//!      `notify_channel_order()` memakai TOKEN bot ini (lihat
//!      `src/lib/notify.rs`), jadi posting tidak butuh proses polling.
//!      Long polling di sini HANYA supaya /start dibalas — bot publik yang diam
//!      kelihatan rusak.

use teloxide::prelude::*;
use teloxide::types::ParseMode;

use crate::config::Config;

const STORE_BOT_URL: &str = "https://t.me/unicodesdigital_bot";

pub fn info_text(cfg: &Config) -> String {
    let mut lines = vec![
        "📢 Unicodes Digital — News & Info Transaksi".to_string(),
        String::new(),
        "Bot ini memposting semua info transaksi: order masuk, order terkirim, promo, dan update stok. Bagian yang sensitif selalu disensor (nama pembeli disamarkan, kode hasil / target tidak pernah ditampilkan).".to_string(),
        String::new(),
        format!("🛍️ Belanja: {STORE_BOT_URL}"),
    ];
    let channel_url = cfg.telegram_channel_url.trim();
    if !channel_url.is_empty() {
        lines.push(format!("👥 Komunitas: {channel_url}"));
    }
    lines.join("\n")
}

async fn handle_message(bot: Bot, msg: Message, cfg: std::sync::Arc<Config>) -> anyhow::Result<()> {
    if msg.text().is_none() {
        return Ok(());
    }
    // Pesan lain: balas info yang sama (bot ini bukan toko — jangan diam).
    let text = info_text(&cfg);
    if let Err(e) = bot
        .send_message(msg.chat.id, text)
        .parse_mode(ParseMode::Html)
        .await
    {
        tracing::warn!("[news-bot] reply gagal: {e}");
    }
    Ok(())
}

/// Jalankan bot news (long polling). Kalau `TELEGRAM_NEWS_BOT_TOKEN`
/// kosong, kembali diam-diam — sama seperti `news-run.ts` (`process.exit(0)`).
pub async fn run_news_bot(cfg: Config) -> anyhow::Result<()> {
    let token = cfg.telegram_news_bot_token.trim().to_string();
    if token.is_empty() {
        tracing::info!("[news-bot] TELEGRAM_NEWS_BOT_TOKEN kosong — news bot tidak jalan.");
        return Ok(());
    }
    let bot = Bot::new(token);
    let cfg = std::sync::Arc::new(cfg);
    let handler = Update::filter_message().endpoint(handle_message);
    let mut dispatcher = Dispatcher::builder(bot, handler)
        .dependencies(dptree::deps![cfg])
        .error_handler(LoggingErrorHandler::with_custom_text("news bot error"))
        .enable_ctrlc_handler()
        .build();
    tracing::info!("Unicodes Digital news bot jalan (long polling)...");
    dispatcher.dispatch().await;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg() -> Config {
        let mut cfg = Config::from_env().unwrap();
        cfg.telegram_channel_url = String::new();
        cfg
    }

    #[test]
    fn info_text_matches_harness_s35() {
        // S35: /start dibalas info + link toko + sensor.
        let text = info_text(&cfg());
        assert!(text.contains("News & Info Transaksi"));
        assert!(text.contains("unicodesdigital_bot"));
        assert!(text.contains("disensor"));
        assert!(text.contains("https://t.me/unicodesdigital_bot"));
        assert!(!text.contains("Komunitas:"));
    }

    #[test]
    fn info_text_includes_channel_when_set() {
        let mut cfg = cfg();
        cfg.telegram_channel_url = "https://t.me/unicodesdigital".to_string();
        let text = info_text(&cfg);
        assert!(text.contains("👥 Komunitas: https://t.me/unicodesdigital"));
    }
}
