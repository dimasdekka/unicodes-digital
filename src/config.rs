//! Konfigurasi: nama env var SAMA dengan `.env.local` versi TypeScript.
//! Tidak ada secret yang pernah ditulis ke repo atau ke memory.

use std::env;

#[derive(Debug, Clone)]
pub struct Config {
    pub database_path: String,
    pub port: u16,
    pub telegram_bot_token: String,
    pub telegram_news_bot_token: String,
    pub telegram_admin_ids: Vec<i64>,
    pub telegram_channel_id: String,
    pub telegram_channel_url: String,
    pub sondev_api_key: String,
    pub sondev_api_secret: String,
    pub sondev_base_url: String,
    pub lahastore_api_key: String,
    pub lahastore_base_url: String,
    pub midtrans_server_key: String,
    // Client Key dipakai Snap.js di browser (versi TS); jalur Rust memakai
    // redirect URL server-side, jadi field ini tidak dibaca — disimpan
    // supaya `.env` yang sama tetap valid.
    #[allow(dead_code)]
    pub midtrans_client_key: String,
    pub midtrans_is_production: bool,
    pub admin_password: String,
    pub app_url: String,
    pub bot_banner_file_id: String,
    pub product_banner_file_id_muse_token_1b: String,
    // Reward referral — TS membaca env ini; Rust memakai konstanta
    // `REFERRAL_REWARD_IDR` di wallet.rs (nilai sama, 1500). Gap paritas
    // yang diketahui, dicatat di PORTING.md.
    #[allow(dead_code)]
    pub referral_reward_idr: i64,
    pub idr_per_usd: i64,
    /// Interval polling pembayaran GoBiz/Midtrans, ms (env
    /// GOPAY_POLL_INTERVAL_MS) — default 30000, minimal 5000; nilai
    /// tidak valid / di bawah minimum jatuh ke default (cerminan
    /// `gopayPollIntervalMs()` di `src/lib/config.ts`).
    pub gopay_poll_interval_ms: u64,
    /// Nomor HP GoBiz merchant (env GOBIZ_PHONE) — default requestOtp.
    pub gobiz_phone: String,
}

fn env_or(key: &str, default: &str) -> String {
    env::var(key).unwrap_or_else(|_| default.to_string())
}

impl Config {
    pub fn from_env() -> anyhow::Result<Self> {
        Ok(Self {
            database_path: env_or("DATABASE_PATH", "dev.db"),
            port: env_or("PORT", "3100").parse().unwrap_or(3100),
            telegram_bot_token: env_or("TELEGRAM_BOT_TOKEN", ""),
            telegram_news_bot_token: env_or("TELEGRAM_NEWS_BOT_TOKEN", ""),
            telegram_admin_ids: env_or("TELEGRAM_ADMIN_IDS", "807719771")
                .split(',')
                .filter_map(|s| s.trim().parse().ok())
                .collect(),
            telegram_channel_id: env_or("TELEGRAM_CHANNEL_ID", ""),
            telegram_channel_url: env_or("TELEGRAM_CHANNEL_URL", ""),
            sondev_api_key: env_or("SONDEV_API_KEY", ""),
            sondev_api_secret: env_or("SONDEV_API_SECRET", ""),
            sondev_base_url: env_or(
                "SONDEV_BASE_URL",
                "https://api.sondevs.tech/api/v1",
            ),
            lahastore_api_key: env_or("LAHASTORE_API_KEY", ""),
            lahastore_base_url: env_or(
                "LAHASTORE_BASE_URL",
                "https://lahastore.up.railway.app/api/v1",
            ),
            midtrans_server_key: env_or("MIDTRANS_SERVER_KEY", ""),
            midtrans_client_key: env_or("MIDTRANS_CLIENT_KEY", ""),
            midtrans_is_production: env_or("MIDTRANS_IS_PRODUCTION", "false") == "true",
            admin_password: env_or("ADMIN_PASSWORD", ""),
            app_url: env_or("NEXT_PUBLIC_APP_URL", "http://localhost:3100"),
            bot_banner_file_id: env_or("BOT_BANNER_FILE_ID", ""),
            product_banner_file_id_muse_token_1b: env_or(
                "PRODUCT_BANNER_FILE_ID_MUSE_TOKEN_1B",
                "",
            ),
            referral_reward_idr: env_or("REFERRAL_REWARD_IDR", "1500")
                .parse()
                .unwrap_or(1500),
            idr_per_usd: env_or("IDR_PER_USD", "16500").parse().unwrap_or(16500),
            gopay_poll_interval_ms: env::var("GOPAY_POLL_INTERVAL_MS")
                .ok()
                .and_then(|v| v.parse::<u64>().ok())
                .filter(|v| *v >= 5000)
                .unwrap_or(30_000),
            gobiz_phone: env_or("GOBIZ_PHONE", ""),
        })
    }
}
