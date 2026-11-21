//! QRIS dinamis — port dari `src/lib/payments/qris.ts` (versi TS).
//!
//! Semuanya ditulis dari nol (tanpa library QRIS pihak ketiga):
//!   1. parser payload QRIS statis format EMVCo (TLV),
//!   2. builder QRIS dinamis (suntik tag 54 nominal + tag 53 mata uang IDR,
//!      ubah tag 01 jadi dinamis, hitung ulang CRC16),
//!   3. generator gambar QR (PNG) dari string payload,
//!   4. alokator nominal unik (supaya pembayaran bisa dibedakan),
//!   5. store pembayaran QRIS di SQLite,
//!   6. matcher transaksi feed -> pembayaran pending,
//!   7. expiry 5 menit.
//!
//! Standar yang dipakai: EMVCo QR Code (dipakai QRIS Indonesia).
//! - TLV: tag 2 digit + length 2 digit (PANJANG BYTE UTF-8, bukan karakter)
//!   + value.
//! - CRC16 tag 63: CRC-16/CCITT-FALSE (poly 0x1021, init 0xFFFF, tanpa
//!   refleksi, tanpa XOR akhir), dihitung atas payload termasuk "6304"
//!   (tag + length CRC, tanpa nilai), hasilnya 4 hex huruf besar.
//! - Tag 01: "11" statis, "12" dinamis. Tag 53: kode mata uang numerik,
//!   "360" = IDR. Tag 54: nominal transaksi (rupiah, bilangan bulat).

use crate::db::Db;
use chrono::{DateTime, Duration, Utc};
use rusqlite::{params, OptionalExtension};
use std::collections::HashSet;
use std::io::Cursor;

use super::{iso_of, parse_iso};

/// Pembayaran pending kedaluwarsa setelah 5 menit (satu tempat, satu konstanta).
pub const QRIS_PAYMENT_TTL_MS: i64 = 5 * 60 * 1000;
/// Kode mata uang EMVCo untuk rupiah.
pub const QRIS_CURRENCY_IDR: &str = "360";
/// Jendela offset nominal unik: delta 1..999 rupiah di atas nominal dasar.
pub const QRIS_MAX_UNIQUE_OFFSET: i64 = 999;
/// Toleransi selisih jam saat mencocokkan waktu transaksi feed.
pub const QRIS_CLOCK_SKEW_MS: i64 = 60 * 1000;

const TAG_PAYLOAD_FORMAT: &str = "00";
const TAG_POINT_OF_INITIATION: &str = "01";
const TAG_CURRENCY: &str = "53";
const TAG_AMOUNT: &str = "54";
const TAG_CRC: &str = "63";
const POI_DYNAMIC: &str = "12";

// ---------------------------------------------------------------------------
// CRC-16/CCITT-FALSE (implementasi table-driven, ditulis sendiri)
// ---------------------------------------------------------------------------

const CRC16_POLY: u16 = 0x1021;
const CRC16_INIT: u16 = 0xffff;

fn crc16_table() -> &'static [u16; 256] {
    static TABLE: std::sync::OnceLock<[u16; 256]> = std::sync::OnceLock::new();
    TABLE.get_or_init(|| {
        let mut table = [0u16; 256];
        for (n, slot) in table.iter_mut().enumerate() {
            let mut crc: u16 = (n as u16) << 8;
            for _ in 0..8 {
                crc = if crc & 0x8000 != 0 {
                    (crc << 1) ^ CRC16_POLY
                } else {
                    crc << 1
                };
            }
            *slot = crc;
        }
        table
    })
}

/// CRC-16/CCITT-FALSE atas string (byte UTF-8) -> 4 hex huruf besar.
pub fn crc16_ccitt_false(input: &str) -> String {
    let table = crc16_table();
    let mut crc = CRC16_INIT;
    for &b in input.as_bytes() {
        crc = (crc << 8) ^ table[(((crc >> 8) ^ b as u16) & 0xff) as usize];
    }
    format!("{crc:04X}")
}

// ---------------------------------------------------------------------------
// Parser + serializer TLV
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QrisTlv {
    pub tag: String,
    pub value: String,
}

fn is_two_digits(s: &str) -> bool {
    s.len() == 2 && s.bytes().all(|b| b.is_ascii_digit())
}

/// Parse payload QRIS/EMVCo jadi daftar TLV berurutan (urutan asli
/// dipertahankan, tag yang sama boleh muncul berkali-kali, mis. 26 dan 27).
/// Error kalau rusak: TLV tidak lengkap, tag/length bukan 2 digit, atau
/// value terpotong. Panjang dihitung dalam BYTE UTF-8 supaya nama merchant
/// non-ASCII tetap benar.
pub fn parse_qris_payload(payload: &str) -> anyhow::Result<Vec<QrisTlv>> {
    if payload.chars().count() < 8 {
        anyhow::bail!("Payload QRIS terlalu pendek atau bukan string");
    }
    let bytes = payload.as_bytes();
    let mut out = Vec::new();
    let mut cursor = 0usize;
    while cursor < bytes.len() {
        if cursor + 4 > bytes.len() {
            anyhow::bail!("Payload QRIS rusak di byte {cursor}: TLV tidak lengkap");
        }
        let tag = String::from_utf8_lossy(&bytes[cursor..cursor + 2]).into_owned();
        cursor += 2;
        let length_text =
            String::from_utf8_lossy(&bytes[cursor..cursor + 2]).into_owned();
        cursor += 2;
        if !is_two_digits(&tag) || !is_two_digits(&length_text) {
            anyhow::bail!(
                "Payload QRIS rusak di byte {}: tag/length bukan 2 digit",
                cursor - 4
            );
        }
        let length: usize = length_text.parse().unwrap_or(0);
        if cursor + length > bytes.len() {
            anyhow::bail!(
                "Payload QRIS rusak: tag {tag} mengaku {length} byte tapi payload habis"
            );
        }
        let value =
            String::from_utf8_lossy(&bytes[cursor..cursor + length]).into_owned();
        cursor += length;
        out.push(QrisTlv { tag, value });
    }
    Ok(out)
}

/// Ambil nilai tag pertama yang cocok (None kalau tidak ada).
pub fn get_qris_tag(tlvs: &[QrisTlv], tag: &str) -> Option<String> {
    tlvs.iter().find(|t| t.tag == tag).map(|t| t.value.clone())
}

fn set_qris_tag(tlvs: &mut Vec<QrisTlv>, tag: &str, value: &str) {
    if let Some(existing) = tlvs.iter_mut().find(|t| t.tag == tag) {
        existing.value = value.to_string();
    } else {
        tlvs.push(QrisTlv {
            tag: tag.to_string(),
            value: value.to_string(),
        });
    }
}

/// Serialisasi daftar TLV jadi payload lengkap: tag diurutkan menaik secara
/// numerik (CRC selalu terakhir), lalu CRC16 dihitung atas body + "6304"
/// dan ditempel sebagai nilai tag 63.
pub fn serialize_qris_payload(tlvs: &[QrisTlv]) -> anyhow::Result<String> {
    let mut body_parts: Vec<String> = Vec::new();
    let mut sorted: Vec<&QrisTlv> = tlvs.iter().filter(|t| t.tag != TAG_CRC).collect();
    sorted.sort_by_key(|t| t.tag.parse::<u32>().unwrap_or(0));
    for tlv in sorted {
        let byte_length = tlv.value.len();
        if byte_length > 99 {
            anyhow::bail!(
                "Nilai tag {} {byte_length} byte, melebihi batas 99 byte TLV",
                tlv.tag
            );
        }
        body_parts.push(format!("{}{:02}{}", tlv.tag, byte_length, tlv.value));
    }
    let with_crc_header = format!("{}{}04", body_parts.concat(), TAG_CRC);
    Ok(format!(
        "{}{}",
        with_crc_header,
        crc16_ccitt_false(&with_crc_header)
    ))
}

/// Validasi CRC trailing payload: CRC dihitung atas seluruh payload kecuali
/// 4 karakter terakhir, lalu dibandingkan (case-insensitive) dengan 4
/// karakter terakhir itu.
pub fn is_valid_qris_checksum(payload: &str) -> bool {
    if payload.chars().count() < 8 {
        return false;
    }
    if payload.len() < 4 {
        return false;
    }
    let provided = payload[payload.len() - 4..].to_uppercase();
    if provided.len() != 4 || !provided.bytes().all(|b| b.is_ascii_hexdigit()) {
        return false;
    }
    crc16_ccitt_false(&payload[..payload.len() - 4]) == provided
}

// ---------------------------------------------------------------------------
// Builder QRIS dinamis
// ---------------------------------------------------------------------------

/// Ubah payload QRIS statis jadi dinamis untuk satu nominal: verifikasi dulu
/// checksum sumbernya (payload rusak / hasil edit tangan DITOLAK — supaya QR
/// tidak mengarahkan pembayaran ke merchant lain), lalu set tag 01 = "12",
/// tag 53 = "360" (IDR), tag 54 = nominal rupiah, dan hitung ulang CRC.
pub fn build_dynamic_qris(static_payload: &str, amount_idr: i64) -> anyhow::Result<String> {
    if amount_idr <= 0 {
        anyhow::bail!("Nominal QRIS harus bilangan bulat positif (rupiah)");
    }
    if !is_valid_qris_checksum(static_payload) {
        anyhow::bail!(
            "Checksum QRIS statis tidak valid — menolak membangun QR bayar dari payload ini"
        );
    }
    let tlvs = parse_qris_payload(static_payload)?;
    if get_qris_tag(&tlvs, TAG_PAYLOAD_FORMAT).is_none() {
        anyhow::bail!("Input bukan payload QRIS (tag 00 tidak ada)");
    }
    let mut next: Vec<QrisTlv> = tlvs.into_iter().filter(|t| t.tag != TAG_CRC).collect();
    set_qris_tag(&mut next, TAG_POINT_OF_INITIATION, POI_DYNAMIC);
    set_qris_tag(&mut next, TAG_CURRENCY, QRIS_CURRENCY_IDR);
    set_qris_tag(&mut next, TAG_AMOUNT, &amount_idr.to_string());
    serialize_qris_payload(&next)
}

// ---------------------------------------------------------------------------
// Gambar QR (PNG)
// ---------------------------------------------------------------------------

/// Render string payload jadi PNG yang bisa dikirim bot ke pembeli.
/// Pure Rust (crate `qrcode` + `image`), tanpa request jaringan.
///
/// Beda kecil dari TS: TS memakai `margin: 2` (quiet zone 2 modul);
/// crate `qrcode` memakai quiet zone standar 4 modul. QR-nya tetap valid
/// dan terbaca scanner yang sama.
pub fn render_qris_png(qr_string: &str, width: u32) -> anyhow::Result<Vec<u8>> {
    use image::{DynamicImage, ImageFormat, Luma};
    use qrcode::{EcLevel, QrCode};
    let code = QrCode::with_error_correction_level(qr_string.as_bytes(), EcLevel::M)
        .map_err(|e| anyhow::anyhow!("QR tidak bisa dibuat: {e}"))?;
    let img = code
        .render::<Luma<u8>>()
        .max_dimensions(width, width)
        .build();
    let mut buf = Vec::new();
    DynamicImage::ImageLuma8(img).write_to(&mut Cursor::new(&mut buf), ImageFormat::Png)?;
    Ok(buf)
}

// ---------------------------------------------------------------------------
// Alokator nominal unik
// ---------------------------------------------------------------------------

/// Cari offset terkecil 1..=max_offset sehingga nominal dasar + offset belum
/// dipakai pembayaran aktif lain. Keunikan dicek pada HASIL (nominal dasar +
/// offset), bukan offsetnya saja: 3500+1 dan 3499+2 sama-sama mendarat di 3501.
/// Error kalau semua slot penuh.
pub fn allocate_unique_amount(
    base_amount: i64,
    taken_unique_amounts: impl IntoIterator<Item = i64>,
    max_offset: i64,
) -> anyhow::Result<(i64, i64)> {
    if base_amount <= 0 {
        anyhow::bail!("Nominal dasar harus bilangan bulat positif (rupiah)");
    }
    if max_offset < 1 {
        anyhow::bail!("maxOffset harus bilangan bulat positif");
    }
    let taken: HashSet<i64> = taken_unique_amounts.into_iter().collect();
    for offset in 1..=max_offset {
        let candidate = base_amount + offset;
        if !taken.contains(&candidate) {
            return Ok((candidate, offset));
        }
    }
    anyhow::bail!(
        "Tidak ada slot nominal unik untuk {base_amount} (offset 1..{max_offset} sudah terpakai semua)"
    )
}

// ---------------------------------------------------------------------------
// Store pembayaran QRIS (SQLite)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum QrisPaymentStatus {
    Pending,
    Paid,
    Expired,
    Cancelled,
}

impl QrisPaymentStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Paid => "paid",
            Self::Expired => "expired",
            Self::Cancelled => "cancelled",
        }
    }

    fn from_str(s: &str) -> anyhow::Result<Self> {
        match s {
            "pending" => Ok(Self::Pending),
            "paid" => Ok(Self::Paid),
            "expired" => Ok(Self::Expired),
            "cancelled" => Ok(Self::Cancelled),
            other => anyhow::bail!("Status qris_payments tidak dikenal: {other}"),
        }
    }
}

/// Baris tabel `qris_payments` — cerminan baris DB; tidak semua field
/// dibaca jalur produksi (dipakai tes + admin).
#[allow(dead_code)]
#[derive(Debug, Clone)]
pub struct QrisPaymentRow {
    pub id: String,
    pub reference: String,
    pub amount_idr: i64,
    pub unique_amount_idr: i64,
    pub qr_string: String,
    pub status: QrisPaymentStatus,
    pub created_at: String,
    pub expires_at: String,
}

pub(crate) fn row_from_sql(row: &rusqlite::Row) -> rusqlite::Result<QrisPaymentRow> {
    let status_str: String = row.get("status")?;
    Ok(QrisPaymentRow {
        id: row.get("id")?,
        reference: row.get("reference")?,
        amount_idr: row.get("amount_idr")?,
        unique_amount_idr: row.get("unique_amount_idr")?,
        qr_string: row.get("qr_string")?,
        status: QrisPaymentStatus::from_str(&status_str)
            .map_err(|_| rusqlite::Error::InvalidColumnName(status_str))?,
        created_at: row.get("created_at")?,
        expires_at: row.get("expires_at")?,
    })
}

pub struct CreatedQrisPayment {
    pub payment: QrisPaymentRow,
    pub qr_string: String,
}

/// Buat satu pembayaran QRIS: sapu dulu yang sudah kedaluwarsa (supaya slot
/// nominalnya bebas), alokasikan nominal unik, bangun payload dinamis, simpan
/// sebagai 'pending'. QR PNG-nya dibuat terpisah oleh pemanggil via
/// `render_qris_png(qr_string)`.
pub fn create_qris_payment(
    db: &Db,
    reference: &str,
    amount_idr: i64,
    static_payload: &str,
    now: Option<DateTime<Utc>>,
) -> anyhow::Result<CreatedQrisPayment> {
    let now_dt = now.unwrap_or_else(Utc::now);
    let created_at = iso_of(now_dt);
    let expires_at = iso_of(now_dt + Duration::milliseconds(QRIS_PAYMENT_TTL_MS));
    expire_qris_payments(db, Some(now_dt))?;
    // Alokasi nominal unik + insert dalam SATU transaksi (BEGIN IMMEDIATE):
    // dua pembeli bersamaan dengan nominal dasar sama tidak bisa dapat
    // unique_amount yang sama (tanpa ini, keduanya baca "taken" yang sama
    // lalu insert duplikat — pembayaran bisa nyasar ke order yang salah).
    let conn = db.lock().unwrap();
    conn.execute_batch("BEGIN IMMEDIATE")?;
    let result: anyhow::Result<CreatedQrisPayment> = (|| {
        let taken: Vec<i64> = {
            let mut stmt = conn.prepare(
                "SELECT unique_amount_idr FROM qris_payments WHERE status = 'pending' AND expires_at > ? ORDER BY created_at ASC",
            )?;
            let rows = stmt.query_map(params![created_at], |r| r.get(0))?;
            rows.collect::<Result<Vec<i64>, _>>()?
        };
        let (unique_amount, _offset) =
            allocate_unique_amount(amount_idr, taken, QRIS_MAX_UNIQUE_OFFSET)?;
        let qr_string = build_dynamic_qris(static_payload, unique_amount)?;
        let id = uuid::Uuid::new_v4().to_string();
        conn.execute(
            "INSERT INTO qris_payments (id, reference, amount_idr, unique_amount_idr, qr_string, status, created_at, expires_at) VALUES (?, ?, ?, ?, ?, 'pending', ?, ?)",
            params![id, reference, amount_idr, unique_amount, qr_string, created_at, expires_at],
        )?;
        let payment = conn
            .query_row(
                "SELECT * FROM qris_payments WHERE id = ?",
                params![id],
                row_from_sql,
            )
            .optional()?
            .ok_or_else(|| {
                anyhow::anyhow!("Gagal membaca kembali pembayaran QRIS yang baru dibuat")
            })?;
        Ok(CreatedQrisPayment { payment, qr_string })
    })();
    match result {
        Ok(created) => {
            conn.execute_batch("COMMIT")?;
            Ok(created)
        }
        Err(e) => {
            let _ = conn.execute_batch("ROLLBACK");
            Err(e)
        }
    }
}

/// Ambil satu pembayaran QRIS berdasarkan id.
/// Dipakai tes + admin; jalur produksi memakai `get_qris_payment_by_reference`.
#[allow(dead_code)]
pub fn get_qris_payment(db: &Db, id: &str) -> anyhow::Result<Option<QrisPaymentRow>> {
    let conn = db.lock().unwrap();
    Ok(conn
        .query_row(
            "SELECT * FROM qris_payments WHERE id = ?",
            params![id],
            row_from_sql,
        )
        .optional()?)
}

/// Pembayaran QRIS terbaru untuk satu referensi (order UD-… / top-up TOPUP-…).
pub fn get_qris_payment_by_reference(
    db: &Db,
    reference: &str,
) -> anyhow::Result<Option<QrisPaymentRow>> {
    let conn = db.lock().unwrap();
    Ok(conn
        .query_row(
            "SELECT * FROM qris_payments WHERE reference = ? ORDER BY created_at DESC LIMIT 1",
            params![reference],
            row_from_sql,
        )
        .optional()?)
}

/// Pembayaran pending yang belum kedaluwarsa (nominalnya masih "dipegang").
pub fn list_pending_qris_payments(
    db: &Db,
    now: Option<DateTime<Utc>>,
) -> anyhow::Result<Vec<QrisPaymentRow>> {
    let now_iso = iso_of(now.unwrap_or_else(Utc::now));
    let conn = db.lock().unwrap();
    let mut stmt = conn.prepare(
        "SELECT * FROM qris_payments WHERE status = 'pending' AND expires_at > ? ORDER BY created_at ASC",
    )?;
    let rows = stmt.query_map(params![now_iso], row_from_sql)?;
    rows.collect::<Result<Vec<_>, _>>().map_err(anyhow::Error::from)
}

/// Tandai semua pending yang lewat expires_at jadi 'expired'. Kembalikan jumlahnya.
pub fn expire_qris_payments(db: &Db, now: Option<DateTime<Utc>>) -> anyhow::Result<usize> {
    let now_iso = iso_of(now.unwrap_or_else(Utc::now));
    let conn = db.lock().unwrap();
    Ok(conn.execute(
        "UPDATE qris_payments SET status = 'expired' WHERE status = 'pending' AND expires_at <= ?",
        params![now_iso],
    )?)
}

/// Daftar pembayaran pending yang sudah lewat expires_at (belum disapu).
/// Dipakai poller supaya order yang QR-nya kedaluwarsa bisa dibatalkan.
pub fn list_expired_pending_qris_payments(
    db: &Db,
    now: Option<DateTime<Utc>>,
) -> anyhow::Result<Vec<QrisPaymentRow>> {
    let now_iso = iso_of(now.unwrap_or_else(Utc::now));
    let conn = db.lock().unwrap();
    let mut stmt = conn.prepare(
        "SELECT * FROM qris_payments WHERE status = 'pending' AND expires_at <= ? ORDER BY created_at ASC",
    )?;
    let rows = stmt.query_map(params![now_iso], row_from_sql)?;
    rows.collect::<Result<Vec<_>, _>>().map_err(anyhow::Error::from)
}

/// Tandai pending jadi 'paid'. False kalau id tidak ada / sudah bukan pending.
pub fn mark_qris_payment_paid(db: &Db, id: &str) -> anyhow::Result<bool> {
    let conn = db.lock().unwrap();
    Ok(conn.execute(
        "UPDATE qris_payments SET status = 'paid' WHERE id = ? AND status = 'pending'",
        params![id],
    )? > 0)
}

/// Batalkan pending (nominal uniknya bebas lagi untuk pembayaran berikutnya).
/// Dipakai tes + admin; jalur produksi memakai expiry sweep.
#[allow(dead_code)]
pub fn mark_qris_payment_cancelled(db: &Db, id: &str) -> anyhow::Result<bool> {
    let conn = db.lock().unwrap();
    Ok(conn.execute(
        "UPDATE qris_payments SET status = 'cancelled' WHERE id = ? AND status = 'pending'",
        params![id],
    )? > 0)
}

// ---------------------------------------------------------------------------
// Matcher transaksi feed -> pembayaran pending
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct FeedTransaction {
    /// ID transaksi di feed provider (untuk dedupe di reconcile).
    pub id: String,
    /// Nominal yang masuk (rupiah).
    pub amount_idr: i64,
    /// Waktu transaksi; None kalau feed tidak memberi waktu (cocok nominal saja).
    pub time: Option<DateTime<Utc>>,
    /// Status dari feed; kosong/tidak dikenal = dianggap sukses (fail-open).
    pub status: Option<String>,
}

const FEED_FAILURE_STATUSES: &[&str] = &[
    "failed", "reversed", "cancelled", "canceled", "expired", "declined",
];

/// Apakah satu transaksi feed melunasi satu pembayaran pending?
/// Aturan: nominal harus SAMA PERSIS dengan unique_amount pembayaran; waktu
/// transaksi harus di dalam jendela [created_at, expires_at] (dengan toleransi
/// selisih jam); status gagal yang sudah dikenal ditolak; status kosong/tidak
/// dikenal diterima (fail-open — feed tiap provider beda-beda labelnya).
pub fn match_feed_transaction(
    tx: &FeedTransaction,
    payment: &QrisPaymentRow,
    clock_skew_ms: Option<i64>,
) -> bool {
    if payment.status != QrisPaymentStatus::Pending {
        return false;
    }
    if tx.amount_idr != payment.unique_amount_idr {
        return false;
    }
    let status = tx.status.clone().unwrap_or_default().trim().to_lowercase();
    if !status.is_empty() && FEED_FAILURE_STATUSES.contains(&status.as_str()) {
        return false;
    }
    let Some(tx_time) = tx.time else {
        return true;
    };
    let skew = clock_skew_ms.unwrap_or(QRIS_CLOCK_SKEW_MS);
    let start = match parse_iso(&payment.created_at) {
        Ok(t) => t.timestamp_millis() - skew,
        Err(_) => return false,
    };
    let end = match parse_iso(&payment.expires_at) {
        Ok(t) => t.timestamp_millis() + skew,
        Err(_) => return false,
    };
    let t = tx_time.timestamp_millis();
    t >= start && t <= end
}

/// Cari pembayaran pending pertama yang cocok dengan satu transaksi feed.
/// Dipakai tes; jalur produksi memakai `reconcile_feed_transactions`.
#[allow(dead_code)]
pub fn find_qris_payment_for_transaction<'a>(
    tx: &FeedTransaction,
    pending: &'a [QrisPaymentRow],
    clock_skew_ms: Option<i64>,
) -> Option<&'a QrisPaymentRow> {
    pending
        .iter()
        .find(|p| match_feed_transaction(tx, p, clock_skew_ms))
}

/// Cocokkan banyak transaksi ke banyak pembayaran pending. Setiap transaksi
/// dipakai paling banyak satu kali (supaya tidak melunasi ganda), pembayaran
/// diurutkan dari unique_amount terkecil supaya hasilnya deterministik.
pub fn reconcile_feed_transactions<'a>(
    pending: &'a [QrisPaymentRow],
    transactions: &'a [FeedTransaction],
    clock_skew_ms: Option<i64>,
) -> Vec<(&'a QrisPaymentRow, &'a FeedTransaction)> {
    let mut ordered: Vec<&QrisPaymentRow> = pending.iter().collect();
    ordered.sort_by_key(|p| p.unique_amount_idr);
    let mut used: HashSet<&str> = HashSet::new();
    let mut matches = Vec::new();
    for payment in ordered {
        for tx in transactions {
            if used.contains(tx.id.as_str()) {
                continue;
            }
            if match_feed_transaction(tx, payment, clock_skew_ms) {
                matches.push((payment, tx));
                used.insert(tx.id.as_str());
                break;
            }
        }
    }
    matches
}

// ---------------------------------------------------------------------------
// Tes (cerminan qris.test.ts — vektor sintetis spek EMVCo, BUKAN merchant asli)
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;
    use std::sync::{Arc, Mutex};

    fn test_db() -> Db {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE qris_payments (
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

    fn test_vector_tlvs() -> Vec<QrisTlv> {
        vec![
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
        ]
    }

    fn static_payload() -> String {
        serialize_qris_payload(&test_vector_tlvs()).unwrap()
    }

    /// CRC16 independen (bitwise, jalur berbeda dari table-driven modul).
    fn crc16_independent(input: &str) -> String {
        let mut crc: u16 = 0xffff;
        for &b in input.as_bytes() {
            crc ^= (b as u16) << 8;
            for _ in 0..8 {
                crc = if crc & 0x8000 != 0 {
                    (crc << 1) ^ 0x1021
                } else {
                    crc << 1
                };
            }
        }
        format!("{crc:04X}")
    }

    #[test]
    fn parse_round_trip() {
        let payload = static_payload();
        let parsed = parse_qris_payload(&payload).unwrap();
        assert_eq!(parsed.len(), 9, "9 tag terbaca (8 + CRC 63)");
        assert_eq!(parsed[8].tag, "63");
        assert!(parsed[8].value.chars().all(|c| c.is_ascii_hexdigit()));
        assert_eq!(
            get_qris_tag(&parsed, "26").as_deref(),
            Some("0014ID.CO.QRIS.WWW011693600123456789010215ID1234567890123")
        );
        assert_eq!(serialize_qris_payload(&parsed).unwrap(), payload);
        assert_eq!(get_qris_tag(&parsed, "59").as_deref(), Some("TOKO TEST QRIS"));
        assert!(parse_qris_payload("00020101021126").is_err());
        assert!(parse_qris_payload("00AB01").is_err());
        assert!(parse_qris_payload("").is_err());
    }

    #[test]
    fn crc_matches_independent() {
        let payload = static_payload();
        assert!(is_valid_qris_checksum(&payload));
        assert_eq!(
            crc16_ccitt_false(&payload[..payload.len() - 4]),
            crc16_independent(&payload[..payload.len() - 4])
        );
        assert_eq!(
            crc16_independent(&payload[..payload.len() - 4]),
            &payload[payload.len() - 4..]
        );
        let tampered = payload.replace("TOKO TEST QRIS", "TOKO TEST QRIT");
        assert!(!is_valid_qris_checksum(&tampered));
        assert!(!is_valid_qris_checksum("1234"));
    }

    #[test]
    fn dynamic_builder() {
        let payload = static_payload();
        let dynamic = build_dynamic_qris(&payload, 1500).unwrap();
        let parsed = parse_qris_payload(&dynamic).unwrap();
        assert_eq!(get_qris_tag(&parsed, "01").as_deref(), Some("12"));
        assert_eq!(get_qris_tag(&parsed, "53").as_deref(), Some("360"));
        assert_eq!(get_qris_tag(&parsed, "54").as_deref(), Some("1500"));
        assert!(dynamic.contains("54041500"));
        assert!(is_valid_qris_checksum(&dynamic));
        assert_eq!(
            get_qris_tag(&parsed, "26").as_deref(),
            Some("0014ID.CO.QRIS.WWW011693600123456789010215ID1234567890123")
        );
        assert!(build_dynamic_qris(&payload, 0).is_err());
        assert!(build_dynamic_qris(&payload, -5).is_err());
        let tampered = payload.replace("TOKO TEST QRIS", "TOKO TEST QRIT");
        assert!(build_dynamic_qris(&tampered, 1500).is_err());
        let big = build_dynamic_qris(&payload, 10_000_000).unwrap();
        assert_eq!(
            get_qris_tag(&parse_qris_payload(&big).unwrap(), "54").as_deref(),
            Some("10000000")
        );
    }

    #[test]
    fn qr_png_magic() {
        let png = render_qris_png(
            &build_dynamic_qris(&static_payload(), 1501).unwrap(),
            512,
        )
        .unwrap();
        assert!(png.len() > 500);
        assert_eq!(&png[0..4], &[0x89, 0x50, 0x4e, 0x47]);
    }

    #[test]
    fn allocator() {
        assert_eq!(
            allocate_unique_amount(1500, Vec::<i64>::new(), QRIS_MAX_UNIQUE_OFFSET).unwrap(),
            (1501, 1)
        );
        assert_eq!(
            allocate_unique_amount(1500, vec![1501], QRIS_MAX_UNIQUE_OFFSET)
                .unwrap()
                .0,
            1502
        );
        let mut taken = HashSet::new();
        let mut allocated = Vec::new();
        for _ in 0..5 {
            let (amount, _) = allocate_unique_amount(
                1500,
                taken.iter().copied(),
                QRIS_MAX_UNIQUE_OFFSET,
            )
            .unwrap();
            taken.insert(amount);
            allocated.push(amount);
        }
        assert_eq!(allocated, vec![1501, 1502, 1503, 1504, 1505]);
        // Keunikan di HASIL, bukan offset (3499+2 == 3500+1).
        assert_eq!(
            allocate_unique_amount(3499, vec![3501], QRIS_MAX_UNIQUE_OFFSET)
                .unwrap()
                .0,
            3500
        );
        assert!(allocate_unique_amount(1500, vec![1501, 1502], 2).is_err());
    }

    #[test]
    fn store_crud_and_expiry() {
        let db = test_db();
        let payload = static_payload();
        let now = Utc::now();
        let p1 = create_qris_payment(&db, "TEST-ORDER-1", 1500, &payload, Some(now)).unwrap();
        assert_eq!(p1.payment.unique_amount_idr, 1501);
        assert!(p1.qr_string.contains("54041501"));
        assert_eq!(p1.payment.status, QrisPaymentStatus::Pending);
        assert_eq!(
            parse_iso(&p1.payment.expires_at).unwrap().timestamp_millis()
                - parse_iso(&p1.payment.created_at).unwrap().timestamp_millis(),
            QRIS_PAYMENT_TTL_MS
        );
        let p2 = create_qris_payment(&db, "TEST-ORDER-2", 1500, &payload, Some(now)).unwrap();
        assert_eq!(p2.payment.unique_amount_idr, 1502);
        assert_eq!(
            get_qris_payment(&db, &p1.payment.id).unwrap().unwrap().reference,
            "TEST-ORDER-1"
        );
        assert_eq!(list_pending_qris_payments(&db, Some(now)).unwrap().len(), 2);
        assert!(mark_qris_payment_cancelled(&db, &p1.payment.id).unwrap());
        assert!(!mark_qris_payment_cancelled(&db, &p1.payment.id).unwrap());
        assert_eq!(list_pending_qris_payments(&db, Some(now)).unwrap().len(), 1);
        let p3 = create_qris_payment(&db, "TEST-ORDER-3", 1500, &payload, Some(now)).unwrap();
        assert_eq!(p3.payment.unique_amount_idr, 1501, "1501 bebas lagi setelah cancel");
        // Pembayaran 6 menit lalu (sudah lewat TTL 5 menit).
        let past = now - Duration::minutes(6);
        let p_old =
            create_qris_payment(&db, "TEST-ORDER-OLD", 1500, &payload, Some(past)).unwrap();
        assert_eq!(p_old.payment.unique_amount_idr, 1503);
        assert_eq!(expire_qris_payments(&db, Some(now)).unwrap(), 1);
        assert_eq!(
            get_qris_payment(&db, &p_old.payment.id).unwrap().unwrap().status,
            QrisPaymentStatus::Expired
        );
        let p4 = create_qris_payment(&db, "TEST-ORDER-4", 1500, &payload, Some(now)).unwrap();
        assert_eq!(p4.payment.unique_amount_idr, 1503, "1503 bebas lagi setelah expiry");
        assert!(mark_qris_payment_paid(&db, &p2.payment.id).unwrap());
        assert_eq!(list_pending_qris_payments(&db, Some(now)).unwrap().len(), 2);
    }

    #[test]
    fn matcher_and_reconcile() {
        let db = test_db();
        let payload = static_payload();
        let now = Utc::now();
        let p3 = create_qris_payment(&db, "TEST-ORDER-3", 1500, &payload, Some(now)).unwrap();
        let _p2 = create_qris_payment(&db, "TEST-ORDER-2", 1500, &payload, Some(now)).unwrap();
        let p4 = create_qris_payment(&db, "TEST-ORDER-4", 1500, &payload, Some(now)).unwrap();
        let pending = list_pending_qris_payments(&db, Some(now)).unwrap();
        let tx_ok = FeedTransaction {
            id: "tx1".into(),
            amount_idr: 1501,
            time: Some(Utc::now()),
            status: Some("settlement".into()),
        };
        assert_eq!(
            find_qris_payment_for_transaction(&tx_ok, &pending, None)
                .unwrap()
                .id,
            p3.payment.id
        );
        let tx_none = FeedTransaction {
            id: "tx2b".into(),
            amount_idr: 1599,
            time: Some(Utc::now()),
            status: None,
        };
        assert!(find_qris_payment_for_transaction(&tx_none, &pending, None).is_none());
        let tx_late = FeedTransaction {
            id: "tx3".into(),
            amount_idr: 1503,
            time: Some(Utc::now() + Duration::minutes(10)),
            status: None,
        };
        assert!(find_qris_payment_for_transaction(&tx_late, &pending, None).is_none());
        let tx_early = FeedTransaction {
            id: "tx4".into(),
            amount_idr: 1501,
            time: Some(Utc::now() - Duration::minutes(10)),
            status: None,
        };
        assert!(find_qris_payment_for_transaction(&tx_early, &pending, None).is_none());
        let tx_failed = FeedTransaction {
            id: "tx5".into(),
            amount_idr: 1503,
            time: Some(Utc::now()),
            status: Some("failed".into()),
        };
        assert!(find_qris_payment_for_transaction(&tx_failed, &pending, None).is_none());
        let tx_no_status = FeedTransaction {
            id: "tx6".into(),
            amount_idr: 1503,
            time: Some(Utc::now()),
            status: None,
        };
        assert_eq!(
            find_qris_payment_for_transaction(&tx_no_status, &pending, None)
                .unwrap()
                .id,
            p4.payment.id,
            "status kosong fail-open"
        );
        let tx_no_time = FeedTransaction {
            id: "tx7".into(),
            amount_idr: 1501,
            time: None,
            status: None,
        };
        assert_eq!(
            find_qris_payment_for_transaction(&tx_no_time, &pending, None)
                .unwrap()
                .id,
            p3.payment.id
        );
        assert!(find_qris_payment_for_transaction(&tx_ok, &[], None).is_none());
        let tx_list = [
            tx_ok,
            FeedTransaction {
                id: "tx8".into(),
                amount_idr: 1503,
                time: Some(Utc::now()),
                status: Some("paid".into()),
            },
            FeedTransaction {
                id: "tx9".into(),
                amount_idr: 1501,
                time: Some(Utc::now()),
                status: Some("paid".into()),
            },
        ];
        let reconciled = reconcile_feed_transactions(&pending, &tx_list, None);
        assert_eq!(reconciled.len(), 2, "tx duplikat tidak dipakai 2x");
        let used_ids: HashSet<&str> =
            reconciled.iter().map(|(_, tx)| tx.id.as_str()).collect();
        assert_eq!(used_ids.len(), 2);
    }

    fn full_db() -> Db {
        // Skema PENUH (tes baru F) — bukan test_db() parsial di atas.
        crate::db::open(":memory:").expect("db tes")
    }

    #[test]
    fn dynamic_builder_rejects_bad_static_payload() {
        assert!(build_dynamic_qris("bukan-qris", 2000).is_err());
        assert!(build_dynamic_qris("", 2000).is_err());
        // Nominal tidak valid.
        let payload = static_payload();
        assert!(build_dynamic_qris(&payload, 0).is_err());
        assert!(build_dynamic_qris(&payload, -100).is_err());
    }

    #[test]
    fn checksum_validation() {
        let payload = static_payload();
        assert!(is_valid_qris_checksum(&payload));
        // Ubah 1 karakter sebelum CRC -> checksum tidak valid.
        let mut bad = payload.clone();
        bad.replace_range(10..11, "9");
        assert!(!is_valid_qris_checksum(&bad));
        assert!(!is_valid_qris_checksum("000201010211"));
        assert!(!is_valid_qris_checksum(""));
    }

    #[test]
    fn allocator_exhaustion_errors() {
        // Semua offset 1..=3 terpakai -> error jelas.
        assert!(allocate_unique_amount(2000, [2001, 2002, 2003], 3).is_err());
        assert!(allocate_unique_amount(2000, [2001, 2002], 3).is_ok());
        assert!(allocate_unique_amount(0, [], 999).is_err());
        assert!(allocate_unique_amount(2000, [], 0).is_err());
    }

    #[test]
    fn concurrent_qris_allocation_has_no_collision() {
        // 8 alokasi bersamaan, nominal dasar sama: semua unique_amount
        // harus BERBEDA (BEGIN IMMEDIATE di create_qris_payment).
        let db = full_db();
        let payload = static_payload();
        let mut handles = Vec::new();
        for i in 0..8 {
            let db2 = db.clone();
            let payload2 = payload.clone();
            handles.push(std::thread::spawn(move || {
                create_qris_payment(&db2, &format!("UD-CONC-{i}"), 2000, &payload2, None)
            }));
        }
        let mut uniques = std::collections::HashSet::new();
        for h in handles {
            let created = h.join().unwrap().unwrap();
            assert!(uniques.insert(created.payment.unique_amount_idr));
            assert_eq!(created.payment.amount_idr, 2000);
            assert_eq!(created.payment.status, QrisPaymentStatus::Pending);
        }
        assert_eq!(uniques.len(), 8);
        // Semua nominal unik berada di 2001..=2008.
        assert!(uniques.iter().all(|u| (2001..=2008).contains(u)));
    }

    #[test]
    fn match_feed_rejects_failed_status_and_wrong_time() {
        let db = full_db();
        let created = create_qris_payment(&db, "UD-MATCH", 2000, &static_payload(), None).unwrap();
        let payment = created.payment.clone();
        // Status gagal di feed -> tidak cocok.
        let failed = FeedTransaction {
            id: "tx-fail".into(),
            amount_idr: payment.unique_amount_idr,
            time: None,
            status: Some("failed".into()),
        };
        assert!(!match_feed_transaction(&failed, &payment, None));
        // Nominal beda 1 rupiah -> tidak cocok.
        let wrong_amount = FeedTransaction {
            id: "tx-wrong".into(),
            amount_idr: payment.unique_amount_idr + 1,
            time: None,
            status: Some("settlement".into()),
        };
        assert!(!match_feed_transaction(&wrong_amount, &payment, None));
        // Waktu di luar jendela (kemarin) -> tidak cocok.
        let old_time = FeedTransaction {
            id: "tx-old".into(),
            amount_idr: payment.unique_amount_idr,
            time: Some(Utc::now() - chrono::Duration::days(1)),
            status: Some("settlement".into()),
        };
        assert!(!match_feed_transaction(&old_time, &payment, None));
        // Cocok: nominal sama + settlement + tanpa waktu.
        let good = FeedTransaction {
            id: "tx-good".into(),
            amount_idr: payment.unique_amount_idr,
            time: None,
            status: Some("settlement".into()),
        };
        assert!(match_feed_transaction(&good, &payment, None));
    }
}
