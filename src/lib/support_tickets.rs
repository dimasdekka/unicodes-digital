//! Tiket support — port `src/lib/support-tickets.ts`.
//!
//! Pengganti forward pesan langsung ke Telegram admin (forward langsung
//! DIMATIKAN permanen 2026-11-21). Alur:
//!   - User buka tiket dari bot (tombol 💬 Support -> kirim satu pesan).
//!   - Pesan user tersimpan di support_messages; admin menjawab dari web
//!     admin (/admin/tickets) dan balasan diteruskan ke user via Bot API.
//!   - Satu user bisa punya banyak tiket, tapi hanya SATU yang terbuka
//!     (open/pending) — pesan baru masuk ke tiket terbuka itu; kalau tiket
//!     sudah ditutup, pesan baru membuka tiket baru.
//!
//! Tabel dibuat di `crate::db::ensure_tables` (dulu lazy via
//! `ensureSupportTables()` di TS). Balasan admin dikirim lewat Bot API
//! langsung (web admin tidak punya instance bot) — pakai
//! `crate::lib::notify::send_telegram_html`.

use crate::db::{now_iso, Db};
use crate::lib::notify::send_telegram_html;
use rand::Rng;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TicketStatus {
    Open,
    Pending,
    Closed,
}

impl TicketStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            TicketStatus::Open => "open",
            TicketStatus::Pending => "pending",
            TicketStatus::Closed => "closed",
        }
    }

    pub fn from_str(s: &str) -> TicketStatus {
        match s {
            "pending" => TicketStatus::Pending,
            "closed" => TicketStatus::Closed,
            _ => TicketStatus::Open,
        }
    }
}

#[derive(Debug, Clone)]
pub struct SupportTicket {
    pub id: String,
    pub tg_id: i64,
    pub username: Option<String>,
    pub first_name: Option<String>,
    pub status: TicketStatus,
    pub subject: String,
    /// Order yang dipilih user saat buka tiket (boleh kosong — tiket
    /// "Tanpa order" atau tiket lama dari sebelum fitur pilih order).
    pub order_id: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    pub last_message_at: String,
}

#[derive(Debug, Clone)]
pub struct SupportMessage {
    pub id: i64,
    pub ticket_id: String,
    pub sender: String, // "user" | "admin"
    pub text: String,
    pub created_at: String,
}

const TICKET_ALPHABET: &[u8] = b"ABCDEFGHJKLMNPQRSTUVWXYZ23456789"; // tanpa 0/O/1/I

fn new_ticket_id() -> String {
    let mut rng = rand::thread_rng();
    let suffix: String = (0..6)
        .map(|_| TICKET_ALPHABET[rng.gen_range(0..TICKET_ALPHABET.len())] as char)
        .collect();
    format!("TKT-{suffix}")
}

fn row_to_ticket(row: &rusqlite::Row) -> rusqlite::Result<SupportTicket> {
    Ok(SupportTicket {
        id: row.get(0)?,
        tg_id: row.get(1)?,
        username: row.get(2)?,
        first_name: row.get(3)?,
        status: TicketStatus::from_str(&row.get::<_, String>(4)?),
        subject: row.get(5)?,
        order_id: row.get(6)?,
        created_at: row.get(7)?,
        updated_at: row.get(8)?,
        last_message_at: row.get(9)?,
    })
}

fn row_to_message(row: &rusqlite::Row) -> rusqlite::Result<SupportMessage> {
    Ok(SupportMessage {
        id: row.get(0)?,
        ticket_id: row.get(1)?,
        sender: row.get(2)?,
        text: row.get(3)?,
        created_at: row.get(4)?,
    })
}

/// Tiket terbuka (open/pending) terbaru milik user, kalau ada.
pub fn get_open_ticket_by_user(db: &Db, tg_id: i64) -> Option<SupportTicket> {
    let conn = db.lock().ok()?;
    conn.query_row(
        "SELECT id, tg_id, username, first_name, status, subject, order_id, created_at, updated_at, last_message_at \
         FROM support_tickets
         WHERE tg_id = ?1 AND status IN ('open', 'pending')
         ORDER BY last_message_at DESC LIMIT 1",
        rusqlite::params![tg_id],
        row_to_ticket,
    )
    .ok()
}

/// Buat tiket baru dari pesan pertama user.
/// `order_id` = order yang dipilih user di langkah pilih order
/// (None = "Tanpa order" / tiket lama).
pub fn create_ticket(
    db: &Db,
    tg_id: i64,
    username: Option<&str>,
    first_name: Option<&str>,
    text: &str,
    order_id: Option<&str>,
) -> anyhow::Result<SupportTicket> {
    let now = now_iso();
    let subject: String = text.trim().chars().take(60).collect();
    let text_trimmed = text.trim().to_string();
    // ID acak 6 karakter — tabrakan praktis tidak mungkin, tapi coba lagi
    // kalau kebetulan bentrok dengan tiket yang sudah ada.
    for _ in 0..5 {
        let id = new_ticket_id();
        let inserted = {
            let conn = match db.lock() {
                Ok(g) => g,
                Err(e) => return Err(anyhow::anyhow!("db lock poisoned: {e}")),
            };
            let r = conn.execute(
                "INSERT INTO support_tickets
                 (id, tg_id, username, first_name, status, subject, order_id, created_at, updated_at, last_message_at)
                 VALUES (?1, ?2, ?3, ?4, 'open', ?5, ?6, ?7, ?7, ?7)",
                rusqlite::params![id, tg_id, username, first_name, subject, order_id, now],
            );
            match r {
                Ok(_) => {
                    let _ = conn.execute(
                        "INSERT INTO support_messages (ticket_id, sender, text, created_at)
                         VALUES (?1, 'user', ?2, ?3)",
                        rusqlite::params![id, text_trimmed, now],
                    );
                    true
                }
                Err(_) => false,
            }
        };
        if inserted {
            return Ok(SupportTicket {
                id,
                tg_id,
                username: username.map(|s| s.to_string()),
                first_name: first_name.map(|s| s.to_string()),
                status: TicketStatus::Open,
                subject,
                order_id: order_id.map(|s| s.to_string()),
                created_at: now.clone(),
                updated_at: now.clone(),
                last_message_at: now,
            });
        }
    }
    Err(anyhow::anyhow!("Gagal membuat tiket support (ID bentrok berulang)"))
}

/// Tambah pesan user ke tiket terbuka; status kembali 'open' (butuh admin).
pub fn add_user_message(db: &Db, ticket_id: &str, text: &str) -> anyhow::Result<()> {
    let now = now_iso();
    let conn = db.lock().map_err(|e| anyhow::anyhow!("db lock poisoned: {e}"))?;
    conn.execute(
        "INSERT INTO support_messages (ticket_id, sender, text, created_at)
         VALUES (?1, 'user', ?2, ?3)",
        rusqlite::params![ticket_id, text.trim(), now],
    )?;
    conn.execute(
        "UPDATE support_tickets
         SET status = 'open', updated_at = ?1, last_message_at = ?1
         WHERE id = ?2",
        rusqlite::params![now, ticket_id],
    )?;
    Ok(())
}

/// Daftar tiket — terbuka dulu, lalu terbaru.
pub fn list_tickets(db: &Db, limit: i64) -> Vec<SupportTicket> {
    let conn = match db.lock() {
        Ok(g) => g,
        Err(_) => return Vec::new(),
    };
    let mut stmt = match conn.prepare(
        "SELECT id, tg_id, username, first_name, status, subject, order_id, created_at, updated_at, last_message_at \
         FROM support_tickets
         ORDER BY CASE status WHEN 'open' THEN 0 WHEN 'pending' THEN 1 ELSE 2 END,
           last_message_at DESC LIMIT ?1",
    ) {
        Ok(s) => s,
        Err(_) => return Vec::new(),
    };
    let rows = match stmt.query_map(rusqlite::params![limit], row_to_ticket) {
        Ok(m) => m.filter_map(|r| r.ok()).collect(),
        Err(_) => Vec::new(),
    };
    rows
}

/// Tiket milik satu user (untuk layar "Tiket Saya" di bot).
pub fn list_tickets_by_user(db: &Db, tg_id: i64, limit: i64) -> Vec<SupportTicket> {
    let conn = match db.lock() {
        Ok(g) => g,
        Err(_) => return Vec::new(),
    };
    let mut stmt = match conn.prepare(
        "SELECT id, tg_id, username, first_name, status, subject, order_id, created_at, updated_at, last_message_at \
         FROM support_tickets WHERE tg_id = ?1
         ORDER BY last_message_at DESC LIMIT ?2",
    ) {
        Ok(s) => s,
        Err(_) => return Vec::new(),
    };
    let rows = match stmt.query_map(rusqlite::params![tg_id, limit], row_to_ticket) {
        Ok(m) => m.filter_map(|r| r.ok()).collect(),
        Err(_) => Vec::new(),
    };
    rows
}

pub fn get_ticket(db: &Db, id: &str) -> Option<SupportTicket> {
    let conn = db.lock().ok()?;
    conn.query_row(
        "SELECT id, tg_id, username, first_name, status, subject, order_id, created_at, updated_at, last_message_at \
         FROM support_tickets WHERE id = ?1",
        rusqlite::params![id],
        row_to_ticket,
    )
    .ok()
}

pub fn get_ticket_messages(db: &Db, ticket_id: &str) -> Vec<SupportMessage> {
    let conn = match db.lock() {
        Ok(g) => g,
        Err(_) => return Vec::new(),
    };
    let mut stmt = match conn.prepare(
        "SELECT id, ticket_id, sender, text, created_at FROM support_messages \
         WHERE ticket_id = ?1 ORDER BY id ASC",
    ) {
        Ok(s) => s,
        Err(_) => return Vec::new(),
    };
    let rows = match stmt.query_map(rusqlite::params![ticket_id], row_to_message) {
        Ok(m) => m.filter_map(|r| r.ok()).collect(),
        Err(_) => Vec::new(),
    };
    rows
}

/// Jumlah tiket yang masih butuh perhatian admin (open + pending).
pub fn count_open_tickets(db: &Db) -> i64 {
    let conn = match db.lock() {
        Ok(g) => g,
        Err(_) => return 0,
    };
    conn.query_row(
        "SELECT COUNT(*) FROM support_tickets WHERE status IN ('open', 'pending')",
        [],
        |row| row.get::<_, i64>(0),
    )
    .unwrap_or(0)
}

pub fn set_ticket_status(db: &Db, id: &str, status: TicketStatus) -> anyhow::Result<()> {
    let conn = db.lock().map_err(|e| anyhow::anyhow!("db lock poisoned: {e}"))?;
    conn.execute(
        "UPDATE support_tickets SET status = ?1, updated_at = ?2 WHERE id = ?3",
        rusqlite::params![status.as_str(), now_iso(), id],
    )?;
    Ok(())
}

pub fn escape_html(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

/// Balas tiket: kirim ke user via Telegram + simpan + status jadi 'pending'
/// (sudah dibalas admin, menunggu user). Pesan tetap disimpan walau kirim
/// ke Telegram gagal — admin bisa kirim ulang dari web.
pub async fn reply_to_ticket(
    db: &Db,
    bot_token: &str,
    ticket_id: &str,
    text: &str,
) -> Result<(), String> {
    let ticket = get_ticket(db, ticket_id).ok_or_else(|| "Tiket tidak ditemukan".to_string())?;
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return Err("Balasan kosong".to_string());
    }
    let sent = send_telegram_html(
        bot_token,
        ticket.tg_id,
        &format!(
            "💬 <b>Balasan admin (tiket {}):</b>\n\n{}",
            ticket.id,
            escape_html(trimmed)
        ),
    )
    .await;
    let now = now_iso();
    {
        let conn = db.lock().map_err(|e| format!("db lock poisoned: {e}"))?;
        conn.execute(
            "INSERT INTO support_messages (ticket_id, sender, text, created_at)
             VALUES (?1, 'admin', ?2, ?3)",
            rusqlite::params![ticket_id, trimmed, now],
        )
        .map_err(|e| e.to_string())?;
        conn.execute(
            "UPDATE support_tickets
             SET status = 'pending', updated_at = ?1, last_message_at = ?1
             WHERE id = ?2",
            rusqlite::params![now, ticket_id],
        )
        .map_err(|e| e.to_string())?;
    }
    sent.map(|_| ())
}

/// Tutup tiket + kabari user di Telegram.
pub async fn close_ticket(db: &Db, bot_token: &str, ticket_id: &str) -> Result<(), String> {
    let ticket = get_ticket(db, ticket_id).ok_or_else(|| "Tiket tidak ditemukan".to_string())?;
    set_ticket_status(db, ticket_id, TicketStatus::Closed).map_err(|e| e.to_string())?;
    send_telegram_html(
        bot_token,
        ticket.tg_id,
        &format!(
            "🔒 Tiket <b>{}</b> ditutup oleh admin. Kalau masalahmu belum selesai, buka tiket baru lewat tombol 💬 Support.",
            ticket.id
        ),
    )
    .await
    .map(|_| ())
}

/// Buka lagi tiket yang sudah ditutup.
pub fn reopen_ticket(db: &Db, ticket_id: &str) -> Result<(), String> {
    if get_ticket(db, ticket_id).is_none() {
        return Err("Tiket tidak ditemukan".to_string());
    }
    set_ticket_status(db, ticket_id, TicketStatus::Open).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db;

    fn test_db() -> Db {
        db::open(":memory:").expect("db tes")
    }

    #[test]
    fn create_ticket_format_and_first_message() {
        // S10: konfirmasi berisi nomor tiket TKT-XXXXXX.
        let db = test_db();
        let t = create_ticket(&db, 111, Some("budi"), Some("Budi"), "Order saya belum masuk, tolong dicek", None).unwrap();
        assert!(t.id.starts_with("TKT-"));
        assert_eq!(t.id.len(), 10);
        assert!(t.id[4..].chars().all(|c| c.is_ascii_alphanumeric()));
        assert_eq!(t.status, TicketStatus::Open);
        assert_eq!(t.tg_id, 111);
        let msgs = get_ticket_messages(&db, &t.id);
        assert_eq!(msgs.len(), 1);
        assert_eq!(msgs[0].sender, "user");
        assert!(msgs[0].text.contains("belum masuk"));
        assert_eq!(get_open_ticket_by_user(&db, 111).unwrap().id, t.id);
        assert_eq!(count_open_tickets(&db), 1);
    }

    #[test]
    fn second_message_joins_same_ticket() {
        // S10: pesan kedua masuk ke tiket yang sama (bukan tiket baru).
        let db = test_db();
        let t = create_ticket(&db, 111, None, Some("Budi"), "Pesan pertama", None).unwrap();
        add_user_message(&db, &t.id, "Sudah 1 jam belum masuk juga").unwrap();
        assert_eq!(get_open_ticket_by_user(&db, 111).unwrap().id, t.id);
        assert_eq!(get_ticket_messages(&db, &t.id).len(), 2);
        assert_eq!(count_open_tickets(&db), 1);
        assert_eq!(list_tickets_by_user(&db, 111, 10).len(), 1);
    }

    #[test]
    fn close_then_new_message_opens_new_ticket() {
        // S10: tutup tiket -> pesan baru membuka tiket BARU.
        let db = test_db();
        let t1 = create_ticket(&db, 111, None, None, "Masalah lama", None).unwrap();
        set_ticket_status(&db, &t1.id, TicketStatus::Closed).unwrap();
        assert!(get_open_ticket_by_user(&db, 111).is_none());
        assert_eq!(count_open_tickets(&db), 0);
        let t2 = create_ticket(&db, 111, None, None, "Masih ada masalah lain", None).unwrap();
        assert_ne!(t2.id, t1.id);
        assert_eq!(get_open_ticket_by_user(&db, 111).unwrap().id, t2.id);
        assert_eq!(list_tickets_by_user(&db, 111, 10).len(), 2);
    }

    #[test]
    fn reopen_ticket_and_status_flow() {
        let db = test_db();
        let t = create_ticket(&db, 111, None, None, "Halo", None).unwrap();
        set_ticket_status(&db, &t.id, TicketStatus::Pending).unwrap();
        assert_eq!(get_ticket(&db, &t.id).unwrap().status, TicketStatus::Pending);
        // Pesan user ke tiket pending -> status kembali open.
        add_user_message(&db, &t.id, "Balasan user").unwrap();
        assert_eq!(get_ticket(&db, &t.id).unwrap().status, TicketStatus::Open);
        set_ticket_status(&db, &t.id, TicketStatus::Closed).unwrap();
        reopen_ticket(&db, &t.id).unwrap();
        assert_eq!(get_ticket(&db, &t.id).unwrap().status, TicketStatus::Open);
        assert!(reopen_ticket(&db, "TKT-TIDAKADA").is_err());
        assert!(get_ticket(&db, "TKT-TIDAKADA").is_none());
    }

    #[test]
    fn list_tickets_open_first() {
        let db = test_db();
        let a = create_ticket(&db, 111, None, None, "A", None).unwrap();
        let b = create_ticket(&db, 222, None, None, "B", None).unwrap();
        set_ticket_status(&db, &a.id, TicketStatus::Closed).unwrap();
        let list = list_tickets(&db, 10);
        assert_eq!(list.len(), 2);
        // Terbuka dulu.
        assert_eq!(list[0].id, b.id);
        assert_eq!(list[1].id, a.id);
    }

    #[test]
    fn subject_is_first_60_chars() {
        let db = test_db();
        let long = "x".repeat(100);
        let t = create_ticket(&db, 111, None, None, &long, None).unwrap();
        assert_eq!(t.subject.chars().count(), 60);
        let t2 = create_ticket(&db, 222, None, None, "  pendek  ", None).unwrap();
        assert_eq!(t2.subject, "pendek");
    }

    #[test]
    fn escape_html_encodes() {
        assert_eq!(
            escape_html("<b>halo</b> & \"x\""),
            "&lt;b&gt;halo&lt;/b&gt; &amp; \"x\""
        );
    }

    #[test]
    fn ticket_status_round_trip() {
        assert_eq!(TicketStatus::Open.as_str(), "open");
        assert_eq!(TicketStatus::Pending.as_str(), "pending");
        assert_eq!(TicketStatus::Closed.as_str(), "closed");
        assert_eq!(TicketStatus::from_str("pending"), TicketStatus::Pending);
        assert_eq!(TicketStatus::from_str("closed"), TicketStatus::Closed);
        assert_eq!(TicketStatus::from_str("aneh"), TicketStatus::Open);
    }

    #[test]
    fn create_ticket_with_bound_order() {
        // Tiket baru dengan order terikat (dipilih di langkah pilih order).
        let db = test_db();
        let t = create_ticket(
            &db,
            111,
            Some("budi"),
            Some("Budi"),
            "Token order ini belum masuk",
            Some("UD-TEST123"),
        )
        .unwrap();
        assert_eq!(t.order_id.as_deref(), Some("UD-TEST123"));
        let again = get_ticket(&db, &t.id).unwrap();
        assert_eq!(again.order_id.as_deref(), Some("UD-TEST123"));
        assert_eq!(list_tickets(&db, 10)[0].order_id.as_deref(), Some("UD-TEST123"));
    }

    #[test]
    fn create_ticket_without_order_has_none() {
        // "Tanpa order" -> order_id kosong.
        let db = test_db();
        let t = create_ticket(&db, 111, None, None, "Tanya soal promo", None).unwrap();
        assert_eq!(t.order_id, None);
        assert_eq!(get_ticket(&db, &t.id).unwrap().order_id, None);
    }

    #[test]
    fn follow_up_keeps_bound_order() {
        // Tiket terbuka yang sudah terikat order: pesan lanjutan masuk ke
        // tiket yang sama dan order_id TIDAK berubah / tidak hilang.
        let db = test_db();
        let t = create_ticket(&db, 111, None, None, "Order belum masuk", Some("UD-TEST1")).unwrap();
        add_user_message(&db, &t.id, "Sudah 2 jam belum masuk juga").unwrap();
        let again = get_ticket(&db, &t.id).unwrap();
        assert_eq!(again.order_id.as_deref(), Some("UD-TEST1"));
        assert_eq!(get_open_ticket_by_user(&db, 111).unwrap().id, t.id);
        assert_eq!(count_open_tickets(&db), 1);
    }
}
