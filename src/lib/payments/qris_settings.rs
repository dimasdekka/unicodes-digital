//! Pengaturan QRIS GoPay (merchant) untuk panel admin — port dari
//! `src/lib/payments/qris-settings.ts`.
//!
//! String QRIS statis disimpan di tabel settings (bukan di env/file),
//! supaya Dim bisa menempelnya sendiri dari /admin tanpa kirim lewat chat.
//! Nilai ini TIDAK pernah di-echo lengkap ke client — API hanya mengembalikan
//! info merchant hasil parse (nama/kota/ID).
//!
//! CATATAN: `get_setting` / `set_setting` tinggal di `crate::db` (port
//! `getSetting` / `setSetting` dari `db.ts`). Modul ini memakai yang itu
//! lewat `get_gopay_static_qris` / `set_gopay_static_qris` di bawah —
//! jangan mendefinisikan ulang di sini.

use crate::db::{get_setting, set_setting, Db};
use rusqlite::params;

use super::qris::{
    get_qris_tag, is_valid_qris_checksum, parse_qris_payload, row_from_sql, QrisPaymentRow,
};

/// Key settings untuk string QRIS statis GoBiz.
pub const GOPAY_STATIC_QRIS_KEY: &str = "gopay_static_qris";

pub fn get_gopay_static_qris(db: &Db) -> anyhow::Result<Option<String>> {
    Ok(get_setting(db, GOPAY_STATIC_QRIS_KEY))
}

pub fn set_gopay_static_qris(db: &Db, value: &str) -> anyhow::Result<()> {
    set_setting(db, GOPAY_STATIC_QRIS_KEY, value)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QrisMerchantInfo {
    pub merchant_name: Option<String>,
    pub merchant_city: Option<String>,
    pub merchant_id: Option<String>,
}

/// Ambil info merchant dari payload QRIS statis: tag 59 (nama), tag 60 (kota),
/// dan sub-tag 02 di dalam tag 26 (ID merchant; fallback tag 27 kalau 26 kosong).
/// Error dengan pesan jelas kalau payload bukan QRIS / rusak.
pub fn extract_merchant_info(payload: &str) -> anyhow::Result<QrisMerchantInfo> {
    let trimmed = payload.trim();
    if trimmed.is_empty() {
        anyhow::bail!("String QRIS kosong");
    }
    let tlvs = parse_qris_payload(trimmed)
        .map_err(|e| anyhow::anyhow!("String QRIS tidak bisa dibaca: {e}"))?;
    if get_qris_tag(&tlvs, "00").is_none() {
        anyhow::bail!("Bukan payload QRIS (tag 00 tidak ada)");
    }
    let mut merchant_id: Option<String> = None;
    for tag in ["26", "27"] {
        let Some(nested) = get_qris_tag(&tlvs, tag) else {
            continue;
        };
        if let Ok(sub) = parse_qris_payload(&nested) {
            if let Some(id) = get_qris_tag(&sub, "02") {
                merchant_id = Some(id);
                break;
            }
        }
        // Tag merchant rusak — abaikan, lanjut ke tag berikutnya.
    }
    Ok(QrisMerchantInfo {
        merchant_name: get_qris_tag(&tlvs, "59"),
        merchant_city: get_qris_tag(&tlvs, "60"),
        merchant_id,
    })
}

/// Validasi string QRIS statis sebelum disimpan: payload harus bisa di-parse,
/// checksum CRC16 harus valid (payload hasil edit tangan / terpotong ditolak),
/// dan harus ada tag 00. Kembalikan info merchant kalau valid.
pub fn validate_static_qris(payload: &str) -> anyhow::Result<QrisMerchantInfo> {
    let trimmed = payload.trim();
    if trimmed.is_empty() {
        anyhow::bail!("String QRIS kosong");
    }
    if !is_valid_qris_checksum(trimmed) {
        anyhow::bail!(
            "Checksum QRIS tidak valid — string mungkin terpotong atau salah salin. Salin ulang dari GoBiz."
        );
    }
    extract_merchant_info(trimmed)
}

/// 20 pembayaran QRIS terbaru (untuk tabel admin).
pub fn list_recent_qris_payments(db: &Db, limit: i64) -> anyhow::Result<Vec<QrisPaymentRow>> {
    let conn = db.lock().unwrap();
    let mut stmt = conn.prepare("SELECT * FROM qris_payments ORDER BY created_at DESC LIMIT ?")?;
    let rows = stmt.query_map(params![limit], row_from_sql)?;
    rows.collect::<Result<Vec<_>, _>>().map_err(anyhow::Error::from)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lib::payments::qris::{create_qris_payment, serialize_qris_payload, QrisTlv};
    use chrono::Utc;
    use rusqlite::Connection;
    use std::sync::{Arc, Mutex};

    fn test_db() -> Db {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE settings (
              key TEXT PRIMARY KEY,
              value TEXT NOT NULL,
              updated_at TEXT NOT NULL
            );
            CREATE TABLE qris_payments (
              id TEXT PRIMARY KEY,
              reference TEXT NOT NULL,
              amount_idr INTEGER NOT NULL,
              unique_amount_idr INTEGER NOT NULL,
              qr_string TEXT NOT NULL,
              status TEXT NOT NULL DEFAULT 'pending',
              created_at TEXT NOT NULL,
              expires_at TEXT NOT NULL
            );",
        )
        .unwrap();
        Arc::new(Mutex::new(conn))
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

    #[test]
    fn gopay_static_qris_round_trip() {
        // Wrapper di atas `crate::db::{get,set}_setting` (round-trip
        // get/set mentahnya sudah diuji di `db.rs::settings_round_trip`).
        let db = test_db();
        assert_eq!(get_gopay_static_qris(&db).unwrap(), None);
        set_gopay_static_qris(&db, "QRIS-TEST-1").unwrap();
        assert_eq!(
            get_gopay_static_qris(&db).unwrap().as_deref(),
            Some("QRIS-TEST-1")
        );
        set_gopay_static_qris(&db, "QRIS-TEST-2").unwrap();
        assert_eq!(
            get_gopay_static_qris(&db).unwrap().as_deref(),
            Some("QRIS-TEST-2")
        );
    }

    #[test]
    fn validate_and_extract() {
        let payload = static_payload();
        let info = validate_static_qris(&payload).unwrap();
        assert_eq!(info.merchant_name.as_deref(), Some("TOKO TEST QRIS"));
        assert_eq!(info.merchant_city.as_deref(), Some("JAKARTA"));
        assert_eq!(info.merchant_id.as_deref(), Some("ID1234567890123"));
        let mut bad_crc = payload.clone();
        let last = bad_crc.pop().unwrap();
        bad_crc.push(if last == '0' { '1' } else { '0' });
        assert!(validate_static_qris(&bad_crc).is_err());
        assert!(validate_static_qris(&payload[..20]).is_err());
        assert!(validate_static_qris("").is_err());
        assert_eq!(
            validate_static_qris(&format!("  {payload}\n"))
                .unwrap()
                .merchant_name
                .as_deref(),
            Some("TOKO TEST QRIS"),
            "spasi di ujung diterima (di-trim)"
        );
        let no_merchant = serialize_qris_payload(&[
            QrisTlv { tag: "00".into(), value: "01".into() },
            QrisTlv { tag: "01".into(), value: "11".into() },
            QrisTlv { tag: "59".into(), value: "TOKO TANPA ID".into() },
            QrisTlv { tag: "60".into(), value: "BANDUNG".into() },
        ])
        .unwrap();
        let info2 = extract_merchant_info(&no_merchant).unwrap();
        assert_eq!(info2.merchant_id, None);
        assert_eq!(info2.merchant_name.as_deref(), Some("TOKO TANPA ID"));
        assert!(extract_merchant_info("hello world, bukan qris").is_err());
    }

    #[test]
    fn recent_payments() {
        let db = test_db();
        assert!(list_recent_qris_payments(&db, 20).unwrap().is_empty());
        let created = create_qris_payment(
            &db,
            "ADMIN-TEST-UNIT",
            1500,
            &static_payload(),
            Some(Utc::now()),
        )
        .unwrap();
        let recent = list_recent_qris_payments(&db, 20).unwrap();
        assert_eq!(recent.len(), 1);
        assert_eq!(recent[0].reference, "ADMIN-TEST-UNIT");
        assert_eq!(recent[0].unique_amount_idr, created.payment.unique_amount_idr);
        assert_eq!(
            recent[0].status,
            crate::lib::payments::qris::QrisPaymentStatus::Pending
        );
    }
}
