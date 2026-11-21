//! Poller status Midtrans — port dari `src/lib/payments/midtrans-poller.ts`.
//!
//! SATU-SATUNYA tempat pembayaran Midtrans dinyatakan lunas LEWAT POLLING
//! (jalur kedua: webhook, route terpisah). Cara kerja: ambil pembayaran
//! Midtrans pending -> GET /v2/{order_id}/status -> lunas: jalankan jalur
//! yang SUDAH ADA (top-up -> kredit dompet; order -> tandai lunas ->
//! fulfill + notify pembeli). Gagal/ditolak/kedaluwarsa: order -> failed
//! (top-up dibiarkan pending sampai kedaluwarsa — tidak ada saldo yang masuk).
//! Dijalankan berkala dari proses bot (interval 30 detik).
//! `tick_midtrans_payments()` TIDAK PERNAH panic — semua kegagalan
//! dikembalikan sebagai hasil.
//!
//! Boundary ke kelompok lain: semua efek ke order/dompet/fulfill/notify
//! lewat trait [`PaymentSettlement`]. Implementasi nyata:
//! [`crate::bot::settlement::BotSettlement`] (bot) dan
//! [`crate::web::settlement::WebSettlement`] (web) — keduanya mendelegasikan
//! 7 metode umum ke [`crate::lib::settlement::CoreSettlement`].

use crate::config::Config;
use crate::db::Db;
use tracing::{error, info};

use super::midtrans::{
    expire_midtrans_payments, get_transaction_status, list_expired_pending_midtrans_payments,
    list_pending_midtrans_payments, map_midtrans_status, mark_midtrans_payment_failed,
    mark_midtrans_payment_paid, midtrans_configured, MidtransClient, MidtransTxStatus,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MidtransPollCode {
    StatusError,
}

#[derive(Debug, Clone)]
pub struct MidtransPollResult {
    pub ok: bool,
    /// Jumlah pembayaran pending yang dicek.
    pub checked: usize,
    /// Jumlah pembayaran yang lunas di tick ini.
    pub matched: usize,
    /// Jumlah pembayaran yang kedaluwarsa di tick ini.
    pub expired: usize,
    /// Pesan error (kalau ok == false).
    pub error: Option<String>,
    /// Kode error.
    pub code: Option<MidtransPollCode>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProcessMidtransOutcome {
    Paid,
    Failed,
    Pending,
    Ignored,
}

// ---------------------------------------------------------------------------
// Boundary settlement (kelompok orders / wallet / notify)
// ---------------------------------------------------------------------------

/// Efek samping penyelesaian pembayaran. Semua method menerima `&Db`
/// (koneksi yang sama) supaya kelompok orders/wallet bisa memakai transaksi
/// atomik mereka sendiri.
///
/// Semantik HARUS mengikuti TS (`orders.ts` / `wallet.ts`):
/// - `credit_topup`: port `markTopupPaid` — idempotent; Ok(true) kalau saldo
///   benar-benar masuk, Ok(false) kalau top-up sudah "paid"; Err kalau
///   top-up tidak ditemukan.
/// - `mark_order_paid`: port `setOrderPaidVia` + `markPaid` — klaim atomik
///   `UPDATE ... WHERE status='awaiting_payment'`; Ok(true) kalau order
///   berubah ke paid, Ok(false) kalau order sudah bukan awaiting_payment
///   (sudah diproses jalur lain).
/// - `mark_order_failed`: port `markOrderFailed` — HANYA dari status
///   processing/paid (dari awaiting_payment = no-op, seperti TS!).
///   Dipakai poller saat Midtrans menjawab deny/cancel/expire.
/// - `cancel_order_expired`: port `cancelOrderForExpiredPayment` — hanya
///   dari awaiting_payment.
/// - `order_status`: status order saat ini (None kalau order tidak ada).
/// - `order_buyer_tg_id`: tg id pembeli dari `buyer_ref` format `tg:<id>`
///   (port `tgIdFromBuyerRef`; None kalau bukan format itu / order tak ada).
/// - `maybe_reward_referrer`: port `maybeRewardReferrer` — reward referral
///   sekali untuk order berbayar pertama.
/// - `spawn_fulfill_and_notify`: jalankan `fulfillOrder(order_id)` di
///   background lalu `notifyBuyer` — JANGAN tahan tick (fulfillment bisa
///   ~10 menit).
pub trait PaymentSettlement: Send + Sync {
    fn credit_topup(&self, db: &Db, topup_id: &str) -> anyhow::Result<bool>;
    fn mark_order_paid(&self, db: &Db, order_id: &str, paid_via: &str) -> anyhow::Result<bool>;
    fn mark_order_failed(&self, db: &Db, order_id: &str, reason: &str) -> anyhow::Result<()>;
    fn cancel_order_expired(
        &self,
        db: &Db,
        order_id: &str,
        reason: &str,
    ) -> anyhow::Result<()>;
    fn order_status(&self, db: &Db, order_id: &str) -> anyhow::Result<Option<String>>;
    fn order_buyer_tg_id(&self, db: &Db, order_id: &str) -> anyhow::Result<Option<i64>>;
    fn maybe_reward_referrer(&self, db: &Db, referred_tg_id: i64) -> anyhow::Result<bool>;
    fn spawn_fulfill_and_notify(&self, order_id: String);
}

// ---------------------------------------------------------------------------
// Transisi tunggal (dipakai poller DAN webhook)
// ---------------------------------------------------------------------------

/// Proses satu hasil status Midtrans untuk satu referensi (dipakai poller
/// DAN webhook — satu-satunya tempat transisi lunas/gagal Midtrans).
pub async fn process_midtrans_result(
    db: &Db,
    settlement: &dyn PaymentSettlement,
    order_id: &str,
    tx_status: &str,
) -> ProcessMidtransOutcome {
    let mapped = map_midtrans_status(tx_status);
    if mapped == MidtransTxStatus::Pending {
        return ProcessMidtransOutcome::Pending;
    }
    if mapped == MidtransTxStatus::Paid {
        let claimed = match mark_midtrans_payment_paid(db, order_id) {
            Ok(v) => v,
            Err(e) => {
                error!("[midtrans-poller] tandai lunas {order_id} gagal: {e}");
                return ProcessMidtransOutcome::Ignored;
            }
        };
        if !claimed {
            return ProcessMidtransOutcome::Ignored; // sudah diproses
        }
        if let Some(topup_id) = order_id.strip_prefix("TOPUP-") {
            let _ = topup_id;
            match settlement.credit_topup(db, order_id) {
                Ok(credited) => {
                    if credited {
                        info!("[midtrans-poller] top-up {order_id} lunas — saldo masuk.");
                    }
                }
                Err(e) => {
                    error!("[midtrans-poller] top-up {order_id} tidak ditemukan (sudah ditandai lunas): {e}");
                    return ProcessMidtransOutcome::Ignored;
                }
            }
            return ProcessMidtransOutcome::Paid;
        }
        if order_id.starts_with("UD-") {
            let status = match settlement.order_status(db, order_id) {
                Ok(s) => s,
                Err(e) => {
                    error!("[midtrans-poller] baca order {order_id} gagal: {e}");
                    return ProcessMidtransOutcome::Ignored;
                }
            };
            let Some(status) = status else {
                error!("[midtrans-poller] order {order_id} tidak ditemukan (sudah ditandai lunas).");
                return ProcessMidtransOutcome::Ignored;
            };
            if status != "awaiting_payment" {
                return ProcessMidtransOutcome::Ignored; // sudah dibayar/diproses
            }
            match settlement.mark_order_paid(db, order_id, "midtrans") {
                Ok(true) => {}
                Ok(false) => return ProcessMidtransOutcome::Ignored,
                Err(e) => {
                    error!("[midtrans-poller] tandai order {order_id} lunas gagal: {e}");
                    return ProcessMidtransOutcome::Ignored;
                }
            }
            if let Ok(Some(tg_id)) = settlement.order_buyer_tg_id(db, order_id) {
                if let Err(e) = settlement.maybe_reward_referrer(db, tg_id) {
                    error!("[midtrans-poller] reward referral {order_id} gagal: {e}");
                }
            }
            // Fulfillment jalan di background (bisa ~10 menit) — jangan tahan tick.
            settlement.spawn_fulfill_and_notify(order_id.to_string());
            info!("[midtrans-poller] order {order_id} lunas — fulfill dimulai.");
            return ProcessMidtransOutcome::Paid;
        }
        return ProcessMidtransOutcome::Ignored;
    }
    // failed (deny/cancel/expire/failure/dll)
    let claimed = match mark_midtrans_payment_failed(db, order_id) {
        Ok(v) => v,
        Err(e) => {
            error!("[midtrans-poller] tandai gagal {order_id} gagal: {e}");
            return ProcessMidtransOutcome::Ignored;
        }
    };
    if !claimed {
        return ProcessMidtransOutcome::Ignored; // sudah diproses
    }
    if order_id.starts_with("UD-") {
        if let Ok(Some(status)) = settlement.order_status(db, order_id) {
            if status == "awaiting_payment" {
                // Catatan: markOrderFailed TS no-op dari awaiting_payment
                // (hanya processing/paid yang diubah) — order baru benar-benar
                // gagal saat expiry sweep membatalkannya. Panggil tetap
                // dilakukan supaya perilaku sama persis dengan TS.
                if let Err(e) = settlement.mark_order_failed(
                    db,
                    order_id,
                    &format!("Pembayaran Midtrans {tx_status} — silakan buat order baru."),
                ) {
                    error!("[midtrans-poller] tandai order {order_id} gagal: {e}");
                }
            }
        }
    }
    ProcessMidtransOutcome::Failed
}

// ---------------------------------------------------------------------------
// Tick + loop
// ---------------------------------------------------------------------------

/// Satu tick polling. Aman dipanggil berkala — idempotent.
///
/// Wrapper kompatibilitas: membangun [`MidtransClient`] dari `cfg` lalu
/// mendelegasikan ke [`tick_midtrans_payments_with`]. Dipakai call-site
/// lama (`bot/mod.rs`, `bot/handlers.rs`); tes + call-site baru memakai
/// varian `_with` supaya status Midtrans bisa diarahkan ke mock lokal.
pub async fn tick_midtrans_payments(
    client: &reqwest::Client,
    cfg: &Config,
    db: &Db,
    settlement: &dyn PaymentSettlement,
) -> MidtransPollResult {
    tick_midtrans_payments_with(&MidtransClient::from_client(client, cfg), cfg, db, settlement)
        .await
}

/// Varian injeksi dari [`tick_midtrans_payments`]: transport + base URL
/// status Midtrans diambil dari [`MidtransClient`].
pub async fn tick_midtrans_payments_with(
    client: &MidtransClient,
    cfg: &Config,
    db: &Db,
    settlement: &dyn PaymentSettlement,
) -> MidtransPollResult {
    if !midtrans_configured(cfg) {
        // Belum ada key = tidak ada yang bisa dicek; diam (bukan error).
        return MidtransPollResult {
            ok: true,
            checked: 0,
            matched: 0,
            expired: 0,
            error: None,
            code: None,
        };
    }
    let pending = match list_pending_midtrans_payments(db, None) {
        Ok(p) => p,
        Err(e) => {
            return MidtransPollResult {
                ok: false,
                checked: 0,
                matched: 0,
                expired: 0,
                error: Some(e.to_string()),
                code: Some(MidtransPollCode::StatusError),
            };
        }
    };
    let expired_list = list_expired_pending_midtrans_payments(db, None).unwrap_or_default();
    let mut matched = 0usize;
    for payment in &pending {
        match get_transaction_status(client, cfg, &payment.order_id).await {
            Ok(status) => {
                let res =
                    process_midtrans_result(db, settlement, &payment.order_id, &status.transaction_status)
                        .await;
                if res == ProcessMidtransOutcome::Paid {
                    matched += 1;
                }
            }
            Err(e) => {
                // 404 = transaksi belum ada di Midtrans — bukan error, lewati diam-diam.
                let msg = e.to_string();
                if !msg.contains("HTTP 404") {
                    error!("[midtrans-poller] status {} gagal: {msg}", payment.order_id);
                }
            }
        }
    }

    // Sapu pembayaran kedaluwarsa + batalkan order yang link-nya mati.
    let _ = expire_midtrans_payments(db, None);
    let mut expired = 0usize;
    for payment in &expired_list {
        expired += 1;
        if !payment.order_id.starts_with("UD-") {
            continue;
        }
        match settlement.order_status(db, &payment.order_id) {
            Ok(Some(status)) if status == "awaiting_payment" => {
                if let Err(e) = settlement.cancel_order_expired(
                    db,
                    &payment.order_id,
                    "Pembayaran Midtrans kedaluwarsa (30 menit) sebelum lunas — silakan buat order baru.",
                ) {
                    error!(
                        "[midtrans-poller] batalkan order kedaluwarsa {} gagal: {e}",
                        payment.order_id
                    );
                }
            }
            Ok(_) => {}
            Err(e) => {
                error!(
                    "[midtrans-poller] batalkan order kedaluwarsa {} gagal: {e}",
                    payment.order_id
                );
            }
        }
    }

    MidtransPollResult {
        ok: true,
        checked: pending.len(),
        matched,
        expired,
        error: None,
        code: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db;
    use crate::lib::orders::{
        create_store_order, get_store_order, mark_order_failed, mark_paid,
        set_order_paid_via, CreateStoreOrderInput,
    };
    use crate::lib::payments::midtrans::{create_midtrans_payment, get_midtrans_payment};
    use crate::lib::wallet::{ensure_user, get_wallet, mark_topup_paid, tg_id_from_buyer_ref,
        EnsureUserInput};
    use crate::lib::wallet::{create_topup, maybe_reward_referrer};
    use std::sync::{Arc, Mutex};

    fn test_db() -> Db {
        db::open(":memory:").expect("db tes")
    }

    /// Settlement nyata (orders/wallet) + pencatat fulfill — cerminan
    /// BotSettlement/WebSettlement, tanpa HTTP/telegram.
    struct FakeSettlement {
        spawned: Arc<Mutex<Vec<String>>>,
    }

    impl PaymentSettlement for FakeSettlement {
        fn credit_topup(&self, db: &Db, topup_id: &str) -> anyhow::Result<bool> {
            let (credited, topup) = mark_topup_paid(db, topup_id);
            if topup.is_none() {
                anyhow::bail!("Top-up tidak ditemukan: {topup_id}");
            }
            Ok(credited)
        }

        fn mark_order_paid(&self, db: &Db, order_id: &str, paid_via: &str) -> anyhow::Result<bool> {
            let Some(order) = get_store_order(db, order_id) else {
                return Ok(false);
            };
            if order.status.as_str() != "awaiting_payment" {
                return Ok(false);
            }
            set_order_paid_via(db, order_id, paid_via)?;
            mark_paid(db, order_id)?;
            Ok(true)
        }

        fn mark_order_failed(&self, db: &Db, order_id: &str, reason: &str) -> anyhow::Result<()> {
            mark_order_failed(db, order_id, reason)?;
            Ok(())
        }

        fn cancel_order_expired(&self, db: &Db, order_id: &str, reason: &str) -> anyhow::Result<()> {
            crate::lib::orders::cancel_order_for_expired_payment(db, order_id, reason)?;
            Ok(())
        }

        fn order_status(&self, db: &Db, order_id: &str) -> anyhow::Result<Option<String>> {
            Ok(get_store_order(db, order_id).map(|o| o.status.as_str().to_string()))
        }

        fn order_buyer_tg_id(&self, db: &Db, order_id: &str) -> anyhow::Result<Option<i64>> {
            Ok(get_store_order(db, order_id)
                .and_then(|o| tg_id_from_buyer_ref(o.buyer_ref.as_deref())))
        }

        fn maybe_reward_referrer(&self, db: &Db, referred_tg_id: i64) -> anyhow::Result<bool> {
            Ok(maybe_reward_referrer(db, referred_tg_id))
        }

        fn spawn_fulfill_and_notify(&self, order_id: String) {
            self.spawned.lock().unwrap().push(order_id);
        }
    }

    fn settlement() -> FakeSettlement {
        FakeSettlement {
            spawned: Arc::new(Mutex::new(Vec::new())),
        }
    }

    fn make_order(db: &Db) -> String {
        create_store_order(
            db,
            CreateStoreOrderInput {
                id: None,
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
        .unwrap()
        .id
    }

    #[tokio::test]
    async fn process_topup_settlement_is_paid_once() {
        // S6: tick poller top-up lunas -> saldo masuk; tick duplikat -> Ignored.
        let db = test_db();
        ensure_user(
            &db,
            EnsureUserInput {
                tg_id: 111,
                first_name: Some("Budi".to_string()),
                username: Some("budi".to_string()),
                referrer_tg_id: None,
            },
        );
        let topup = create_topup(&db, 111, 25000).unwrap();
        create_midtrans_payment(
            &db,
            &topup.id,
            25000,
            "SNAP-HARNESS",
            "https://app.sandbox.midtrans.com/snap/v1/redirection/harness",
            None,
        )
        .unwrap();
        let s = settlement();
        let out = process_midtrans_result(&db, &s, &topup.id, "settlement").await;
        assert_eq!(out, ProcessMidtransOutcome::Paid);
        assert_eq!(get_wallet(&db, 111).balance_idr, 25000);
        // Panggil lagi (webhook + poller dobel): idempotent.
        let out2 = process_midtrans_result(&db, &s, &topup.id, "settlement").await;
        assert_eq!(out2, ProcessMidtransOutcome::Ignored);
        assert_eq!(get_wallet(&db, 111).balance_idr, 25000);
    }

    #[tokio::test]
    async fn process_order_settlement_marks_paid_and_spawns_fulfill() {
        // S34: order lunas -> paid + fulfill di-spawn.
        let db = test_db();
        let order_id = make_order(&db);
        create_midtrans_payment(
            &db,
            &order_id,
            2000,
            "SNAP-1",
            "https://app.sandbox.midtrans.com/snap/v1/redirection/1",
            None,
        )
        .unwrap();
        let s = settlement();
        let out = process_midtrans_result(&db, &s, &order_id, "settlement").await;
        assert_eq!(out, ProcessMidtransOutcome::Paid);
        assert_eq!(get_store_order(&db, &order_id).unwrap().status.as_str(), "paid");
        assert_eq!(s.spawned.lock().unwrap().as_slice(), vec![order_id.clone()]);
        // Kedua kali: sudah bukan awaiting_payment -> Ignored.
        let out2 = process_midtrans_result(&db, &s, &order_id, "settlement").await;
        assert_eq!(out2, ProcessMidtransOutcome::Ignored);
        assert_eq!(s.spawned.lock().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn process_pending_is_noop() {
        let db = test_db();
        let order_id = make_order(&db);
        create_midtrans_payment(&db, &order_id, 2000, "SNAP-1", "https://x/1", None).unwrap();
        let s = settlement();
        let out = process_midtrans_result(&db, &s, &order_id, "pending").await;
        assert_eq!(out, ProcessMidtransOutcome::Pending);
        assert_eq!(
            get_store_order(&db, &order_id).unwrap().status.as_str(),
            "awaiting_payment"
        );
        assert!(s.spawned.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn process_deny_fails_payment_but_keeps_order() {
        // Quirk TS: markOrderFailed no-op dari awaiting_payment — order
        // baru benar-benar gagal saat expiry sweep.
        let db = test_db();
        let order_id = make_order(&db);
        create_midtrans_payment(&db, &order_id, 2000, "SNAP-1", "https://x/1", None).unwrap();
        let s = settlement();
        let out = process_midtrans_result(&db, &s, &order_id, "deny").await;
        assert_eq!(out, ProcessMidtransOutcome::Failed);
        assert_eq!(
            get_midtrans_payment(&db, &order_id).unwrap().unwrap().status.as_str(),
            "failed"
        );
        assert_eq!(
            get_store_order(&db, &order_id).unwrap().status.as_str(),
            "awaiting_payment"
        );
    }

    #[tokio::test]
    async fn process_settlement_without_midtrans_row_is_ignored() {
        // Tidak ada baris midtrans_payments -> tidak bisa diklaim.
        let db = test_db();
        let order_id = make_order(&db);
        let s = settlement();
        let out = process_midtrans_result(&db, &s, &order_id, "settlement").await;
        assert_eq!(out, ProcessMidtransOutcome::Ignored);
        assert_eq!(
            get_store_order(&db, &order_id).unwrap().status.as_str(),
            "awaiting_payment"
        );
    }

    #[tokio::test]
    async fn process_settlement_unknown_order_is_ignored() {
        // Baris Midtrans ada tapi order tidak ada di DB.
        let db = test_db();
        create_midtrans_payment(&db, "UD-UNKNOWN", 2000, "SNAP-1", "https://x/1", None).unwrap();
        let s = settlement();
        let out = process_midtrans_result(&db, &s, "UD-UNKNOWN", "settlement").await;
        assert_eq!(out, ProcessMidtransOutcome::Ignored);
    }

    // ------------------------------------------------- Gap 2: tick + mock status

    /// Peta order_id -> (HTTP status, body JSON) untuk mock server status.
    type StatusMap = Arc<Mutex<std::collections::HashMap<String, (u16, serde_json::Value)>>>;

    /// Mock server status Midtrans lokal: GET /v2/{order_id}/status.
    /// Peta order_id -> (HTTP status, body JSON); order tak dikenal -> 404.
    async fn start_status_mock(
    ) -> (
        String,
        StatusMap,
        tokio::task::JoinHandle<()>,
    ) {
        let map: StatusMap =
            Arc::new(Mutex::new(std::collections::HashMap::new()));
        let state = map.clone();
        let router = axum::Router::new()
            .route(
                "/v2/{order_id}/status",
                axum::routing::get(
                    |axum::extract::State(map): axum::extract::State<StatusMap>,
                     axum::extract::Path(order_id): axum::extract::Path<String>| async move {
                        let guard = map.lock().unwrap();
                        match guard.get(&order_id) {
                            Some((code, body)) => (
                                axum::http::StatusCode::from_u16(*code).unwrap(),
                                axum::Json(body.clone()),
                            ),
                            None => (
                                axum::http::StatusCode::NOT_FOUND,
                                axum::Json(serde_json::json!({
                                    "status_code": "404",
                                    "status_message": "Transaction doesn't exist",
                                })),
                            ),
                        }
                    },
                ),
            )
            .with_state(state);
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind mock status");
        let base = format!("http://127.0.0.1:{}", listener.local_addr().unwrap().port());
        let handle = tokio::spawn(async move {
            axum::serve(listener, router).await.unwrap();
        });
        (base, map, handle)
    }

    fn tick_cfg() -> crate::config::Config {
        let mut cfg = crate::config::Config::from_env().unwrap();
        cfg.midtrans_server_key = "TEST-SERVER-KEY".to_string();
        cfg
    }

    fn settlement_body(order_id: &str) -> serde_json::Value {
        serde_json::json!({
            "transaction_status": "settlement",
            "status_code": "200",
            "gross_amount": "2000",
            "payment_type": "qris",
            "order_id": order_id,
        })
    }

    #[tokio::test]
    async fn tick_settlement_marks_order_paid_and_spawns_fulfill() {
        // Gap 2b: tick end-to-end — status "settlement" -> order paid +
        // fulfill di-spawn, pembayaran ditandai paid.
        let db = test_db();
        let order_id = make_order(&db);
        create_midtrans_payment(
            &db,
            &order_id,
            2000,
            "SNAP-1",
            "https://app.sandbox.midtrans.com/snap/v1/redirection/1",
            None,
        )
        .unwrap();
        let (base, map, server) = start_status_mock().await;
        map.lock()
            .unwrap()
            .insert(order_id.clone(), (200, settlement_body(&order_id)));
        let client =
            crate::lib::payments::midtrans::MidtransClient::with_bases(reqwest::Client::new(), &base, &base);
        let cfg = tick_cfg();
        let s = settlement();
        let res = tick_midtrans_payments_with(&client, &cfg, &db, &s).await;
        assert!(res.ok, "tick harus ok: {:?}", res.error);
        assert_eq!(res.checked, 1);
        assert_eq!(res.matched, 1);
        assert_eq!(get_store_order(&db, &order_id).unwrap().status.as_str(), "paid");
        assert_eq!(
            get_midtrans_payment(&db, &order_id).unwrap().unwrap().status.as_str(),
            "paid"
        );
        assert_eq!(s.spawned.lock().unwrap().as_slice(), vec![order_id.clone()]);
        server.abort();
    }

    #[tokio::test]
    async fn tick_404_keeps_order_pending() {
        // Gap 2b: status 404 (transaksi belum dibuka pembeli) -> tetap pending,
        // bukan error, bukan gagal.
        let db = test_db();
        let order_id = make_order(&db);
        create_midtrans_payment(&db, &order_id, 2000, "SNAP-1", "https://x/1", None).unwrap();
        let (base, _map, server) = start_status_mock().await;
        let client =
            crate::lib::payments::midtrans::MidtransClient::with_bases(reqwest::Client::new(), &base, &base);
        let cfg = tick_cfg();
        let s = settlement();
        let res = tick_midtrans_payments_with(&client, &cfg, &db, &s).await;
        assert!(res.ok, "tick harus ok: {:?}", res.error);
        assert_eq!(res.checked, 1);
        assert_eq!(res.matched, 0);
        assert_eq!(res.expired, 0);
        assert_eq!(
            get_store_order(&db, &order_id).unwrap().status.as_str(),
            "awaiting_payment"
        );
        assert_eq!(
            get_midtrans_payment(&db, &order_id).unwrap().unwrap().status.as_str(),
            "pending"
        );
        assert!(s.spawned.lock().unwrap().is_empty());
        server.abort();
    }

    #[tokio::test]
    async fn tick_expired_payment_cancels_order() {
        // Gap 2b: pembayaran lewat expires_at -> pembayaran 'expired' +
        // order dibatalkan (status 'failed' via cancel_order_for_expired_payment).
        let db = test_db();
        let order_id = make_order(&db);
        let past = chrono::Utc::now() - chrono::Duration::minutes(31);
        create_midtrans_payment(&db, &order_id, 2000, "SNAP-1", "https://x/1", Some(past))
            .unwrap();
        let (base, _map, server) = start_status_mock().await;
        let client =
            crate::lib::payments::midtrans::MidtransClient::with_bases(reqwest::Client::new(), &base, &base);
        let cfg = tick_cfg();
        let s = settlement();
        let res = tick_midtrans_payments_with(&client, &cfg, &db, &s).await;
        assert!(res.ok, "tick harus ok: {:?}", res.error);
        assert_eq!(res.expired, 1);
        assert_eq!(
            get_midtrans_payment(&db, &order_id).unwrap().unwrap().status.as_str(),
            "expired"
        );
        assert_eq!(
            get_store_order(&db, &order_id).unwrap().status.as_str(),
            "failed"
        );
        assert!(s.spawned.lock().unwrap().is_empty());
        server.abort();
    }
}
