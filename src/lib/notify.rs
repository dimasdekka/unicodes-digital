//! Notifikasi Telegram ke pembeli saat order selesai/gagal — port `src/lib/notify.ts`.
//!
//! Dipakai jalur bot (bayar saldo) DAN poller QRIS — pembeli tidak perlu
//! mengecek /order terus-menerus. Dikirim via reqwest langsung ke Bot API
//! (tanpa teloxide) supaya bisa dipakai dari proses web maupun proses bot.
//! Gagal kirim = log saja, jangan pernah menggagalkan order.
//!
//! Perbedaan dari TS: TS membaca token/channel dari `config.ts` global.
//! Di Rust, semua fungsi menerima `&NotifyCtx` (dibangun dari `Config`
//! sekali di `main`) supaya tidak ada global tersembunyi.

use crate::catalog::get_catalog_product;
use crate::config::Config;
use crate::db::Db;
use crate::i18n::{rupiah, t};
use crate::lib::orders::StoreOrder;
use crate::lib::pricing::IDR_PER_USD;
use crate::lib::wallet::{get_user, tg_id_from_buyer_ref};

/// Semua yang dibutuhkan pengiriman notifikasi Telegram.
#[derive(Debug, Clone)]
pub struct NotifyCtx {
    /// Token bot toko (@unicodesdigital_bot).
    pub bot_token: String,
    /// Token bot news (@unicodesdigitalbot); fallback ke bot toko kalau kosong.
    pub news_bot_token: String,
    /// ID channel komunitas; kosong = channel belum dikonfigurasi.
    pub channel_id: String,
    /// ID admin Telegram; [0] dipakai `notify_admin`.
    pub admin_ids: Vec<i64>,
    /// Base URL Bot API — default `https://api.telegram.org` (seam Gap 4:
    /// tes mengisinya dengan mock server lokal; produksi tidak berubah).
    pub api_base: String,
}

impl NotifyCtx {
    pub fn from_config(cfg: &Config) -> Self {
        Self {
            bot_token: cfg.telegram_bot_token.clone(),
            news_bot_token: cfg.telegram_news_bot_token.clone(),
            channel_id: cfg.telegram_channel_id.clone(),
            admin_ids: cfg.telegram_admin_ids.clone(),
            api_base: "https://api.telegram.org".to_string(),
        }
    }
}

/// Kirim satu pesan HTML ke chat Telegram lewat Bot API.
/// Dipakai juga `support_tickets` (balasan admin ke user).
/// Mengembalikan Ok(true) kalau Telegram menjawab ok.
/// Base URL default api.telegram.org (lihat `send_telegram_html_with_base`).
pub async fn send_telegram_html(
    bot_token: &str,
    chat_id: i64,
    html: &str,
) -> Result<bool, String> {
    send_telegram_html_with_markup(bot_token, chat_id, html, None).await
}

/// Varian injeksi dari [`send_telegram_html`]: base URL Bot API diambil
/// dari `api_base` (seam Gap 4 — tes mengarahkannya ke mock lokal).
pub async fn send_telegram_html_with_base(
    api_base: &str,
    bot_token: &str,
    chat_id: i64,
    html: &str,
) -> Result<bool, String> {
    send_telegram_html_with_markup_and_base(api_base, bot_token, chat_id, html, None).await
}

pub async fn send_telegram_html_with_markup(
    bot_token: &str,
    chat_id: i64,
    html: &str,
    reply_markup: Option<serde_json::Value>,
) -> Result<bool, String> {
    send_telegram_html_with_markup_and_base(
        "https://api.telegram.org",
        bot_token,
        chat_id,
        html,
        reply_markup,
    )
    .await
}

/// Varian injeksi dari [`send_telegram_html_with_markup`].
pub async fn send_telegram_html_with_markup_and_base(
    api_base: &str,
    bot_token: &str,
    chat_id: i64,
    html: &str,
    reply_markup: Option<serde_json::Value>,
) -> Result<bool, String> {
    if bot_token.is_empty() {
        return Err("TELEGRAM_BOT_TOKEN belum diisi di env".to_string());
    }
    let client = reqwest::Client::new();
    let mut body = serde_json::json!({
        "chat_id": chat_id,
        "text": html,
        "parse_mode": "HTML",
    });
    if let Some(markup) = reply_markup {
        body["reply_markup"] = markup;
    }
    let res = client
        .post(format!(
            "{}/bot{bot_token}/sendMessage",
            api_base.trim_end_matches('/')
        ))
        .json(&body)
        .timeout(std::time::Duration::from_secs(15))
        .send()
        .await
        .map_err(|e| e.to_string())?;
    if res.status().is_success() {
        Ok(true)
    } else {
        Err(format!("Telegram API {}", res.status()))
    }
}

/// Samarkan tengah string: 4 karakter awal + •••• + 4 karakter akhir.
pub fn mask_middle(value: &str) -> String {
    let chars: Vec<char> = value.chars().collect();
    if chars.len() <= 8 {
        let head: String = chars.iter().take(2).collect();
        format!("{head}••••")
    } else {
        let head: String = chars.iter().take(4).collect();
        let tail: String = chars.iter().skip(chars.len() - 4).collect();
        format!("{head}••••{tail}")
    }
}

/// Posting "NEW ORDER" ke channel komunitas (bukti transaksi, ala Sondev
/// Store). HANYA order delivered. Yang diposting: ringkasan order saja —
/// TIDAK PERNAH item hasil (kode invite/token/link) atau target pembeli.
/// Buyer + Order ID disamarkan. Dedupe via tabel channel_posts: satu order
/// diposting sekali. Channel belum dikonfigurasi = diam-diam tidak posting.
/// Gagal kirim = log saja, jangan pernah menggagalkan order.
pub async fn notify_channel_order(ctx: &NotifyCtx, db: &Db, order: &StoreOrder) -> bool {
    if order.status.as_str() != "delivered" {
        return false;
    }
    if ctx.channel_id.is_empty() {
        return false;
    }
    let token = if !ctx.news_bot_token.is_empty() {
        &ctx.news_bot_token
    } else {
        &ctx.bot_token
    };
    if token.is_empty() {
        return false;
    }
    // Dedupe dulu (insert-or-ignore): sudah pernah diposting = lewati.
    let inserted = {
        let conn = match db.lock() {
            Ok(g) => g,
            Err(_) => return false,
        };
        match conn.execute(
            "INSERT OR IGNORE INTO channel_posts (order_id, posted_at) VALUES (?1, ?2)",
            rusqlite::params![order.id, crate::db::now_iso()],
        ) {
            Ok(changed) => changed > 0,
            Err(e) => {
                tracing::error!("[notify] channel dedupe gagal: {e}");
                return false;
            }
        }
    };
    if !inserted {
        return false;
    }
    let tg_id = tg_id_from_buyer_ref(order.buyer_ref.as_deref());
    let user = tg_id.and_then(|id| get_user(db, id));
    let buyer_raw = user
        .as_ref()
        .and_then(|u| u.username.clone())
        .or_else(|| user.as_ref().and_then(|u| u.first_name.clone()))
        .unwrap_or_default();
    let buyer = if buyer_raw.is_empty() {
        "—".to_string()
    } else {
        let head: String = buyer_raw.chars().take(2).collect();
        format!("@{head}••••")
    };
    let mut product_name = order.catalog_product_id.clone();
    let mut qty_unit = "item";
    if let Ok(p) = get_catalog_product(db, &order.catalog_product_id) {
        product_name = p.name.to_string();
        if order.catalog_product_id.starts_with("muse-token") {
            qty_unit = "B";
        }
    }
    // (total / 16500).toFixed(2).replace(/\.?0+$/, "") — persis TS.
    let usd_raw = format!("{:.2}", order.retail_total_idr as f64 / IDR_PER_USD as f64);
    let usd = usd_raw.trim_end_matches('0').trim_end_matches('.');
    let method = match order.paid_via.as_deref() {
        Some("wallet") => "Saldo",
        Some("qris") => "QRIS",
        _ => "—",
    };
    let payment = match order.paid_via.as_deref() {
        Some("wallet") => "Wallet Balance",
        Some("qris") => "QRIS",
        _ => "—",
    };
    let date_wib = {
        let dt = chrono::DateTime::parse_from_rfc3339(&order.created_at)
            .map(|d| d.with_timezone(&chrono::FixedOffset::east_opt(7 * 3600).unwrap()))
            .unwrap_or_else(|_| {
                chrono::Utc::now().with_timezone(&chrono::FixedOffset::east_opt(7 * 3600).unwrap())
            });
        dt.format("%d %b %Y, %H:%M").to_string()
    };
    let text = [
        "🎉 <b>NEW ORDER</b>".to_string(),
        "──────────────────".to_string(),
        "🛍️ <b>Status: DELIVERED</b> ⚡".to_string(),
        String::new(),
        format!("👤 <b>Buyer:</b> {buyer}"),
        format!("📦 <b>Product:</b> {product_name} ({}{qty_unit})", order.quantity),
        format!("🔢 <b>Quantity:</b> {}{qty_unit}", order.quantity),
        format!(
            "💰 <b>Total:</b> {} / ${usd}",
            rupiah(order.retail_total_idr)
        ),
        format!("💳 <b>Method:</b> {method}"),
        format!("🧾 <b>Payment:</b> {payment}"),
        format!("🆔 <b>Order ID:</b> {}", mask_middle(&order.id)),
        format!("📅 <b>Date:</b> {date_wib} WIB"),
        String::new(),
        "🚀 <b>Automated &amp; Instant sent by bot</b>".to_string(),
    ]
    .join("\n");
    let markup = serde_json::json!({
        "inline_keyboard": [[{ "text": "🛍️ Buy Now", "url": "https://t.me/unicodesdigital_bot" }]]
    });
    // channel_id disimpan sebagai string (bisa "-100..." atau "@nama").
    // Kirim sebagai string JSON apa adanya.
    let chat_id: serde_json::Value = match ctx.channel_id.parse::<i64>() {
        Ok(n) => serde_json::json!(n),
        Err(_) => serde_json::json!(ctx.channel_id),
    };
    let client = reqwest::Client::new();
    let res = client
        .post(format!(
            "{}/bot{token}/sendMessage",
            ctx.api_base.trim_end_matches('/')
        ))
        .json(&serde_json::json!({
            "chat_id": chat_id,
            "text": text,
            "parse_mode": "HTML",
            "reply_markup": markup,
        }))
        .timeout(std::time::Duration::from_secs(15))
        .send()
        .await;
    match res {
        Ok(r) if r.status().is_success() => true,
        Ok(r) => {
            tracing::error!("[notify] channel post gagal: {}", r.status());
            false
        }
        Err(e) => {
            tracing::error!("[notify] channel post gagal: {e}");
            false
        }
    }
}

pub async fn notify_buyer(ctx: &NotifyCtx, db: &Db, order: &StoreOrder) {
    let Some(tg_id) = tg_id_from_buyer_ref(order.buyer_ref.as_deref()) else {
        return; // order web / tanpa pembeli Telegram
    };
    if ctx.bot_token.is_empty() {
        return;
    }
    let user = get_user(db, tg_id);
    let lang = user.as_ref().map(|u| u.lang).unwrap_or(crate::lib::wallet::Lang::Id);
    let s = t(lang);
    let delivered = order.status.as_str() == "delivered";
    let mut lines: Vec<String> = vec![
        if delivered {
            match lang {
                crate::lib::wallet::Lang::Id => format!("🎉 <b>Order {} selesai!</b>", order.id),
                crate::lib::wallet::Lang::En => format!("🎉 <b>Order {} is done!</b>", order.id),
            }
        } else {
            match lang {
                crate::lib::wallet::Lang::Id => format!("❌ <b>Order {} gagal.</b>", order.id),
                crate::lib::wallet::Lang::En => format!("❌ <b>Order {} failed.</b>", order.id),
            }
        },
        String::new(),
        format!("Produk: {} × {}", order.catalog_product_id, order.quantity),
        format!("Total: {}", rupiah(order.retail_total_idr)),
        format!("Status: {}", (s.status_label)(order.status.as_str())),
    ];
    if !order.delivered_items.is_empty() {
        lines.push(String::new());
        lines.push("Hasil:".to_string());
        for item in &order.delivered_items {
            lines.push(format!("• {item}"));
        }
    }
    if let Some(link) = &order.result_link {
        lines.push(format!("🔗 Link: {link}"));
    }
    if let Some(err) = &order.error {
        lines.push(format!("⚠️ {err}"));
    }
    if delivered {
        lines.push(String::new());
        lines.push(match lang {
            crate::lib::wallet::Lang::Id => {
                "Terima kasih sudah belanja di Unicodes Digital! 💜".to_string()
            }
            crate::lib::wallet::Lang::En => {
                "Thanks for shopping at Unicodes Digital! 💜".to_string()
            }
        });
    }
    if let Err(e) =
        send_telegram_html_with_base(&ctx.api_base, &ctx.bot_token, tg_id, &lines.join("\n")).await
    {
        tracing::error!("[notify] gagal kirim ke {tg_id}: {e}");
    }
    // Order selesai -> bukti transaksi ke channel komunitas (sekali saja).
    if delivered {
        notify_channel_order(ctx, db, order).await;
    }
}

/// Kirim pesan ke admin pertama (TELEGRAM_ADMIN_IDS[0]).
/// Dipakai untuk hal yang butuh tindakan manusia — mis. refund manual QRIS
/// setelah fulfillment gagal. Gagal kirim = log saja, jangan pernah melempar
/// (pemanggil tidak boleh gagal hanya karena notifikasi).
pub async fn notify_admin(ctx: &NotifyCtx, text: &str) {
    if ctx.admin_ids.is_empty() || ctx.bot_token.is_empty() {
        return;
    }
    if let Err(e) = send_telegram_html_with_base(&ctx.api_base, &ctx.bot_token, ctx.admin_ids[0], text).await {
        tracing::error!("[notify] gagal kirim ke admin: {e}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db;
    use crate::lib::orders::{create_store_order, mark_paid, CreateStoreOrderInput, StoreOrderStatus};
    use std::sync::{Arc, Mutex};

    fn test_db() -> Db {
        db::open(":memory:").expect("db tes")
    }

    fn ctx() -> NotifyCtx {
        NotifyCtx {
            bot_token: "TEST-BOT-TOKEN".to_string(),
            news_bot_token: "TEST-NEWS-TOKEN".to_string(),
            channel_id: "-100999".to_string(),
            admin_ids: vec![807719771],
            api_base: "https://api.telegram.org".to_string(),
        }
    }

    /// Mock Bot API lokal: rekam semua sendMessage yang masuk.
    async fn start_mock_api(
    ) -> (String, Arc<Mutex<Vec<serde_json::Value>>>, tokio::task::JoinHandle<()>) {
        let sent: Arc<Mutex<Vec<serde_json::Value>>> = Arc::new(Mutex::new(Vec::new()));
        let state = sent.clone();
        let router = axum::Router::new()
            .fallback(
                |axum::extract::State(sent): axum::extract::State<
                    Arc<Mutex<Vec<serde_json::Value>>>,
                >,
                 uri: axum::http::Uri,
                 body: axum::body::Bytes| async move {
                    // notify.rs mem-post ke {api_base}/bot{token}/sendMessage.
                    assert!(uri.path().ends_with("/sendMessage"), "path tak dikenal: {}", uri.path());
                    let v: serde_json::Value =
                        serde_json::from_slice(&body).unwrap_or_default();
                    sent.lock().unwrap().push(v);
                    axum::Json(serde_json::json!({"ok": true, "result": true}))
                },
            )
            .with_state(state);
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind mock api");
        let base = format!("http://127.0.0.1:{}", listener.local_addr().unwrap().port());
        let handle = tokio::spawn(async move {
            axum::serve(listener, router).await.unwrap();
        });
        (base, sent, handle)
    }

    fn delivered_order(db: &Db) -> StoreOrder {
        let o = create_store_order(
            db,
            CreateStoreOrderInput {
                id: Some("UD-TEST123456".to_string()),
                catalog_product_id: "muse-token-1b".to_string(),
                quantity: Some(5),
                buyer_ref: Some("tg:111".to_string()),
                xiaomi_accounts: None,
                sheerid_url: None,
                target: Some("UVI0QD".to_string()),
                discount_idr: None,
                promo_code: None,
            },
        )
        .unwrap();
        mark_paid(db, &o.id).unwrap();
        // Tandai delivered langsung (tanpa supplier) untuk tes channel.
        // Guard dilepas dulu sebelum get_store_order() me-lock lagi
        // (Mutex tidak re-entrant).
        {
            let conn = db.lock().unwrap();
            conn.execute(
                "UPDATE store_orders SET status = 'delivered', paid_via = 'qris' WHERE id = ?1",
                rusqlite::params![o.id],
            )
            .unwrap();
        }
        crate::lib::orders::get_store_order(db, &o.id).unwrap()
    }

    #[test]
    fn mask_middle_masks_order_ids() {
        // S33: "UD-TEST123456" -> "UD-T••••3456" (4 awal + 4 akhir).
        assert_eq!(mask_middle("UD-TEST123456"), "UD-T••••3456");
        assert_eq!(mask_middle("UD-ABCDEFGHIJ"), "UD-A••••GHIJ");
        // <= 8 karakter: 2 awal + ••••.
        assert_eq!(mask_middle("budi"), "bu••••");
        assert_eq!(mask_middle("abcdefgh"), "ab••••");
        assert_eq!(mask_middle("abcdefghi"), "abcd••••fghi");
        assert_eq!(mask_middle(""), "••••");
    }

    #[tokio::test]
    async fn channel_post_skips_non_delivered() {
        let db = test_db();
        let mut ctx = ctx();
        ctx.channel_id = "-100999".to_string();
        let o = create_store_order(
            &db,
            CreateStoreOrderInput {
                id: Some("UD-PENDING1".to_string()),
                catalog_product_id: "muse-token-1b".to_string(),
                quantity: Some(1),
                buyer_ref: Some("tg:111".to_string()),
                xiaomi_accounts: None,
                sheerid_url: None,
                target: Some("UVI0QD".to_string()),
                discount_idr: None,
                promo_code: None,
            },
        )
        .unwrap();
        assert_eq!(o.status, StoreOrderStatus::AwaitingPayment);
        assert!(!notify_channel_order(&ctx, &db, &o).await);
    }

    #[tokio::test]
    async fn channel_post_skips_when_no_channel() {
        let db = test_db();
        let mut ctx = ctx();
        ctx.channel_id = String::new();
        let o = delivered_order(&db);
        assert_eq!(o.status, StoreOrderStatus::Delivered);
        assert!(!notify_channel_order(&ctx, &db, &o).await);
    }

    #[tokio::test]
    async fn channel_post_skips_when_no_token() {
        let db = test_db();
        let mut ctx = ctx();
        ctx.bot_token = String::new();
        ctx.news_bot_token = String::new();
        let o = delivered_order(&db);
        assert!(!notify_channel_order(&ctx, &db, &o).await);
    }

    #[test]
    fn notify_ctx_from_config() {
        let mut cfg = crate::config::Config::from_env().unwrap();
        cfg.telegram_bot_token = "BOT".to_string();
        cfg.telegram_news_bot_token = "NEWS".to_string();
        cfg.telegram_channel_id = "-1001".to_string();
        cfg.telegram_admin_ids = vec![1, 2];
        let ctx = NotifyCtx::from_config(&cfg);
        assert_eq!(ctx.bot_token, "BOT");
        assert_eq!(ctx.news_bot_token, "NEWS");
        assert_eq!(ctx.channel_id, "-1001");
        assert_eq!(ctx.admin_ids, vec![1, 2]);
        // Seam Gap 4: default tetap api.telegram.org.
        assert_eq!(ctx.api_base, "https://api.telegram.org");
    }

    #[tokio::test]
    async fn notify_buyer_posts_to_mock_api() {
        // Gap 4: notify_buyer order delivered -> satu sendMessage ke
        // pembeli (chat tg:111) berisi ID order; channel dimatikan di sini
        // supaya hanya jalur buyer yang teruji.
        let db = test_db();
        let (base, sent, server) = start_mock_api().await;
        let mut ctx = ctx();
        ctx.api_base = base;
        ctx.channel_id = String::new();
        let o = delivered_order(&db);
        notify_buyer(&ctx, &db, &o).await;
        let sent = sent.lock().unwrap();
        assert_eq!(sent.len(), 1, "notify_buyer harus mengirim tepat 1 pesan");
        assert_eq!(sent[0]["chat_id"], 111);
        let text = sent[0]["text"].as_str().unwrap_or("");
        assert!(text.contains("UD-TEST123456"), "pesan harus menyebut order: {text}");
        assert!(text.contains("selesai"), "pesan delivered harus menyebut selesai: {text}");
        server.abort();
    }

    #[tokio::test]
    async fn notify_channel_order_posts_masked_to_mock_api() {
        // Gap 4: notify_channel_order -> posting channel dengan Order ID
        // disamarkan (bukan ID lengkap); kedua kalinya di-dedupe (false).
        let db = test_db();
        let (base, sent, server) = start_mock_api().await;
        let mut ctx = ctx();
        ctx.api_base = base;
        let o = delivered_order(&db);
        assert!(notify_channel_order(&ctx, &db, &o).await);
        {
            let guard = sent.lock().unwrap();
            assert_eq!(guard.len(), 1, "channel post harus mengirim tepat 1 pesan");
            let text = guard[0]["text"].as_str().unwrap_or("").to_string();
            assert!(
                text.contains("UD-T••••3456"),
                "Order ID harus disamarkan: {text}"
            );
            assert!(
                !text.contains("UD-TEST123456"),
                "ID lengkap tidak boleh bocor ke channel: {text}"
            );
            assert!(text.contains("NEW ORDER"), "posting harus berjudul NEW ORDER: {text}");
        }
        // Dedupe: panggilan kedua tidak mengirim lagi.
        assert!(!notify_channel_order(&ctx, &db, &o).await);
        assert_eq!(sent.lock().unwrap().len(), 1);
        server.abort();
    }

    #[tokio::test]
    async fn notify_admin_posts_to_mock_api() {
        // Gap 4: notify_admin -> sendMessage ke admin pertama.
        let _db = test_db();
        let (base, sent, server) = start_mock_api().await;
        let mut ctx = ctx();
        ctx.api_base = base;
        notify_admin(&ctx, "Refund manual diperlukan untuk UD-TEST123456").await;
        let sent = sent.lock().unwrap();
        assert_eq!(sent.len(), 1);
        assert_eq!(sent[0]["chat_id"], 807719771);
        assert!(sent[0]["text"]
            .as_str()
            .unwrap_or("")
            .contains("Refund manual"));
        server.abort();
    }
}
