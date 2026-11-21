//! Web: storefront, Mini App /tg, /admin, /api/*, webhook Midtrans/Sondev.
//! axum 0.8 + maud 0.27 (server-rendered HTML).
//!
//! Port `src/app/**` (versi TS):
//! - `/` storefront (katalog saja), `/tg` Mini App Telegram
//! - `/admin`, `/admin/tickets`, `/admin/tickets/{id}`
//! - `/api/products`, `/api/orders*`, `/api/admin/*`
//! - `/api/webhooks/midtrans`, `/api/webhooks/sondev`
//!
//! Satu binary menjalankan bot + web dalam satu proses (seperti
//! `scripts/vps-start.sh` di versi TS).

mod api;
mod auth;
mod pages;
mod settlement;
mod util;
mod views;

use std::sync::Arc;

use axum::routing::{get, post};
use axum::Router;
use tracing::info;

use crate::config::Config;
use crate::db::Db;
use crate::lib::payments::gopay::GopayClient;
use settlement::WebSettlement;

/// State bersama semua handler web.
#[derive(Clone)]
pub struct AppState {
    pub cfg: Config,
    pub db: Db,
    /// Client HTTP bersama (dipakai Snap Midtrans).
    pub http: reqwest::Client,
    /// Settlement nyata (orders/wallet/notify) untuk webhook Midtrans
    /// + tick GoBiz manual.
    pub settlement: Arc<WebSettlement>,
    /// Client GoBiz (sesi tersimpan di settings SQLite).
    pub gopay: Arc<GopayClient>,
}

pub async fn serve(cfg: Config, db: Db) -> anyhow::Result<()> {
    let settlement = WebSettlement::new(db.clone(), cfg.clone()).as_arc();
    let gopay = Arc::new(GopayClient::new(db.clone(), &cfg));
    let state = AppState {
        cfg: cfg.clone(),
        db,
        http: reqwest::Client::new(),
        settlement,
        gopay,
    };
    let app = Router::new()
        // Halaman
        .route("/", get(pages::storefront_page))
        .route("/tg", get(pages::tg_page_handler))
        .route("/admin", get(pages::admin_page))
        .route("/admin/tickets", get(pages::tickets_page))
        .route("/admin/tickets/{id}", get(pages::ticket_detail_page))
        // API publik
        .route("/api/products", get(api::list_products))
        .route("/api/orders", post(api::create_order))
        .route("/api/orders/{id}", get(api::get_order))
        .route("/api/orders/{id}/fulfill", post(api::fulfill_order_route))
        // Webhook
        .route("/api/webhooks/midtrans", post(api::midtrans_webhook))
        .route("/api/webhooks/sondev", post(api::sondev_webhook))
        // Admin: login/logout
        .route("/api/admin/login", post(api::admin_login))
        .route("/api/admin/logout", get(api::admin_logout))
        // Admin: tiket
        .route("/api/admin/tickets", get(api::admin_list_tickets))
        .route("/api/admin/tickets/{id}", get(api::admin_get_ticket))
        .route("/api/admin/tickets/{id}/reply", post(api::admin_reply_ticket))
        .route("/api/admin/tickets/{id}/close", post(api::admin_close_ticket))
        .route(
            "/api/admin/tickets/{id}/reopen",
            post(api::admin_reopen_ticket),
        )
        // Admin: harga produk
        .route(
            "/api/admin/products/{id}/price",
            post(api::admin_set_price),
        )
        // Admin: QRIS GoPay
        .route(
            "/api/admin/payments/qris",
            get(api::qris_status).post(api::qris_save),
        )
        .route(
            "/api/admin/payments/qris/payments",
            get(api::qris_payments),
        )
        .route("/api/admin/payments/qris/test", get(api::qris_test))
        // Admin: GoBiz / GoPay (jalur DORMANT — OTP diblokir server Gojek)
        .route(
            "/api/admin/payments/gopay/session",
            get(api::gopay_session),
        )
        .route("/api/admin/payments/gopay/otp", post(api::gopay_otp))
        .route("/api/admin/payments/gopay/verify", post(api::gopay_verify))
        .route("/api/admin/payments/gopay/poll", post(api::gopay_poll))
        .route(
            "/api/admin/payments/gopay/merchant",
            get(api::gopay_merchant),
        )
        .route(
            "/api/admin/payments/gopay/logout",
            post(api::gopay_logout),
        )
        .with_state(state);

    let addr = format!("0.0.0.0:{}", cfg.port);
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    info!("[web] Unicodes Digital mendengarkan di http://{addr}");
    axum::serve(listener, app).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Smoke test web layer saja (tanpa bot): jalankan `serve()` di port
    /// test, lalu cek semua halaman + API utama menjawab dengan benar.
    /// DB sementara di /tmp (dihapus dulu); tidak ada secret di sini.
    #[tokio::test]
    async fn web_smoke() {
        let db_path = "/tmp/web-smoke-test.db";
        let _ = std::fs::remove_file(db_path);
        let _ = std::fs::remove_file(format!("{db_path}-wal"));
        let _ = std::fs::remove_file(format!("{db_path}-shm"));
        std::env::set_var("DATABASE_PATH", db_path);
        std::env::set_var("PORT", "3223");
        std::env::set_var("ADMIN_PASSWORD", "test123");
        std::env::set_var("TELEGRAM_BOT_TOKEN", "");
        std::env::set_var("MIDTRANS_SERVER_KEY", "");
        std::env::set_var("SONDEV_API_SECRET", "");
        let cfg = Config::from_env().expect("config test");
        let db = crate::db::open(&cfg.database_path).expect("db test");
        let server = tokio::spawn(serve(cfg, db));
        tokio::time::sleep(std::time::Duration::from_millis(800)).await;
        let client = reqwest::Client::new();
        let base = "http://127.0.0.1:3223";

        // Storefront: katalog + harga Rp2.000.
        let home = client.get(base).send().await.expect("GET /").text().await.unwrap();
        assert!(home.contains("Muse AI Token 1B"), "storefront harus menampilkan Muse 1B");
        assert!(home.contains("Rp2.000"), "storefront harus menampilkan Rp2.000");
        assert!(home.contains("Segera Hadir"), "Gemini harus ber-badge Segera Hadir");
        assert!(!home.contains("Sondev"), "halaman buyer tidak boleh menyebut supplier");

        // API produk.
        let products: serde_json::Value = client
            .get(format!("{base}/api/products"))
            .send()
            .await
            .expect("GET /api/products")
            .json()
            .await
            .unwrap();
        assert_eq!(products["ok"], true);
        assert_eq!(products["products"].as_array().unwrap().len(), 3);
        assert_eq!(products["products"][0]["id"], "muse-token-1b");
        assert_eq!(products["products"][0]["retailPriceIdr"], 2000);

        // Mini App.
        let tg = client.get(format!("{base}/tg")).send().await.expect("GET /tg");
        assert_eq!(tg.status(), 200);
        assert!(tg.text().await.unwrap().contains("telegram-web-app.js"));

        // Admin tanpa login: form login.
        let admin = client
            .get(format!("{base}/admin"))
            .send()
            .await
            .expect("GET /admin")
            .text()
            .await
            .unwrap();
        assert!(admin.contains("Password admin"), "admin harus menampilkan form login");

        // API admin tanpa cookie -> 401.
        let no_auth = client
            .get(format!("{base}/api/admin/tickets"))
            .send()
            .await
            .expect("GET tickets tanpa cookie");
        assert_eq!(no_auth.status(), 401);

        // Login salah -> redirect ?error=1; login benar -> cookie + redirect.
        // Pakai client tanpa redirect-follow agar header Set-Cookie di respons
        // 303-nya bisa dibaca (client default akan mengikuti redirect ke /admin
        // dan Set-Cookie-nya hilang dari respons akhir).
        let no_redir = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .unwrap();
        let bad = no_redir
            .post(format!("{base}/api/admin/login"))
            .form(&[("password", "salah")])
            .send()
            .await
            .expect("POST login salah");
        assert_eq!(bad.status(), 303);
        assert!(bad.headers().get(axum::http::header::LOCATION).unwrap().to_str().unwrap().contains("error=1"), "login salah harus redirect ?error=1");
        let good = no_redir
            .post(format!("{base}/api/admin/login"))
            .form(&[("password", "test123")])
            .send()
            .await
            .expect("POST login benar");
        assert_eq!(good.status(), 303);
        let cookie = good
            .headers()
            .get(axum::http::header::SET_COOKIE)
            .expect("login benar harus set cookie")
            .to_str()
            .unwrap()
            .to_string();
        assert!(cookie.starts_with("unicodes_admin="), "cookie admin harus bernama unicodes_admin");
        assert!(cookie.contains("HttpOnly"), "cookie admin harus HttpOnly");
        let cookie_val = cookie.split(';').next().unwrap().to_string();

        // Dengan cookie: dashboard + tiket JSON.
        let dash = client
            .get(format!("{base}/admin"))
            .header("Cookie", &cookie_val)
            .send()
            .await
            .expect("GET /admin authed")
            .text()
            .await
            .unwrap();
        assert!(dash.contains("Total pengguna"), "dashboard harus menampilkan statistik");
        assert!(dash.contains("Saldo supplier"), "dashboard harus menampilkan saldo supplier");
        assert!(dash.contains("harga retail"), "dashboard harus menampilkan produk");
        let tickets: serde_json::Value = client
            .get(format!("{base}/api/admin/tickets"))
            .header("Cookie", &cookie_val)
            .send()
            .await
            .expect("GET tickets authed")
            .json()
            .await
            .unwrap();
        assert_eq!(tickets["ok"], true);
        assert_eq!(tickets["tickets"].as_array().unwrap().len(), 0);

        // Tiket detail 404 (JSON + halaman).
        let t404 = client
            .get(format!("{base}/api/admin/tickets/TKT-TIDAKADA"))
            .header("Cookie", &cookie_val)
            .send()
            .await
            .expect("GET tiket 404");
        assert_eq!(t404.status(), 404);
        let tpage = client
            .get(format!("{base}/admin/tickets/TKT-TIDAKADA"))
            .header("Cookie", &cookie_val)
            .send()
            .await
            .expect("GET halaman tiket 404")
            .text()
            .await
            .unwrap();
        assert!(tpage.contains("Tiket tidak ditemukan"));

        // Order: produk tidak dikenal -> 400; qty > 30 -> 400.
        let bad_order = client
            .post(format!("{base}/api/orders"))
            .json(&serde_json::json!({ "productId": "tidak-ada", "quantity": 1 }))
            .send()
            .await
            .expect("POST order produk salah");
        assert_eq!(bad_order.status(), 400);
        let big_order = client
            .post(format!("{base}/api/orders"))
            .json(&serde_json::json!({ "productId": "muse-token-1b", "quantity": 31 }))
            .send()
            .await
            .expect("POST order qty 31");
        assert_eq!(big_order.status(), 400);
        let big_body: serde_json::Value = big_order.json().await.unwrap();
        assert_eq!(big_body["error"], "Jumlah maksimal 30 per order.");

        // Order tidak dikenal -> 404.
        let o404 = client
            .get(format!("{base}/api/orders/UD-TIDAKADA"))
            .send()
            .await
            .expect("GET order 404");
        assert_eq!(o404.status(), 404);

        // Webhook Midtrans tanpa key -> 500; Sondev tanpa secret -> 500.
        let mw = client
            .post(format!("{base}/api/webhooks/midtrans"))
            .json(&serde_json::json!({}))
            .send()
            .await
            .expect("POST webhook midtrans");
        assert_eq!(mw.status(), 500);
        let sw = client
            .post(format!("{base}/api/webhooks/sondev"))
            .body("{}")
            .send()
            .await
            .expect("POST webhook sondev");
        assert_eq!(sw.status(), 500);

        // QRIS status tanpa konfigurasi.
        let qs: serde_json::Value = client
            .get(format!("{base}/api/admin/payments/qris"))
            .header("Cookie", &cookie_val)
            .send()
            .await
            .expect("GET qris status")
            .json()
            .await
            .unwrap();
        assert_eq!(qs["configured"], false);

        // GoPay session tanpa login.
        let gs: serde_json::Value = client
            .get(format!("{base}/api/admin/payments/gopay/session"))
            .header("Cookie", &cookie_val)
            .send()
            .await
            .expect("GET gopay session")
            .json()
            .await
            .unwrap();
        assert_eq!(gs["ok"], true);
        assert_eq!(gs["authenticated"], false);

        server.abort();
        let _ = std::fs::remove_file(db_path);
        let _ = std::fs::remove_file(format!("{db_path}-wal"));
        let _ = std::fs::remove_file(format!("{db_path}-shm"));
    }
}
