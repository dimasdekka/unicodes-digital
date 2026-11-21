//! Poller feed GoBiz — port dari `src/lib/payments/qris-poller.ts`.
//!
//! SATU-SATUNYA tempat pembayaran QRIS dinyatakan lunas. Cara kerja: ambil
//! transaksi terbaru dari feed GoBiz -> cocokkan dengan pembayaran QRIS
//! pending via `reconcile_feed_transactions` (qris.rs) -> untuk tiap match,
//! jalankan jalur yang SUDAH ADA:
//!   - top-up (ref TOPUP-...) -> kredit dompet (wallet, idempotent),
//!   - order (ref UD-...) -> tandai lunas -> fulfill (orders) + notifyBuyer.
//!     Tidak ada logika fulfillment / saldo yang diduplikasi di sini.
//!     QR yang kedaluwarsa -> pembayaran jadi 'expired' + order terkait
//!     dibatalkan supaya tidak menggantung.
//!
//! Dijalankan berkala dari proses bot (interval default 30 detik) dan manual
//! dari panel admin (POST /api/admin/payments/gopay/poll).
//! `tick_qris_payments()` TIDAK PERNAH panic — semua kegagalan dikembalikan
//! sebagai hasil `{ ok: false, error, code }` supaya interval tidak mati.
//!
//! Efek ke order/dompet lewat trait [`PaymentSettlement`] yang sama dengan
//! `midtrans_poller.rs` (lihat trait itu untuk signature yang harus diisi
//! kelompok orders/wallet/notify).

use tracing::{error, info};

use crate::db::Db;

use super::gopay::{is_auth_required, FeedListOptions, GopayClient};
use super::midtrans_poller::PaymentSettlement;
use super::qris::{
    expire_qris_payments, list_expired_pending_qris_payments, list_pending_qris_payments,
    mark_qris_payment_paid, reconcile_feed_transactions,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum QrisPollCode {
    AuthRequired,
    FeedError,
}

#[derive(Debug, Clone)]
pub struct QrisPollResult {
    pub ok: bool,
    /// Jumlah pembayaran pending yang dicek.
    pub checked: usize,
    /// Jumlah pembayaran yang lunas di tick ini.
    pub matched: usize,
    /// Jumlah QR yang kedaluwarsa di tick ini.
    pub expired: usize,
    /// Pesan error (kalau ok == false).
    pub error: Option<String>,
    /// Kode error: "auth_required" | "feed_error".
    pub code: Option<QrisPollCode>,
}

fn ok_result(checked: usize, matched: usize, expired: usize) -> QrisPollResult {
    QrisPollResult {
        ok: true,
        checked,
        matched,
        expired,
        error: None,
        code: None,
    }
}

fn err_result(error: String, code: QrisPollCode) -> QrisPollResult {
    QrisPollResult {
        ok: false,
        checked: 0,
        matched: 0,
        expired: 0,
        error: Some(error),
        code: Some(code),
    }
}

/// Satu tick polling. Aman dipanggil berkala — idempotent.
pub async fn tick_qris_payments(
    db: &Db,
    client: &GopayClient,
    settlement: &dyn PaymentSettlement,
) -> QrisPollResult {
    let now = chrono::Utc::now();
    // Tidak ada yang menunggu + tidak ada yang kedaluwarsa = jangan panggil
    // feed sama sekali (diam — bukan error; sejak Midtrans jadi jalur bayar
    // aktif, ini jalur yang paling sering terjadi).
    let pending_now = match list_pending_qris_payments(db, Some(now)) {
        Ok(p) => p,
        Err(e) => return err_result(e.to_string(), QrisPollCode::FeedError),
    };
    let expired_now = match list_expired_pending_qris_payments(db, Some(now)) {
        Ok(p) => p,
        Err(e) => return err_result(e.to_string(), QrisPollCode::FeedError),
    };
    if pending_now.is_empty() && expired_now.is_empty() {
        return ok_result(0, 0, 0);
    }
    let feed = match client.list_feed_transactions(FeedListOptions::default()).await {
        Ok(f) => f,
        Err(e) => {
            if is_auth_required(&e) {
                return err_result(
                    "Sesi GoBiz belum login / kedaluwarsa — minta OTP di panel admin (/admin)."
                        .to_string(),
                    QrisPollCode::AuthRequired,
                );
            }
            error!("[qris-poller] feed gagal: {e}");
            return err_result(e.to_string(), QrisPollCode::FeedError);
        }
    };
    if feed.truncated {
        error!("[qris-poller] feed terpotong (lebih dari 10 halaman) — sebagian transaksi mungkin terlewat tick ini.");
    }

    let pending = match list_pending_qris_payments(db, Some(now)) {
        Ok(p) => p,
        Err(e) => return err_result(e.to_string(), QrisPollCode::FeedError),
    };
    let matches = reconcile_feed_transactions(&pending, &feed.transactions, None);
    let mut matched = 0usize;
    for (payment, transaction) in matches {
        let claimed = match mark_qris_payment_paid(db, &payment.id) {
            Ok(v) => v,
            Err(e) => {
                error!("[qris-poller] tandai lunas {} gagal: {e}", payment.reference);
                continue;
            }
        };
        if !claimed {
            continue; // sudah diproses tick lain
        }
        matched += 1;
        let ref_ = payment.reference.as_str();
        if ref_.starts_with("TOPUP-") {
            match settlement.credit_topup(db, ref_) {
                Ok(credited) => {
                    if credited {
                        info!(
                            "[qris-poller] top-up {ref_} lunas (feed {}) — saldo masuk.",
                            transaction.id
                        );
                    }
                }
                Err(e) => {
                    error!(
                        "[qris-poller] top-up {ref_} tidak ditemukan (transaksi feed {} sudah ditandai lunas): {e}",
                        transaction.id
                    );
                }
            }
        } else if ref_.starts_with("UD-") {
            let status = match settlement.order_status(db, ref_) {
                Ok(s) => s,
                Err(e) => {
                    error!("[qris-poller] baca order {ref_} gagal: {e}");
                    continue;
                }
            };
            let Some(status) = status else {
                error!(
                    "[qris-poller] order {ref_} tidak ditemukan (transaksi feed {} sudah ditandai lunas).",
                    transaction.id
                );
                continue;
            };
            if status != "awaiting_payment" {
                continue; // sudah dibayar/diproses
            }
            match settlement.mark_order_paid(db, ref_, "qris") {
                Ok(true) => {}
                Ok(false) => continue,
                Err(e) => {
                    error!("[qris-poller] tandai order {ref_} lunas gagal: {e}");
                    continue;
                }
            }
            if let Ok(Some(tg_id)) = settlement.order_buyer_tg_id(db, ref_) {
                if let Err(e) = settlement.maybe_reward_referrer(db, tg_id) {
                    error!("[qris-poller] reward referral {ref_} gagal: {e}");
                }
            }
            // Fulfillment jalan di background (bisa ~10 menit) — jangan tahan tick.
            settlement.spawn_fulfill_and_notify(ref_.to_string());
            info!(
                "[qris-poller] order {ref_} lunas (feed {}) — fulfill dimulai.",
                transaction.id
            );
        } else if ref_.starts_with("ADMIN-TEST-") {
            // QR tes panel admin — tidak ada tindak lanjut.
        } else {
            error!(
                "[qris-poller] referensi pembayaran tidak dikenal: {ref_} (feed {}) — sudah ditandai lunas, cek manual.",
                transaction.id
            );
        }
    }

    // Sapu QR kedaluwarsa + batalkan order yang QR-nya mati.
    let expired_list = list_expired_pending_qris_payments(db, Some(now)).unwrap_or_default();
    let _ = expire_qris_payments(db, Some(now));
    let mut expired = 0usize;
    for payment in &expired_list {
        expired += 1;
        if !payment.reference.starts_with("UD-") {
            continue;
        }
        match settlement.order_status(db, &payment.reference) {
            Ok(Some(status)) if status == "awaiting_payment" => {
                if let Err(e) = settlement.cancel_order_expired(
                    db,
                    &payment.reference,
                    "Pembayaran QRIS kedaluwarsa (5 menit) sebelum lunas — silakan buat order baru.",
                ) {
                    error!(
                        "[qris-poller] batalkan order kedaluwarsa {} gagal: {e}",
                        payment.reference
                    );
                }
            }
            Ok(_) => {}
            Err(e) => {
                error!(
                    "[qris-poller] batalkan order kedaluwarsa {} gagal: {e}",
                    payment.reference
                );
            }
        }
    }

    ok_result(pending.len(), matched, expired)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db;
    use crate::lib::orders::{
        create_store_order, get_store_order, CreateStoreOrderInput,
    };
    use crate::lib::payments::gopay::{
        GopayClient, GopayClientOptions, GopayError, GopayErrorCode, GopayHttp,
        GopaySession, HttpResponse,
    };
    use crate::lib::payments::midtrans_poller::PaymentSettlement;
    use crate::lib::payments::qris::{
        create_qris_payment, serialize_qris_payload, QrisTlv,
    };
    use crate::lib::wallet::{get_wallet, mark_topup_paid, tg_id_from_buyer_ref};
    use crate::lib::wallet::{create_topup, ensure_user, EnsureUserInput};
    use std::future::Future;
    use std::pin::Pin;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};

    fn test_db() -> Db {
        db::open(":memory:").expect("db tes")
    }

    fn static_payload() -> String {
        serialize_qris_payload(&[
            QrisTlv { tag: "00".into(), value: "01".into() },
            QrisTlv { tag: "01".into(), value: "11".into() },
            QrisTlv {
                tag: "26".into(),
                value: "0014ID.CO.QRIS.WWW011693600123456789010215ID1234567890123".into(),
            },
            QrisTlv { tag: "52".into(), value: "4814".into() },
            QrisTlv { tag: "53".into(), value: "360".into() },
            QrisTlv { tag: "58".into(), value: "ID".into() },
            QrisTlv { tag: "59".into(), value: "TOKO TEST QRIS".into() },
            QrisTlv { tag: "60".into(), value: "JAKARTA".into() },
        ])
        .unwrap()
    }

    /// Transport HTTP palsu: mengembalikan feed transaksi yang diatur tes.
    struct FakeHttp {
        calls: Arc<AtomicUsize>,
        feed: Arc<Mutex<Vec<serde_json::Value>>>,
    }

    impl GopayHttp for FakeHttp {
        fn request(
            &self,
            _method: &str,
            url: &str,
            _headers: Vec<(String, String)>,
            _body: Option<String>,
        ) -> Pin<Box<dyn Future<Output = Result<HttpResponse, GopayError>> + Send + '_>> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            let feed = self.feed.lock().unwrap().clone();
            let is_feed = url.contains("transactions");
            let url_owned = url.to_string();
            Box::pin(async move {
                if is_feed {
                    Ok(HttpResponse {
                        status: 200,
                        body: serde_json::json!({
                            "from": 0,
                            "size": 100,
                            "total": feed.len(),
                            "transactions": feed,
                        }),
                    })
                } else {
                    Err(GopayError::new(
                        GopayErrorCode::HttpError,
                        format!("FakeHttp: URL tak terduga {url_owned}"),
                    ))
                }
            })
        }
    }

    fn feed_tx(id: &str, amount_idr: i64) -> serde_json::Value {
        serde_json::json!({
            "id": id,
            "order_id": null,
            "merchant_id": "M-HARNESS",
            "transaction_status": "settlement",
            "payment_type": "qris",
            "gross_amount": amount_idr * 100,
            "real_gross_amount": amount_idr * 100,
            "currency": "IDR",
            "transaction_time": chrono::Utc::now().to_rfc3339(),
            "settlement_time": null,
            "transaction_source": "qris",
        })
    }

    fn client_with_feed(db: &Db, feed: Vec<serde_json::Value>) -> (GopayClient, Arc<AtomicUsize>) {
        let calls = Arc::new(AtomicUsize::new(0));
        let fake = FakeHttp {
            calls: calls.clone(),
            feed: Arc::new(Mutex::new(feed)),
        };
        let client = GopayClient::with_options(
            db.clone(),
            GopayClientOptions {
                http: Some(Arc::new(fake)),
                ..Default::default()
            },
        );
        // Sesi GoBiz mock (seperti harness): token segar + merchant_id,
        // supaya tick langsung membaca feed tanpa refresh/profil.
        let now_ms = chrono::Utc::now().timestamp_millis();
        client
            .save_session(&GopaySession {
                access_token: "AT-HARNESS".to_string(),
                refresh_token: "RT-HARNESS".to_string(),
                token_type: "Bearer".to_string(),
                expires_at: now_ms + 3_600_000,
                phone: "81234567890".to_string(),
                merchant_id: Some("M-HARNESS".to_string()),
            })
            .unwrap();
        (client, calls)
    }

    /// Settlement nyata (orders/wallet) + pencatat fulfill.
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
            crate::lib::orders::set_order_paid_via(db, order_id, paid_via)?;
            crate::lib::orders::mark_paid(db, order_id)?;
            Ok(true)
        }

        fn mark_order_failed(&self, db: &Db, order_id: &str, reason: &str) -> anyhow::Result<()> {
            crate::lib::orders::mark_order_failed(db, order_id, reason)?;
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
            Ok(crate::lib::wallet::maybe_reward_referrer(db, referred_tg_id))
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
    async fn tick_without_pending_does_not_call_feed() {
        // Tidak ada yang menunggu = diam, tanpa HTTP.
        let db = test_db();
        let (client, calls) = client_with_feed(&db, vec![]);
        let s = settlement();
        let res = tick_qris_payments(&db, &client, &s).await;
        assert!(res.ok);
        assert_eq!(res.checked, 0);
        assert_eq!(res.matched, 0);
        assert_eq!(res.expired, 0);
        assert_eq!(calls.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn tick_matches_feed_once_then_duplicate_is_ignored() {
        // S26: feed cocok -> order lunas + fulfill di-spawn; feed yang sama
        // di tick kedua -> matched 0, tidak ada fulfill kedua.
        let db = test_db();
        let order_id = make_order(&db);
        let created =
            create_qris_payment(&db, &order_id, 2000, &static_payload(), None).unwrap();
        let unique = created.payment.unique_amount_idr;
        let (client, _) = client_with_feed(&db, vec![feed_tx("TX-DUP-1", unique)]);
        let s = settlement();
        let res = tick_qris_payments(&db, &client, &s).await;
        assert!(res.ok, "{:?}", res.error);
        assert_eq!(res.matched, 1);
        assert_eq!(get_store_order(&db, &order_id).unwrap().status.as_str(), "paid");
        assert_eq!(s.spawned.lock().unwrap().as_slice(), vec![order_id.clone()]);
        // Tick kedua dengan feed yang SAMA.
        let res2 = tick_qris_payments(&db, &client, &s).await;
        assert!(res2.ok);
        assert_eq!(res2.matched, 0);
        assert_eq!(s.spawned.lock().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn tick_topup_credits_wallet() {
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
        let topup = create_topup(&db, 111, 10000).unwrap();
        let created =
            create_qris_payment(&db, &topup.id, 10000, &static_payload(), None).unwrap();
        let unique = created.payment.unique_amount_idr;
        let (client, _) = client_with_feed(&db, vec![feed_tx("TX-TOPUP-1", unique)]);
        let s = settlement();
        let res = tick_qris_payments(&db, &client, &s).await;
        assert!(res.ok, "{:?}", res.error);
        assert_eq!(res.matched, 1);
        assert_eq!(get_wallet(&db, 111).balance_idr, 10000);
        // Tick duplikat: tidak kredit 2x.
        let res2 = tick_qris_payments(&db, &client, &s).await;
        assert_eq!(res2.matched, 0);
        assert_eq!(get_wallet(&db, 111).balance_idr, 10000);
    }

    #[tokio::test]
    async fn tick_without_session_returns_auth_required() {
        // S30b: sesi GoBiz habis -> error jelas (auth_required), tidak throw.
        let db = test_db();
        let order_id = make_order(&db);
        create_qris_payment(&db, &order_id, 2000, &static_payload(), None).unwrap();
        // Client TANPA sesi tersimpan.
        let client = GopayClient::with_options(db.clone(), GopayClientOptions::default());
        let s = settlement();
        let res = tick_qris_payments(&db, &client, &s).await;
        assert!(!res.ok);
        assert_eq!(res.code, Some(QrisPollCode::AuthRequired));
        assert!(res.error.as_deref().unwrap().contains("OTP"));
        assert_eq!(
            get_store_order(&db, &order_id).unwrap().status.as_str(),
            "awaiting_payment"
        );
    }

    #[tokio::test]
    async fn tick_expired_qr_cancels_order() {
        // S30c: QR kedaluwarsa -> pembayaran expired + order dibatalkan.
        let db = test_db();
        let order_id = make_order(&db);
        let past = chrono::Utc::now() - chrono::Duration::minutes(10);
        create_qris_payment(&db, &order_id, 2000, &static_payload(), Some(past)).unwrap();
        let (client, _) = client_with_feed(&db, vec![]);
        let s = settlement();
        let res = tick_qris_payments(&db, &client, &s).await;
        assert!(res.ok, "{:?}", res.error);
        assert!(res.expired >= 1);
        let done = get_store_order(&db, &order_id).unwrap();
        assert_eq!(done.status.as_str(), "failed");
        assert!(done.error.as_deref().unwrap().contains("kedaluwarsa"));
    }
}
