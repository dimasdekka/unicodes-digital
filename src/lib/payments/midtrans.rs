//! Pembayaran via Midtrans Snap — port dari `src/lib/payments/midtrans.ts`.
//!
//! HANYA QRIS yang diaktifkan (enabled_payments: ["qris"]) — VA/kartu punya
//! fee flat (Rp4.000 / 2,9% + Rp2.000) yang lebih mahal dari produk Rp2.000.
//! Deteksi DUA jalur (sama-sama idempotent, via `process_midtrans_result`
//! di `midtrans_poller.rs`):
//!   1. Polling status API tiap 30 detik (proses bot) — jalan tanpa domain publik.
//!   2. Webhook POST /api/webhooks/midtrans (butuh domain publik) —
//!      verifikasi signature SHA512(order_id+status_code+gross_amount+server_key).
//!      Secret TIDAK pernah di-log — pemanggil dilarang mencetak hasilnya.

use crate::config::Config;
use crate::db::Db;
use chrono::{DateTime, Duration, Utc};
use rusqlite::{params, OptionalExtension};
use sha2::{Digest, Sha512};
use std::time::Duration as StdDuration;

use super::iso_of;

/// Masa berlaku link bayar Midtrans (menit).
pub const MIDTRANS_EXPIRY_MINUTES: i64 = 30;
/// Satu-satunya metode yang diaktifkan di Snap.
pub const MIDTRANS_QRIS_ONLY: &[&str] = &["qris"];
/// Timeout tiap request HTTP ke Midtrans (TS: AbortSignal.timeout(25000)).
const MIDTRANS_REQUEST_TIMEOUT: StdDuration = StdDuration::from_secs(25);

fn snap_base(is_production: bool) -> &'static str {
    if is_production {
        "https://app.midtrans.com"
    } else {
        "https://app.sandbox.midtrans.com"
    }
}

fn api_base(is_production: bool) -> &'static str {
    if is_production {
        "https://api.midtrans.com"
    } else {
        "https://api.sandbox.midtrans.com"
    }
}

fn auth_header(server_key: &str) -> String {
    use base64::Engine;
    let encoded = base64::engine::general_purpose::STANDARD.encode(format!("{server_key}:"));
    format!("Basic {encoded}")
}

/// Client Midtrans dengan base URL yang bisa diinjeksi (seam tes, Gap 2).
///
/// Perilaku produksi TIDAK berubah: `from_config` / `from_client` memakai
/// URL yang sama persis seperti sebelumnya (sandbox vs produksi dari
/// `cfg.midtrans_is_production`). Dua env var opsional — kalau diisi —
/// menimpa base URL: `MIDTRANS_SNAP_BASE_URL` dan `MIDTRANS_API_BASE_URL`
/// (dipakai tes supaya Snap/status bisa diarahkan ke mock server lokal);
/// kalau kosong, default di bawah yang dipakai. Tidak ada env var lama
/// yang diubah namanya.
#[derive(Debug, Clone)]
pub struct MidtransClient {
    pub client: reqwest::Client,
    pub snap_base: String,
    pub api_base: String,
}

impl MidtransClient {
    /// Konstruktor standalone dari [`Config`]; jalur produksi memakai
    /// `from_client` supaya memakai HTTP client bersama. Dipakai tes.
    #[allow(dead_code)]
    pub fn from_config(cfg: &Config) -> Self {
        Self::from_client(&reqwest::Client::new(), cfg)
    }

    pub fn from_client(client: &reqwest::Client, cfg: &Config) -> Self {
        let snap_base = std::env::var("MIDTRANS_SNAP_BASE_URL")
            .ok()
            .filter(|s| !s.trim().is_empty())
            .unwrap_or_else(|| snap_base(cfg.midtrans_is_production).to_string());
        let api_base = std::env::var("MIDTRANS_API_BASE_URL")
            .ok()
            .filter(|s| !s.trim().is_empty())
            .unwrap_or_else(|| api_base(cfg.midtrans_is_production).to_string());
        Self::with_bases(client.clone(), &snap_base, &api_base)
    }

    /// Konstruktor eksplisit (tes): base URL apa adanya, tanpa baca env.
    pub fn with_bases(client: reqwest::Client, snap_base: &str, api_base: &str) -> Self {
        Self {
            client,
            snap_base: snap_base.trim_end_matches('/').to_string(),
            api_base: api_base.trim_end_matches('/').to_string(),
        }
    }
}

/// Kunci serialisasi untuk tes yang menyentuh env var override Midtrans
/// (`MIDTRANS_SNAP_BASE_URL` / `MIDTRANS_API_BASE_URL`): tes Rust berjalan
/// paralel dalam satu proses dan `set_var`/`remove_var` bersifat global.
/// Tes yang mengubah env var ini WAJIB menahan kunci ini selama tes berjalan.
#[cfg(test)]
pub(crate) static MIDTRANS_ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// Apakah Midtrans terkonfigurasi (server key terisi)? Sama seperti
/// `midtransConfigured()` di TS.
pub fn midtrans_configured(cfg: &Config) -> bool {
    !cfg.midtrans_server_key.trim().is_empty()
}

#[derive(Debug, Clone)]
pub struct SnapTransaction {
    pub token: String,
    pub redirect_url: String,
}

/// Buat transaksi Snap — kembalikan token + URL halaman bayar Midtrans.
/// Pembeli membuka redirect_url (QRIS tampil di halaman Midtrans).
///
/// Wrapper kompatibilitas: membangun [`MidtransClient`] dari `cfg`
/// (URL produksi/sandbox seperti dulu) lalu mendelegasikan ke
/// [`create_snap_transaction_with`]. Dipakai call-site lama
/// (`bot/handlers.rs`); call-site baru memakai varian `_with` langsung.
pub async fn create_snap_transaction(
    client: &reqwest::Client,
    cfg: &Config,
    order_id: &str,
    amount_idr: i64,
    item_name: &str,
) -> anyhow::Result<SnapTransaction> {
    create_snap_transaction_with(&MidtransClient::from_client(client, cfg), cfg, order_id, amount_idr, item_name).await
}

/// Varian injeksi dari [`create_snap_transaction`]: transport + base URL
/// diambil dari [`MidtransClient`] (bukan dari flag produksi).
pub async fn create_snap_transaction_with(
    client: &MidtransClient,
    cfg: &Config,
    order_id: &str,
    amount_idr: i64,
    item_name: &str,
) -> anyhow::Result<SnapTransaction> {
    let item_name: String = item_name.chars().take(50).collect();
    let body = serde_json::json!({
        "transaction_details": {
            "order_id": order_id,
            "gross_amount": amount_idr,
        },
        "item_details": [{
            "id": order_id,
            "price": amount_idr,
            "quantity": 1,
            "name": item_name,
        }],
        "enabled_payments": MIDTRANS_QRIS_ONLY,
        "expiry": { "unit": "minute", "duration": MIDTRANS_EXPIRY_MINUTES },
    });
    let res = client
        .client
        .post(format!("{}/snap/v1/transactions", client.snap_base))
        .header("Accept", "application/json")
        .header("Content-Type", "application/json")
        .header("Authorization", auth_header(&cfg.midtrans_server_key))
        .json(&body)
        .timeout(MIDTRANS_REQUEST_TIMEOUT)
        .send()
        .await?;
    let status = res.status();
    let data: serde_json::Value = res.json().await.unwrap_or(serde_json::json!({}));
    let token = data.get("token").and_then(|t| t.as_str());
    if !status.is_success() || token.is_none() {
        let msg = data
            .get("error_messages")
            .and_then(|m| m.as_array())
            .filter(|a| !a.is_empty())
            .map(|a| {
                a.iter()
                    .map(|v| v.as_str().unwrap_or_default())
                    .collect::<Vec<_>>()
                    .join("; ")
            })
            .unwrap_or_else(|| format!("Midtrans Snap gagal (HTTP {status})"));
        anyhow::bail!("{msg}");
    }
    Ok(SnapTransaction {
        token: token.unwrap().to_string(),
        redirect_url: data
            .get("redirect_url")
            .and_then(|u| u.as_str())
            .unwrap_or("")
            .to_string(),
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MidtransTxStatus {
    Paid,
    Pending,
    Failed,
}

/// Body status transaksi Midtrans (GET /v2/{order_id}/status) —
/// cerminan respons API; tidak semua field dibaca jalur produksi.
#[allow(dead_code)]
#[derive(Debug, Clone)]
pub struct MidtransStatusBody {
    pub transaction_status: String,
    /// status_code dinormalisasi ke string ("200" / "404") — Midtrans kadang
    /// mengirim angka, kadang string.
    pub status_code: String,
    pub gross_amount: String,
    pub payment_type: Option<String>,
    pub order_id: String,
    /// Body mentah (field lain dari Midtrans, mis. status_message).
    pub raw: serde_json::Value,
}

/// settlement/capture = lunas; pending/authorize = tunggu; sisanya = gagal.
pub fn map_midtrans_status(transaction_status: &str) -> MidtransTxStatus {
    match transaction_status {
        "settlement" | "capture" => MidtransTxStatus::Paid,
        "pending" | "authorize" => MidtransTxStatus::Pending,
        _ => MidtransTxStatus::Failed,
    }
}

fn json_to_string(v: Option<&serde_json::Value>) -> String {
    match v {
        Some(serde_json::Value::String(s)) => s.clone(),
        Some(serde_json::Value::Number(n)) => n.to_string(),
        _ => String::new(),
    }
}

/// GET /v2/{order_id}/status — status transaksi di Midtrans.
///
/// Menerima [`MidtransClient`] (seam Gap 2): transport + base URL dari
/// struct, bukan dari flag produksi. Tidak ada call-site lama yang memakai
/// signature lama (`&reqwest::Client`) — satu-satunya pemakai adalah
/// `tick_midtrans_payments_with` — jadi tidak perlu wrapper kompatibilitas
/// (berbeda dengan `create_snap_transaction` yang masih dipakai
/// `bot/handlers.rs`).
///
/// KRITIS: transaksi Snap BELUM ADA di sistem Midtrans sampai pembeli membuka
/// halaman bayar — status API menjawab 404 ("Transaction doesn't exist",
/// kadang HTTP 200 dengan status_code "404" di body). Itu BUKAN gagal:
/// perlakukan sebagai pending; expiry sweep 30 menit yang menutupnya kalau
/// pembeli tidak pernah membuka link.
pub async fn get_transaction_status(
    client: &MidtransClient,
    cfg: &Config,
    order_id: &str,
) -> anyhow::Result<MidtransStatusBody> {
    let url = format!(
        "{}/v2/{}/status",
        client.api_base,
        urlencoding(order_id)
    );
    let res = client
        .client
        .get(url)
        .header("Accept", "application/json")
        .header("Authorization", auth_header(&cfg.midtrans_server_key))
        .timeout(MIDTRANS_REQUEST_TIMEOUT)
        .send()
        .await?;
    let http_status = res.status();
    let data: serde_json::Value = res.json().await.unwrap_or(serde_json::json!({}));
    let status_code = json_to_string(data.get("status_code"));
    if http_status.as_u16() == 404 || status_code == "404" {
        return Ok(MidtransStatusBody {
            transaction_status: "pending".to_string(),
            status_code,
            gross_amount: json_to_string(data.get("gross_amount")),
            payment_type: data
                .get("payment_type")
                .and_then(|p| p.as_str())
                .map(str::to_string),
            order_id: json_to_string(data.get("order_id")),
            raw: data,
        });
    }
    if !http_status.is_success() {
        let msg = data
            .get("status_message")
            .and_then(|m| m.as_str())
            .unwrap_or("")
            .to_string();
        let msg = if msg.is_empty() {
            format!("HTTP {http_status}")
        } else {
            msg
        };
        anyhow::bail!("Midtrans status gagal (HTTP {http_status}): {msg}");
    }
    Ok(MidtransStatusBody {
        transaction_status: json_to_string(data.get("transaction_status")),
        status_code,
        gross_amount: json_to_string(data.get("gross_amount")),
        payment_type: data
            .get("payment_type")
            .and_then(|p| p.as_str())
            .map(str::to_string),
        order_id: json_to_string(data.get("order_id")),
        raw: data,
    })
}

fn urlencoding(s: &str) -> String {
    // encodeURIComponent setara TS: huruf/angka + - _ . ! ~ * ' ( ) dibiarkan.
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'!' | b'~'
            | b'*' | b'\'' | b'(' | b')' => out.push(b as char),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

/// Perbandingan constant-time dua string hex (supaya timing tidak membocorkan
/// berapa prefix signature yang benar).
fn constant_time_eq(a: &str, b: &str) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for (x, y) in a.bytes().zip(b.bytes()) {
        diff |= x ^ y;
    }
    diff == 0
}

/// Verifikasi signature webhook: SHA512(order_id + status_code +
/// gross_amount + server_key) hex == signature_key (constant-time).
pub fn verify_notification_signature(
    server_key: &str,
    order_id: &str,
    status_code: &str,
    gross_amount: &str,
    signature_key: &str,
) -> bool {
    let input = format!("{order_id}{status_code}{gross_amount}{server_key}");
    let want = hex::encode(Sha512::digest(input.as_bytes()));
    constant_time_eq(&want, signature_key)
}

// ------------------------------------------------------- tabel midtrans ---

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MidtransPaymentStatus {
    Pending,
    Paid,
    Failed,
    Expired,
}

impl MidtransPaymentStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Paid => "paid",
            Self::Failed => "failed",
            Self::Expired => "expired",
        }
    }

    fn from_str(s: &str) -> anyhow::Result<Self> {
        match s {
            "pending" => Ok(Self::Pending),
            "paid" => Ok(Self::Paid),
            "failed" => Ok(Self::Failed),
            "expired" => Ok(Self::Expired),
            other => anyhow::bail!("Status midtrans_payments tidak dikenal: {other}"),
        }
    }
}

/// Baris tabel `midtrans_payments` — cerminan baris DB; tidak semua
/// field dibaca jalur produksi (dipakai tes + admin).
#[allow(dead_code)]
#[derive(Debug, Clone)]
pub struct MidtransPaymentRow {
    pub order_id: String,
    pub amount_idr: i64,
    pub snap_token: String,
    pub redirect_url: String,
    pub status: MidtransPaymentStatus,
    pub created_at: String,
    pub expires_at: String,
}

fn row_from_sql(row: &rusqlite::Row) -> rusqlite::Result<MidtransPaymentRow> {
    let status_str: String = row.get("status")?;
    Ok(MidtransPaymentRow {
        order_id: row.get("order_id")?,
        amount_idr: row.get("amount_idr")?,
        snap_token: row.get("snap_token")?,
        redirect_url: row.get("redirect_url")?,
        status: MidtransPaymentStatus::from_str(&status_str)
            .map_err(|_| rusqlite::Error::InvalidColumnName(status_str))?,
        created_at: row.get("created_at")?,
        expires_at: row.get("expires_at")?,
    })
}

pub fn create_midtrans_payment(
    db: &Db,
    order_id: &str,
    amount_idr: i64,
    snap_token: &str,
    redirect_url: &str,
    now: Option<DateTime<Utc>>,
) -> anyhow::Result<MidtransPaymentRow> {
    let now_dt = now.unwrap_or_else(Utc::now);
    let created_at = iso_of(now_dt);
    let expires_at = iso_of(now_dt + Duration::minutes(MIDTRANS_EXPIRY_MINUTES));
    let conn = db.lock().unwrap();
    conn.execute(
        "INSERT INTO midtrans_payments (order_id, amount_idr, snap_token, redirect_url, status, created_at, expires_at) VALUES (?, ?, ?, ?, 'pending', ?, ?)",
        params![order_id, amount_idr, snap_token, redirect_url, created_at, expires_at],
    )?;
    let row = conn
        .query_row(
            "SELECT * FROM midtrans_payments WHERE order_id = ?",
            params![order_id],
            row_from_sql,
        )
        .optional()?
        .ok_or_else(|| {
            anyhow::anyhow!("Gagal membaca kembali pembayaran Midtrans yang baru dibuat")
        })?;
    Ok(row)
}

pub fn get_midtrans_payment(db: &Db, order_id: &str) -> anyhow::Result<Option<MidtransPaymentRow>> {
    let conn = db.lock().unwrap();
    Ok(conn
        .query_row(
            "SELECT * FROM midtrans_payments WHERE order_id = ?",
            params![order_id],
            row_from_sql,
        )
        .optional()?)
}

/// Pembayaran pending yang belum kedaluwarsa.
pub fn list_pending_midtrans_payments(
    db: &Db,
    now: Option<DateTime<Utc>>,
) -> anyhow::Result<Vec<MidtransPaymentRow>> {
    let now_iso = iso_of(now.unwrap_or_else(Utc::now));
    let conn = db.lock().unwrap();
    let mut stmt = conn.prepare(
        "SELECT * FROM midtrans_payments WHERE status = 'pending' AND expires_at > ? ORDER BY created_at ASC",
    )?;
    let rows = stmt.query_map(params![now_iso], row_from_sql)?;
    rows.collect::<Result<Vec<_>, _>>().map_err(anyhow::Error::from)
}

/// Daftar pembayaran pending yang sudah lewat expires_at (belum disapu).
pub fn list_expired_pending_midtrans_payments(
    db: &Db,
    now: Option<DateTime<Utc>>,
) -> anyhow::Result<Vec<MidtransPaymentRow>> {
    let now_iso = iso_of(now.unwrap_or_else(Utc::now));
    let conn = db.lock().unwrap();
    let mut stmt = conn.prepare(
        "SELECT * FROM midtrans_payments WHERE status = 'pending' AND expires_at <= ? ORDER BY created_at ASC",
    )?;
    let rows = stmt.query_map(params![now_iso], row_from_sql)?;
    rows.collect::<Result<Vec<_>, _>>().map_err(anyhow::Error::from)
}

/// Tandai pending jadi 'paid'. False kalau id tidak ada / sudah bukan pending.
pub fn mark_midtrans_payment_paid(db: &Db, order_id: &str) -> anyhow::Result<bool> {
    let conn = db.lock().unwrap();
    Ok(conn.execute(
        "UPDATE midtrans_payments SET status = 'paid' WHERE order_id = ? AND status = 'pending'",
        params![order_id],
    )? > 0)
}

/// Tandai pending jadi 'failed'. False kalau id tidak ada / sudah bukan pending.
pub fn mark_midtrans_payment_failed(db: &Db, order_id: &str) -> anyhow::Result<bool> {
    let conn = db.lock().unwrap();
    Ok(conn.execute(
        "UPDATE midtrans_payments SET status = 'failed' WHERE order_id = ? AND status = 'pending'",
        params![order_id],
    )? > 0)
}

/// Tandai semua pending yang lewat expires_at jadi 'expired'. Kembalikan jumlahnya.
pub fn expire_midtrans_payments(db: &Db, now: Option<DateTime<Utc>>) -> anyhow::Result<usize> {
    let now_iso = iso_of(now.unwrap_or_else(Utc::now));
    let conn = db.lock().unwrap();
    Ok(conn.execute(
        "UPDATE midtrans_payments SET status = 'expired' WHERE status = 'pending' AND expires_at <= ?",
        params![now_iso],
    )?)
}

// ---------------------------------------------------------------------------
// Tes (cerminan midtrans.test.ts — server key SINTETIS, tanpa jaringan)
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;
    use std::sync::{Arc, Mutex};

    fn test_db() -> Db {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE midtrans_payments (
              order_id TEXT PRIMARY KEY,
              amount_idr INTEGER NOT NULL,
              snap_token TEXT NOT NULL,
              redirect_url TEXT NOT NULL,
              status TEXT NOT NULL DEFAULT 'pending',
              created_at TEXT NOT NULL,
              expires_at TEXT NOT NULL
            );",
        )
        .unwrap();
        Arc::new(Mutex::new(conn))
    }

    #[test]
    fn status_mapping() {
        assert_eq!(map_midtrans_status("settlement"), MidtransTxStatus::Paid);
        assert_eq!(map_midtrans_status("capture"), MidtransTxStatus::Paid);
        assert_eq!(map_midtrans_status("pending"), MidtransTxStatus::Pending);
        assert_eq!(map_midtrans_status("authorize"), MidtransTxStatus::Pending);
        assert_eq!(map_midtrans_status("deny"), MidtransTxStatus::Failed);
        assert_eq!(map_midtrans_status("cancel"), MidtransTxStatus::Failed);
        assert_eq!(map_midtrans_status("expire"), MidtransTxStatus::Failed);
        assert_eq!(map_midtrans_status("failure"), MidtransTxStatus::Failed);
    }

    #[test]
    fn signature_verify() {
        let expected = hex::encode(Sha512::digest(b"UD-TEST1232001500TEST-SERVER-KEY"));
        assert!(verify_notification_signature(
            "TEST-SERVER-KEY",
            "UD-TEST123",
            "200",
            "1500",
            &expected
        ));
        assert!(!verify_notification_signature(
            "TEST-SERVER-KEY",
            "UD-TEST123",
            "200",
            "1500",
            &"0".repeat(128)
        ));
        assert!(!verify_notification_signature(
            "TEST-SERVER-KEY",
            "UD-TEST123",
            "200",
            "1501",
            &expected
        ));
        // Panjang beda -> false (bukan panic).
        assert!(!verify_notification_signature(
            "TEST-SERVER-KEY",
            "UD-TEST123",
            "200",
            "1500",
            "abc"
        ));
    }

    #[test]
    fn table_transitions() {
        let db = test_db();
        let p1 = create_midtrans_payment(
            &db,
            "UD-MT-1",
            1500,
            "SNAP-1",
            "https://app.sandbox.midtrans.com/snap/v1/redirection/1",
            None,
        )
        .unwrap();
        assert_eq!(p1.status, MidtransPaymentStatus::Pending);
        assert_eq!(
            crate::lib::payments::parse_iso(&p1.expires_at).unwrap().timestamp_millis()
                - crate::lib::payments::parse_iso(&p1.created_at).unwrap().timestamp_millis(),
            30 * 60 * 1000
        );
        assert!(mark_midtrans_payment_paid(&db, "UD-MT-1").unwrap());
        assert!(!mark_midtrans_payment_paid(&db, "UD-MT-1").unwrap());
        assert_eq!(
            get_midtrans_payment(&db, "UD-MT-1").unwrap().unwrap().status,
            MidtransPaymentStatus::Paid
        );
        create_midtrans_payment(
            &db,
            "UD-MT-2",
            7500,
            "SNAP-2",
            "https://app.sandbox.midtrans.com/snap/v1/redirection/2",
            None,
        )
        .unwrap();
        assert_eq!(
            list_pending_midtrans_payments(&db, None)
                .unwrap()
                .iter()
                .map(|p| p.order_id.as_str())
                .collect::<Vec<_>>(),
            vec!["UD-MT-2"]
        );
        assert!(mark_midtrans_payment_failed(&db, "UD-MT-2").unwrap());
        assert_eq!(
            get_midtrans_payment(&db, "UD-MT-2").unwrap().unwrap().status,
            MidtransPaymentStatus::Failed
        );
        create_midtrans_payment(
            &db,
            "UD-MT-3",
            1500,
            "SNAP-3",
            "https://app.sandbox.midtrans.com/snap/v1/redirection/3",
            None,
        )
        .unwrap();
        // Buat kedaluwarsa secara manual.
        {
            let conn = db.lock().unwrap();
            conn.execute(
                "UPDATE midtrans_payments SET expires_at = ? WHERE order_id = ?",
                params![iso_of(Utc::now() - Duration::seconds(1)), "UD-MT-3"],
            )
            .unwrap();
        }
        assert!(
            list_expired_pending_midtrans_payments(&db, None)
                .unwrap()
                .iter()
                .any(|p| p.order_id == "UD-MT-3")
        );
        assert!(expire_midtrans_payments(&db, None).unwrap() >= 1);
        assert_eq!(
            get_midtrans_payment(&db, "UD-MT-3").unwrap().unwrap().status,
            MidtransPaymentStatus::Expired
        );
    }

    #[test]
    fn signature_wrong_key_fails() {
        // S34: signature dihitung dengan key lain -> tidak valid.
        let expected = hex::encode(Sha512::digest(b"UD-TEST1232001500TEST-SERVER-KEY"));
        assert!(!verify_notification_signature(
            "KEY-LAIN",
            "UD-TEST123",
            "200",
            "1500",
            &expected
        ));
        // Case-sensitive: hash huruf besar tetap diterima? (hex lowercase
        // dibanding apa adanya — huruf besar ditolak.)
        assert!(!verify_notification_signature(
            "TEST-SERVER-KEY",
            "UD-TEST123",
            "200",
            "1500",
            &expected.to_uppercase()
        ));
    }

    #[test]
    fn status_mapping_unknown_is_failed() {
        assert_eq!(map_midtrans_status("refund"), MidtransTxStatus::Failed);
        assert_eq!(map_midtrans_status(""), MidtransTxStatus::Failed);
        assert_eq!(map_midtrans_status("chargeback"), MidtransTxStatus::Failed);
    }

    #[test]
    fn midtrans_configured_needs_server_key() {
        let mut cfg = crate::config::Config::from_env().unwrap();
        cfg.midtrans_server_key = String::new();
        assert!(!midtrans_configured(&cfg));
        cfg.midtrans_server_key = "TEST-SERVER-KEY".to_string();
        assert!(midtrans_configured(&cfg));
    }

    #[test]
    fn create_payment_rejects_duplicate_order_id() {
        let db = test_db();
        create_midtrans_payment(&db, "UD-DUP", 2000, "SNAP-1", "https://x/1", None).unwrap();
        assert!(create_midtrans_payment(&db, "UD-DUP", 2000, "SNAP-2", "https://x/2", None).is_err());
        assert!(get_midtrans_payment(&db, "UD-TIDAKADA").unwrap().is_none());
        assert!(!mark_midtrans_payment_paid(&db, "UD-TIDAKADA").unwrap());
        assert!(!mark_midtrans_payment_failed(&db, "UD-TIDAKADA").unwrap());
    }

    #[test]
    fn expiry_sweep_only_touches_expired() {
        let db = test_db();
        let past = chrono::Utc::now() - chrono::Duration::minutes(31);
        create_midtrans_payment(&db, "UD-OLD", 2000, "SNAP-OLD", "https://x/old", Some(past)).unwrap();
        create_midtrans_payment(&db, "UD-NEW", 2000, "SNAP-NEW", "https://x/new", None).unwrap();
        assert_eq!(expire_midtrans_payments(&db, None).unwrap(), 1);
        assert_eq!(
            get_midtrans_payment(&db, "UD-OLD").unwrap().unwrap().status,
            MidtransPaymentStatus::Expired
        );
        assert_eq!(
            get_midtrans_payment(&db, "UD-NEW").unwrap().unwrap().status,
            MidtransPaymentStatus::Pending
        );
        // Sweep kedua: tidak ada lagi.
        assert_eq!(expire_midtrans_payments(&db, None).unwrap(), 0);
    }

    #[test]
    fn auth_header_basic() {
        assert_eq!(
            auth_header("TEST-SERVER-KEY"),
            format!(
                "Basic {}",
                base64::Engine::encode(
                    &base64::engine::general_purpose::STANDARD,
                    "TEST-SERVER-KEY:"
                )
            )
        );
    }

    // ------------------------------------------------- Gap 2: mock Midtrans

    /// Jalankan mock server Midtrans lokal (127.0.0.1, port ephemeral).
    /// Kembalikan base URL + handle server (di-abort pemanggil).
    async fn start_mock_server(
        router: axum::Router,
    ) -> (String, tokio::task::JoinHandle<()>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind mock midtrans");
        let base = format!("http://127.0.0.1:{}", listener.local_addr().unwrap().port());
        let handle = tokio::spawn(async move {
            axum::serve(listener, router).await.unwrap();
        });
        (base, handle)
    }

    fn test_cfg_with_key() -> crate::config::Config {
        let mut cfg = crate::config::Config::from_env().unwrap();
        cfg.midtrans_server_key = "TEST-SERVER-KEY".to_string();
        cfg
    }

    #[tokio::test]
    async fn snap_create_with_mock_returns_token_and_redirect() {
        // Gap 2a: create_snap_transaction_with melawan mock Snap lokal.
        let router = axum::Router::new().route(
            "/snap/v1/transactions",
            axum::routing::post(
                |headers: axum::http::HeaderMap, body: axum::body::Bytes| async move {
                    // Auth Basic dari server key harus terkirim.
                    assert_eq!(
                        headers
                            .get(axum::http::header::AUTHORIZATION)
                            .and_then(|v| v.to_str().ok()),
                        Some(auth_header("TEST-SERVER-KEY").as_str())
                    );
                    let v: serde_json::Value =
                        serde_json::from_slice(&body).expect("body snap harus JSON");
                    assert_eq!(v["transaction_details"]["order_id"], "UD-MOCK-1");
                    assert_eq!(v["transaction_details"]["gross_amount"], 2000);
                    assert_eq!(v["enabled_payments"], serde_json::json!(["qris"]));
                    axum::Json(serde_json::json!({
                        "token": "SNAP-MOCK-TOKEN",
                        "redirect_url": "https://app.sandbox.midtrans.com/snap/v1/redirection/mock",
                    }))
                },
            ),
        );
        let (base, server) = start_mock_server(router).await;
        let client = MidtransClient::with_bases(reqwest::Client::new(), &base, &base);
        let cfg = test_cfg_with_key();
        let snap = create_snap_transaction_with(&client, &cfg, "UD-MOCK-1", 2000, "Muse AI Token 1B")
            .await
            .expect("snap mock harus sukses");
        assert_eq!(snap.token, "SNAP-MOCK-TOKEN");
        assert_eq!(
            snap.redirect_url,
            "https://app.sandbox.midtrans.com/snap/v1/redirection/mock"
        );
        server.abort();
    }

    #[tokio::test]
    async fn snap_create_non_2xx_bails_with_error_messages() {
        // Gap 2a: jalur error — Snap menjawab 401 + error_messages.
        let router = axum::Router::new().route(
            "/snap/v1/transactions",
            axum::routing::post(|| async {
                (
                    axum::http::StatusCode::UNAUTHORIZED,
                    axum::Json(serde_json::json!({
                        "error_messages": ["Access denied due to unauthorized transaction"],
                    })),
                )
            }),
        );
        let (base, server) = start_mock_server(router).await;
        let client = MidtransClient::with_bases(reqwest::Client::new(), &base, &base);
        let cfg = test_cfg_with_key();
        let err = create_snap_transaction_with(&client, &cfg, "UD-MOCK-2", 2000, "Muse AI Token 1B")
            .await
            .expect_err("snap 401 harus bail");
        assert!(
            err.to_string().contains("Access denied"),
            "pesan error harus membawa error_messages Midtrans: {err}"
        );
        server.abort();
    }

    #[tokio::test]
    async fn status_settlement_parsed_from_mock() {
        // Gap 2: get_transaction_status melawan mock status lokal.
        let router = axum::Router::new().route(
            "/v2/{order_id}/status",
            axum::routing::get(|axum::extract::Path(order_id): axum::extract::Path<String>| async move {
                axum::Json(serde_json::json!({
                    "transaction_status": "settlement",
                    "status_code": "200",
                    "gross_amount": "2000",
                    "payment_type": "qris",
                    "order_id": order_id,
                }))
            }),
        );
        let (base, server) = start_mock_server(router).await;
        let client = MidtransClient::with_bases(reqwest::Client::new(), &base, &base);
        let cfg = test_cfg_with_key();
        let st = get_transaction_status(&client, &cfg, "UD-MOCK-3")
            .await
            .expect("status mock harus sukses");
        assert_eq!(st.transaction_status, "settlement");
        assert_eq!(st.status_code, "200");
        assert_eq!(map_midtrans_status(&st.transaction_status), MidtransTxStatus::Paid);
        server.abort();
    }

    #[tokio::test]
    async fn status_404_is_pending_not_error() {
        // Gap 2: transaksi belum ada di Midtrans (HTTP 404) -> pending.
        let router = axum::Router::new().route(
            "/v2/{order_id}/status",
            axum::routing::get(|| async {
                (
                    axum::http::StatusCode::NOT_FOUND,
                    axum::Json(serde_json::json!({
                        "status_code": "404",
                        "status_message": "Transaction doesn't exist",
                    })),
                )
            }),
        );
        let (base, server) = start_mock_server(router).await;
        let client = MidtransClient::with_bases(reqwest::Client::new(), &base, &base);
        let cfg = test_cfg_with_key();
        let st = get_transaction_status(&client, &cfg, "UD-BELUM-ADA")
            .await
            .expect("404 harus dipetakan jadi pending, bukan error");
        assert_eq!(st.transaction_status, "pending");
        assert_eq!(map_midtrans_status(&st.transaction_status), MidtransTxStatus::Pending);
        server.abort();
    }

    #[test]
    fn midtrans_client_from_config_uses_sandbox_by_default() {
        // Seam tidak mengubah default produksi: sandbox kalau flag false.
        // Tahan kunci env supaya tes Gap 3 (yang mengeset override) tidak
        // berjalan bersamaan.
        let _env_guard = MIDTRANS_ENV_LOCK.lock().unwrap();
        let mut cfg = crate::config::Config::from_env().unwrap();
        cfg.midtrans_is_production = false;
        // Pastikan env override tidak ikut campur di tes ini.
        std::env::remove_var("MIDTRANS_SNAP_BASE_URL");
        std::env::remove_var("MIDTRANS_API_BASE_URL");
        let c = MidtransClient::from_config(&cfg);
        assert_eq!(c.snap_base, "https://app.sandbox.midtrans.com");
        assert_eq!(c.api_base, "https://api.sandbox.midtrans.com");
        cfg.midtrans_is_production = true;
        let c = MidtransClient::from_config(&cfg);
        assert_eq!(c.snap_base, "https://app.midtrans.com");
        assert_eq!(c.api_base, "https://api.midtrans.com");
    }
}
