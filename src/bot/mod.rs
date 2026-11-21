//! Bot Telegram: toko (@unicodesdigital_bot) + news (@unicodesdigitalbot).
//! teloxide 0.17, long polling.
//!
//! Paralel antar-chat, sekuensial dalam satu chat — SAMA seperti
//! `@grammyjs/runner` + `sequentialize` di versi TS. Di teloxide ini
//! dijamin Dispatcher: distribution function default-nya adalah
//! `|upd| upd.chat().map(|chat| chat.id)` (terverifikasi di source
//! teloxide 0.17: `src/dispatching/dispatcher.rs` "Default distribution
//! function" + `src/dispatching/distribution.rs` `default_distribution_function`).
//! Update dari chat yang sama diproses berurutan (aman buat session +
//! checkout), update dari chat berbeda berjalan paralel. Di sini fungsi
//! itu ditulis eksplisit supaya kontraknya kelihatan di kode, bukan
//! mengandalkan default diam-diam.
//!
//! `fixed-fetch.ts` TIDAK di-port (workaround Node 24 + grammY; teloxide
//! memakai reqwest native).

pub mod deps;
pub mod extract;
pub mod handlers;
pub mod menu_button;
pub mod news;
pub mod settlement;

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use teloxide::dispatching::UpdateFilterExt;
use teloxide::error_handlers::LoggingErrorHandler;
use teloxide::prelude::*;
use teloxide::types::BotCommand;

use crate::bot::settlement::BotSettlement;
use crate::config::Config;
use crate::db::Db;
use crate::lib::notify::NotifyCtx;
use crate::lib::payments::gopay::GopayClient;
use crate::lib::payments::midtrans_poller::{tick_midtrans_payments, MidtransPollCode};
use crate::lib::payments::qris_poller::{tick_qris_payments, QrisPollCode};
use crate::lib::session::SessionStore;
use handlers::BOT_COMMANDS;

/// State bersama semua handler bot toko.
pub struct BotState {
    pub cfg: Config,
    pub db: Db,
    /// Sesi checkout in-memory (restart = draft hilang, order di DB aman).
    pub sessions: SessionStore,
    /// Konteks notifikasi (token bot toko + news + channel + admin).
    pub notify_ctx: NotifyCtx,
    /// Cache hasil cek wajib-join channel: tg_id -> (joined, dicek kapan).
    pub join_cache: Mutex<HashMap<i64, (bool, Instant)>>,
    /// Throttle "Saya Sudah Bayar": referensi -> terakhir dicek.
    pub manual_check: Mutex<HashMap<String, Instant>>,
    /// Penjaga tick poller supaya tidak tumpang tindih (seperti TS).
    pub poller_running: AtomicBool,
    /// HTTP client bersama (Midtrans Snap + cek status).
    pub http_client: reqwest::Client,
    /// Client GoBiz (sesi + feed transaksi QRIS dibaca dari DB).
    pub gopay_client: GopayClient,
}

impl BotState {
    pub fn new(cfg: Config, db: Db) -> Self {
        let notify_ctx = NotifyCtx::from_config(&cfg);
        let gopay_client = GopayClient::new(db.clone(), &cfg);
        Self {
            cfg,
            db,
            sessions: SessionStore::new(),
            notify_ctx,
            join_cache: Mutex::new(HashMap::new()),
            manual_check: Mutex::new(HashMap::new()),
            poller_running: AtomicBool::new(false),
            http_client: reqwest::Client::new(),
            gopay_client,
        }
    }
}

/// Interval polling pembayaran (ms) — dari `Config` (env
/// `GOPAY_POLL_INTERVAL_MS`, default 30 detik, minimal 5 detik),
/// cerminan `gopayPollIntervalMs()` di `src/lib/config.ts`.
fn poll_interval_ms(cfg: &Config) -> u64 {
    cfg.gopay_poll_interval_ms
}

/// Poller pembayaran — SATU-SATUNYA tempat pembayaran dinyatakan lunas
/// (QRIS GoBiz di `qris-poller`, Midtrans di `midtrans-poller`). Jalan di
/// proses bot saja (bukan di web) supaya tidak ada dua poller yang rebutan
/// memproses transaksi yang sama. Midtrans yang belum dikonfigurasi diam
/// saja (tick mengembalikan ok).
async fn poll_once(state: &Arc<BotState>) {
    if state
        .poller_running
        .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
        .is_err()
    {
        return; // tick sebelumnya belum selesai — lewati jadwal ini
    }
    let result = poll_once_inner(state).await;
    state.poller_running.store(false, Ordering::SeqCst);
    if let Err(e) = result {
        tracing::error!("[poller] tick gagal: {e}");
    }
}

async fn poll_once_inner(state: &Arc<BotState>) -> anyhow::Result<()> {
    let settlement = BotSettlement::new(Arc::clone(state));
    let qris = tick_qris_payments(&state.db, &state.gopay_client, &settlement).await;
    if !qris.ok {
        let code = match qris.code {
            Some(QrisPollCode::AuthRequired) => "auth_required",
            Some(QrisPollCode::FeedError) => "feed_error",
            None => "error",
        };
        tracing::warn!(
            "[qris-poller] {code}: {}",
            qris.error.as_deref().unwrap_or("")
        );
    } else if qris.matched > 0 || qris.expired > 0 {
        tracing::info!(
            "[qris-poller] dicek {}, lunas {}, kedaluwarsa {}.",
            qris.checked,
            qris.matched,
            qris.expired
        );
    }
    let midtrans =
        tick_midtrans_payments(&state.http_client, &state.cfg, &state.db, &settlement).await;
    if !midtrans.ok {
        let code = match midtrans.code {
            Some(MidtransPollCode::StatusError) => "status_error",
            None => "error",
        };
        tracing::warn!(
            "[midtrans-poller] {code}: {}",
            midtrans.error.as_deref().unwrap_or("")
        );
    } else if midtrans.matched > 0 || midtrans.expired > 0 {
        tracing::info!(
            "[midtrans-poller] dicek {}, lunas {}, kedaluwarsa {}.",
            midtrans.checked,
            midtrans.matched,
            midtrans.expired
        );
    }
    Ok(())
}

pub async fn run_store_bot(cfg: Config, db: Db) -> anyhow::Result<()> {
    if cfg.telegram_bot_token.trim().is_empty() {
        anyhow::bail!("TELEGRAM_BOT_TOKEN belum diisi di .env.local");
    }
    let bot = Bot::new(cfg.telegram_bot_token.clone());

    // Daftarkan shortcut command (menu "/" di Telegram) sekali saat start.
    // Kegagalan tidak boleh mematikan bot — bot tetap jalan dengan handler
    // command-nya; daftar command bisa didaftarkan ulang di restart.
    let commands: Vec<BotCommand> = BOT_COMMANDS
        .iter()
        .map(|(command, description)| BotCommand {
            command: command.to_string(),
            description: description.to_string(),
        })
        .collect();
    if let Err(e) = bot.set_my_commands(commands).await {
        tracing::error!("[bot] setMyCommands gagal: {e}");
    }

    let state = Arc::new(BotState::new(cfg, db));

    // Poller pembayaran: tick pertama 10 detik setelah start, lalu tiap
    // interval (default 30 detik).
    let poller_state = Arc::clone(&state);
    let poller = tokio::spawn(async move {
        tokio::time::sleep(Duration::from_secs(10)).await;
        let mut interval = tokio::time::interval(Duration::from_millis(poll_interval_ms(
            &poller_state.cfg,
        )));
        loop {
            interval.tick().await;
            poll_once(&poller_state).await;
        }
    });

    let handler = dptree::entry()
        .branch(Update::filter_message().endpoint(handlers::handle_message))
        .branch(Update::filter_callback_query().endpoint(handlers::handle_callback));

    let mut dispatcher = Dispatcher::builder(bot, handler)
        .dependencies(dptree::deps![state])
        .error_handler(LoggingErrorHandler::with_custom_text("bot error"))
        // Paralel antar-chat, sekuensial dalam satu chat (lihat header modul).
        .distribution_function(|upd| upd.chat().map(|c| c.id))
        .enable_ctrlc_handler()
        .build();

    tracing::info!("Unicodes Digital bot jalan (long polling, dispatcher paralel)...");
    dispatcher.dispatch().await;
    poller.abort();
    Ok(())
}

pub async fn run_news_bot(cfg: Config, _db: Db) -> anyhow::Result<()> {
    news::run_news_bot(cfg).await
}

// ---------------------------------------------------------------------------
// Tes Gap 1: dispatcher ASLI melawan mock Bot API (teloxide 0.17)
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Bytes;
    use axum::extract::State;
    use axum::http::Uri;
    use axum::Json;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};
    use teloxide::dispatching::UpdateFilterExt;

    /// Mock Bot API Telegram lokal (127.0.0.1, port ephemeral).
    ///
    /// Seam yang dipakai: `Bot::set_api_url` (teloxide-core 0.13,
    /// `src/bot.rs` — `set_api_url(mut self, url: reqwest::Url) -> Self`;
    /// URL request dibangun `method_url` sebagai
    /// `{api_url}/bot{token}/{method}`). Jadi semua panggilan Bot API
    /// (getMe, getWebhookInfo, getUpdates, sendMessage) mendarat di mock
    /// ini, tanpa jaringan nyata.
    struct MockApi {
        /// Body sendMessage yang direkam.
        sent: Mutex<Vec<serde_json::Value>>,
        /// Berapa kali getUpdates dipanggil.
        updates_calls: AtomicUsize,
    }

    async fn mock_api(
        State(state): State<Arc<MockApi>>,
        uri: Uri,
        body: Bytes,
    ) -> Json<serde_json::Value> {
        let path = uri.path().to_string();
        if path.ends_with("/GetMe") {
            return Json(serde_json::json!({
                "ok": true,
                "result": {
                    "id": 999,
                    "is_bot": true,
                    "first_name": "Unicodes",
                    "username": "unicodesdigital_bot",
                    "can_join_groups": true,
                    "can_read_all_group_messages": false,
                    "supports_inline_queries": false,
                    "has_main_web_app": false,
                },
            }));
        }
        if path.ends_with("/GetWebhookInfo") {
            // url "" = webhook tidak dipasang -> dispatcher tidak memanggil
            // deleteWebhook (lihat teloxide polling::delete_webhook_if_setup).
            return Json(serde_json::json!({
                "ok": true,
                "result": {
                    "url": "",
                    "has_custom_certificate": false,
                    "pending_update_count": 0,
                    "ip_address": null,
                    "last_error_date": null,
                    "last_error_message": null,
                    "last_synchronization_error_date": null,
                    "max_connections": null,
                    "allowed_updates": null,
                },
            }));
        }
        if path.ends_with("/GetUpdates") {
            let n = state.updates_calls.fetch_add(1, Ordering::SeqCst);
            if n == 0 {
                // SATU update /start dari user palsu, lalu kosong.
                return Json(serde_json::json!({
                    "ok": true,
                    "result": [{
                        "update_id": 1,
                        "message": {
                            "message_id": 1,
                            "date": 1700000000,
                            "chat": {"id": 111, "type": "private"},
                            "from": {"id": 111, "is_bot": false, "first_name": "Budi", "username": "budi"},
                            "text": "/start",
                        },
                    }],
                }));
            }
            return Json(serde_json::json!({"ok": true, "result": []}));
        }
        if path.ends_with("/SendMessage") {
            let v: serde_json::Value = serde_json::from_slice(&body).unwrap_or_default();
            let text = v
                .get("text")
                .and_then(|t| t.as_str())
                .unwrap_or("")
                .to_string();
            state.sent.lock().unwrap().push(v);
            return Json(serde_json::json!({
                "ok": true,
                "result": {
                    "message_id": 10,
                    "date": 1700000000,
                    "chat": {"id": 111, "type": "private"},
                    "from": {"id": 999, "is_bot": true, "first_name": "Unicodes", "username": "unicodesdigital_bot"},
                    "text": text,
                },
            }));
        }
        if path.ends_with("/DeleteWebhook") {
            return Json(serde_json::json!({"ok": true, "result": true}));
        }
        Json(serde_json::json!({"ok": false, "error_code": 404, "description": "Not Found"}))
    }

    #[tokio::test]
    async fn start_command_end_to_end_via_mock_bot_api() {
        // Gap 1: dispatcher ASLI (bukan handler yang dipanggil langsung)
        // melawan mock Bot API lokal — /start harus menghasilkan satu
        // sendMessage berisi sambutan, dan baris user terbentuk di DB.
        let mock = Arc::new(MockApi {
            sent: Mutex::new(Vec::new()),
            updates_calls: AtomicUsize::new(0),
        });
        let app = axum::Router::new()
            .fallback(mock_api)
            .with_state(mock.clone());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind mock bot api");
        let port = listener.local_addr().unwrap().port();
        let server = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });

        let mut cfg = Config::from_env().unwrap();
        cfg.telegram_bot_token = "TEST-TOKEN-123".to_string();
        cfg.telegram_news_bot_token = String::new();
        cfg.telegram_channel_id = String::new(); // gate wajib-join mati
        cfg.telegram_channel_url = String::new();
        cfg.bot_banner_file_id = String::new(); // /start fallback ke teks
        let db = crate::db::open(":memory:").expect("db tes");
        let state = Arc::new(BotState::new(cfg, db.clone()));

        let bot = Bot::new("TEST-TOKEN-123")
            .set_api_url(reqwest::Url::parse(&format!("http://127.0.0.1:{port}/")).unwrap());

        // Dispatcher yang sama persis dengan run_store_bot, tanpa
        // setMyCommands / poller / ctrlc handler.
        let handler = dptree::entry()
            .branch(Update::filter_message().endpoint(handlers::handle_message))
            .branch(Update::filter_callback_query().endpoint(handlers::handle_callback));
        let mut dispatcher = Dispatcher::builder(bot, handler)
            .dependencies(dptree::deps![state])
            .error_handler(LoggingErrorHandler::with_custom_text("bot error"))
            .distribution_function(|upd| upd.chat().map(|c| c.id))
            .build();
        let dispatch_task = tokio::spawn(async move {
            dispatcher.dispatch().await;
        });

        // Tunggu sampai mock merekam sendMessage (maks 8 detik).
        let waited = tokio::time::timeout(Duration::from_secs(8), async {
            loop {
                if !mock.sent.lock().unwrap().is_empty() {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
        })
        .await;
        assert!(
            waited.is_ok(),
            "dispatcher tidak mengirim sendMessage dalam 8 detik"
        );
        dispatch_task.abort();
        server.abort();

        let sent = mock.sent.lock().unwrap();
        assert_eq!(sent.len(), 1, "harus tepat satu sendMessage untuk /start");
        let text = sent[0]
            .get("text")
            .and_then(|t| t.as_str())
            .unwrap_or("");
        assert!(
            text.contains("UNICODES DIGITAL"),
            "sambutan /start harus menyebut UNICODES DIGITAL: {text}"
        );
        assert_eq!(sent[0].get("chat_id"), Some(&serde_json::json!(111)));
        // Baris user terbentuk di DB (ensure_user_for di handle_message).
        let user = crate::lib::wallet::get_user(&db, 111);
        assert!(user.is_some(), "user tg:111 harus terbentuk di DB");
        assert_eq!(user.unwrap().first_name.as_deref(), Some("Budi"));
    }
}
