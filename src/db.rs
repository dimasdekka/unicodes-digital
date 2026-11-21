//! SQLite: skema IDENTIK dengan versi TypeScript (`src/lib/db.ts` +
//! `ensureSupportTables`, `product-prices.ts`, dst) supaya `dev.db` yang
//! sudah ada bisa dipakai langsung tanpa migrasi.
//!
//! Satu writer (SQLite): koneksi dibungkus `Mutex`, WAL mode,
//! `busy_timeout = 5000` (perbaikan konkurensi commit `a5ed97a`).
//!
//! JANGAN PERNAH membuat tabel dengan skema parsial untuk "cek" —
//! untuk memeriksa skema pakai `PRAGMA table_info(...)` read-only,
//! dan hanya pada DB tes sementara, JANGAN PERNAH sentuh `dev.db` asli.

use rusqlite::Connection;
use std::path::Path;
use std::sync::{Arc, Mutex};

pub type Db = Arc<Mutex<Connection>>;

/// Timestamp ISO-8601 UTC dengan milidetik, sama seperti
/// `new Date().toISOString()` di TS (mis. `2026-11-21T09:00:00.000Z`).
pub fn now_iso() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}

pub fn open(path: &str) -> anyhow::Result<Db> {
    let conn = Connection::open(Path::new(path))?;
    conn.execute_batch(
        "PRAGMA journal_mode = WAL;
         PRAGMA busy_timeout = 5000;
         -- foreign_keys = ON adalah hardening SADAR, bukan paritas TS
         -- (TS tidak pernah mengaturnya): satu-satunya FK di skema adalah
         -- wallets.tg_id REFERENCES users(tg_id), dan satu-satunya INSERT
         -- ke wallets (wallet.rs ensure_user) selalu didahului upsert ke
         -- users dalam fungsi yang sama — jadi FK tidak pernah memblokir
         -- jalur normal, hanya menangkap bug bila ada kode baru yang
         -- menulis wallet tanpa user. FK hanya ditegakkan saat tulis,
         -- jadi dev.db lama yang sudah ada tidak terpengaruh.
         PRAGMA foreign_keys = ON;",
    )?;
    ensure_tables(&conn)?;
    Ok(Arc::new(Mutex::new(conn)))
}

/// Buat semua tabel kalau belum ada — SQL disalin dari versi TS:
/// `src/lib/db.ts` (openDb), `src/lib/support-tickets.ts`
/// (ensureSupportTables), `src/lib/product-prices.ts`
/// (ensureProductPriceTables). Termasuk migrasi ringan TS: kolom
/// `target`, `discount_idr`, `promo_code`, `paid_via` di `store_orders`
/// ditambahkan kalau belum ada (order lama tetap jalan).
pub fn ensure_tables(conn: &Connection) -> anyhow::Result<()> {
    conn.execute_batch(
        "
    CREATE TABLE IF NOT EXISTS store_orders (
      id TEXT PRIMARY KEY,
      catalog_product_id TEXT NOT NULL,
      quantity INTEGER NOT NULL,
      retail_total_idr INTEGER NOT NULL,
      status TEXT NOT NULL,
      supplier TEXT NOT NULL,
      supplier_order_id TEXT,
      delivered_items TEXT NOT NULL DEFAULT '[]',
      result_link TEXT,
      error TEXT,
      buyer_ref TEXT,
      xiaomi_accounts TEXT,
      sheerid_url TEXT,
      created_at TEXT NOT NULL,
      updated_at TEXT NOT NULL
    );
    CREATE INDEX IF NOT EXISTS idx_store_orders_status ON store_orders(status);
    CREATE INDEX IF NOT EXISTS idx_store_orders_created ON store_orders(created_at);
    CREATE TABLE IF NOT EXISTS webhook_deliveries (
      delivery_id TEXT PRIMARY KEY,
      source TEXT NOT NULL,
      received_at TEXT NOT NULL
    );
    CREATE TABLE IF NOT EXISTS users (
      tg_id INTEGER PRIMARY KEY,
      first_name TEXT,
      username TEXT,
      lang TEXT NOT NULL DEFAULT 'id',
      referrer_tg_id INTEGER,
      created_at TEXT NOT NULL,
      updated_at TEXT NOT NULL
    );
    CREATE TABLE IF NOT EXISTS wallets (
      tg_id INTEGER PRIMARY KEY,
      balance_idr INTEGER NOT NULL DEFAULT 0,
      total_topup_idr INTEGER NOT NULL DEFAULT 0,
      total_spent_idr INTEGER NOT NULL DEFAULT 0,
      updated_at TEXT NOT NULL,
      FOREIGN KEY (tg_id) REFERENCES users(tg_id)
    );
    CREATE TABLE IF NOT EXISTS wallet_txns (
      id INTEGER PRIMARY KEY AUTOINCREMENT,
      tg_id INTEGER NOT NULL,
      kind TEXT NOT NULL,
      amount_idr INTEGER NOT NULL,
      balance_after_idr INTEGER NOT NULL,
      ref TEXT,
      note TEXT,
      created_at TEXT NOT NULL
    );
    CREATE INDEX IF NOT EXISTS idx_wallet_txns_tg ON wallet_txns(tg_id, created_at);
    CREATE TABLE IF NOT EXISTS wallet_topups (
      id TEXT PRIMARY KEY,
      tg_id INTEGER NOT NULL,
      amount_idr INTEGER NOT NULL,
      status TEXT NOT NULL DEFAULT 'awaiting_payment',
      created_at TEXT NOT NULL,
      paid_at TEXT
    );
    CREATE TABLE IF NOT EXISTS promo_codes (
      code TEXT PRIMARY KEY,
      kind TEXT NOT NULL,
      value INTEGER NOT NULL,
      max_uses INTEGER,
      uses_count INTEGER NOT NULL DEFAULT 0,
      valid_until TEXT,
      created_by TEXT,
      created_at TEXT NOT NULL
    );
    CREATE TABLE IF NOT EXISTS promo_redemptions (
      id INTEGER PRIMARY KEY AUTOINCREMENT,
      code TEXT NOT NULL,
      tg_id INTEGER NOT NULL,
      order_id TEXT,
      discount_idr INTEGER NOT NULL,
      created_at TEXT NOT NULL,
      UNIQUE(code, tg_id)
    );
    CREATE TABLE IF NOT EXISTS referrals (
      id INTEGER PRIMARY KEY AUTOINCREMENT,
      referrer_tg_id INTEGER NOT NULL,
      referred_tg_id INTEGER NOT NULL UNIQUE,
      rewarded INTEGER NOT NULL DEFAULT 0,
      rewarded_at TEXT,
      created_at TEXT NOT NULL
    );
    CREATE INDEX IF NOT EXISTS idx_referrals_referrer ON referrals(referrer_tg_id);
    CREATE TABLE IF NOT EXISTS qris_payments (
      id TEXT PRIMARY KEY,
      reference TEXT NOT NULL,
      amount_idr INTEGER NOT NULL,
      unique_amount_idr INTEGER NOT NULL,
      qr_string TEXT NOT NULL,
      status TEXT NOT NULL DEFAULT 'pending',
      created_at TEXT NOT NULL,
      expires_at TEXT NOT NULL
    );
    CREATE INDEX IF NOT EXISTS idx_qris_payments_status ON qris_payments(status, unique_amount_idr);
    CREATE INDEX IF NOT EXISTS idx_qris_payments_ref ON qris_payments(reference);
    CREATE TABLE IF NOT EXISTS settings (
      key TEXT PRIMARY KEY,
      value TEXT NOT NULL,
      updated_at TEXT NOT NULL
    );
    CREATE TABLE IF NOT EXISTS channel_posts (
      order_id TEXT PRIMARY KEY,
      posted_at TEXT NOT NULL
    );
    CREATE TABLE IF NOT EXISTS midtrans_payments (
      order_id TEXT PRIMARY KEY,
      amount_idr INTEGER NOT NULL,
      snap_token TEXT NOT NULL,
      redirect_url TEXT NOT NULL,
      status TEXT NOT NULL DEFAULT 'pending',
      created_at TEXT NOT NULL,
      expires_at TEXT NOT NULL
    );
    CREATE INDEX IF NOT EXISTS idx_midtrans_payments_status ON midtrans_payments(status, expires_at);
    CREATE TABLE IF NOT EXISTS support_tickets (
      id TEXT PRIMARY KEY,
      tg_id INTEGER NOT NULL,
      username TEXT,
      first_name TEXT,
      status TEXT NOT NULL DEFAULT 'open',
      subject TEXT NOT NULL DEFAULT '',
      order_id TEXT,
      created_at TEXT NOT NULL,
      updated_at TEXT NOT NULL,
      last_message_at TEXT NOT NULL
    );
    CREATE INDEX IF NOT EXISTS idx_support_tickets_status ON support_tickets(status);
    CREATE INDEX IF NOT EXISTS idx_support_tickets_user ON support_tickets(tg_id);
    CREATE TABLE IF NOT EXISTS support_messages (
      id INTEGER PRIMARY KEY AUTOINCREMENT,
      ticket_id TEXT NOT NULL,
      sender TEXT NOT NULL,
      text TEXT NOT NULL,
      created_at TEXT NOT NULL
    );
    CREATE INDEX IF NOT EXISTS idx_support_messages_ticket ON support_messages(ticket_id);
    CREATE TABLE IF NOT EXISTS product_price_overrides (
      product_id TEXT PRIMARY KEY,
      retail_price_idr INTEGER NOT NULL,
      updated_at TEXT NOT NULL
    );
    ",
    )?;

    // Migrasi ringan (persis TS): tambah kolom kalau belum ada.
    let cols: Vec<String> = {
        let mut stmt = conn.prepare("PRAGMA table_info(store_orders)")?;
        let rows = stmt.query_map([], |row| row.get::<_, String>(1))?;
        rows.collect::<Result<Vec<_>, _>>()?
    };
    for (col, ddl) in [
        ("target", "ALTER TABLE store_orders ADD COLUMN target TEXT"),
        (
            "discount_idr",
            "ALTER TABLE store_orders ADD COLUMN discount_idr INTEGER NOT NULL DEFAULT 0",
        ),
        (
            "promo_code",
            "ALTER TABLE store_orders ADD COLUMN promo_code TEXT",
        ),
        (
            "paid_via",
            "ALTER TABLE store_orders ADD COLUMN paid_via TEXT",
        ),
    ] {
        if !cols.iter().any(|c| c == col) {
            conn.execute_batch(ddl)?;
        }
    }

    // Migrasi tiket support (2026-11-21): tiket bisa terikat ke satu order
    // (dipilih user saat buka tiket). Pola sama — tambah kolom kalau belum ada.
    let ticket_cols: Vec<String> = {
        let mut stmt = conn.prepare("PRAGMA table_info(support_tickets)")?;
        let rows = stmt.query_map([], |row| row.get::<_, String>(1))?;
        rows.collect::<Result<Vec<_>, _>>()?
    };
    if !ticket_cols.iter().any(|c| c == "order_id") {
        conn.execute_batch("ALTER TABLE support_tickets ADD COLUMN order_id TEXT")?;
    }
    Ok(())
}

/// Catat satu pengiriman webhook. Mengembalikan true kalau ini pertama kalinya
/// ID itu diterima (false = duplikat/retry, jangan diproses lagi).
/// Dipakai dedupe webhook Sondev via header X-Sondev-Delivery (docs v1.2).
pub fn record_webhook_delivery(db: &Db, source: &str, delivery_id: &str) -> bool {
    if delivery_id.is_empty() {
        return true; // tanpa ID, tidak bisa dedupe — proses saja
    }
    let conn = match db.lock() {
        Ok(g) => g,
        Err(_) => return true,
    };
    match conn.execute(
        "INSERT OR IGNORE INTO webhook_deliveries (delivery_id, source, received_at) VALUES (?1, ?2, ?3)",
        rusqlite::params![delivery_id, source, now_iso()],
    ) {
        Ok(changed) => changed > 0,
        Err(_) => true,
    }
}

/// Baca satu setting; None kalau key belum pernah disimpan.
pub fn get_setting(db: &Db, key: &str) -> Option<String> {
    let conn = db.lock().ok()?;
    conn.query_row(
        "SELECT value FROM settings WHERE key = ?1",
        rusqlite::params![key],
        |row| row.get::<_, String>(0),
    )
    .ok()
}

/// Simpan/ubah satu setting (upsert).
pub fn set_setting(db: &Db, key: &str, value: &str) -> anyhow::Result<()> {
    let conn = db.lock().map_err(|e| anyhow::anyhow!("db lock poisoned: {e}"))?;
    conn.execute(
        "INSERT INTO settings (key, value, updated_at) VALUES (?1, ?2, ?3) \
         ON CONFLICT(key) DO UPDATE SET value = excluded.value, updated_at = excluded.updated_at",
        rusqlite::params![key, value, now_iso()],
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_db() -> Db {
        // DB temporer :memory: + skema PENUH via ensure_tables (bukan skema
        // parsial) — sama seperti open() di produksi.
        open(":memory:").expect("db tes")
    }

    fn table_names(db: &Db) -> Vec<String> {
        let conn = db.lock().unwrap();
        let mut stmt = conn
            .prepare("SELECT name FROM sqlite_master WHERE type = 'table' ORDER BY name")
            .unwrap();
        stmt.query_map([], |r| r.get::<_, String>(0))
            .unwrap()
            .filter_map(|r| r.ok())
            .collect()
    }

    #[test]
    fn all_tables_exist() {
        let db = test_db();
        let names = table_names(&db);
        for t in [
            "store_orders",
            "webhook_deliveries",
            "users",
            "wallets",
            "wallet_txns",
            "wallet_topups",
            "promo_codes",
            "promo_redemptions",
            "referrals",
            "qris_payments",
            "settings",
            "channel_posts",
            "midtrans_payments",
            "support_tickets",
            "support_messages",
            "product_price_overrides",
        ] {
            assert!(names.iter().any(|n| n == t), "tabel {t} harus ada");
        }
    }

    #[test]
    fn store_orders_has_migration_columns() {
        // Kolom migrasi TS: target / discount_idr / promo_code / paid_via.
        let db = test_db();
        let conn = db.lock().unwrap();
        let mut stmt = conn.prepare("PRAGMA table_info(store_orders)").unwrap();
        let cols: Vec<String> = stmt
            .query_map([], |r| r.get::<_, String>(1))
            .unwrap()
            .filter_map(|r| r.ok())
            .collect();
        for c in ["target", "discount_idr", "promo_code", "paid_via"] {
            assert!(cols.iter().any(|x| x == c), "kolom {c} harus ada");
        }
    }

    #[test]
    fn support_tickets_order_id_migration_from_old_schema() {
        // Tiket dari skema lama (tanpa order_id) -> ensure_tables menambah
        // kolomnya, dan pemanggilan kedua tidak error (idempotent).
        let conn = rusqlite::Connection::open_in_memory().expect("memory db");
        conn.execute_batch(
            "CREATE TABLE support_tickets (
              id TEXT PRIMARY KEY,
              tg_id INTEGER NOT NULL,
              username TEXT,
              first_name TEXT,
              status TEXT NOT NULL DEFAULT 'open',
              subject TEXT NOT NULL DEFAULT '',
              created_at TEXT NOT NULL,
              updated_at TEXT NOT NULL,
              last_message_at TEXT NOT NULL
            );",
        )
        .unwrap();
        ensure_tables(&conn).unwrap();
        ensure_tables(&conn).unwrap();
        let mut stmt = conn.prepare("PRAGMA table_info(support_tickets)").unwrap();
        let cols: Vec<String> = stmt
            .query_map([], |r| r.get::<_, String>(1))
            .unwrap()
            .filter_map(|r| r.ok())
            .collect();
        assert!(
            cols.iter().any(|x| x == "order_id"),
            "kolom order_id harus ada setelah migrasi"
        );
    }

    #[test]
    fn ensure_tables_is_idempotent() {
        let db = test_db();
        // Dipanggil kedua kali (seperti open() ulang) — tidak boleh error
        // dan tidak boleh menghapus data. Guard dilepas dulu sebelum
        // table_names() me-lock lagi (Mutex tidak re-entrant).
        {
            let conn = db.lock().unwrap();
            ensure_tables(&conn).unwrap();
            ensure_tables(&conn).unwrap();
        }
        assert!(table_names(&db).contains(&"users".to_string()));
    }

    #[test]
    fn settings_round_trip() {
        let db = test_db();
        assert_eq!(get_setting(&db, "k"), None);
        set_setting(&db, "k", "v1").unwrap();
        assert_eq!(get_setting(&db, "k").as_deref(), Some("v1"));
        set_setting(&db, "k", "v2").unwrap();
        assert_eq!(get_setting(&db, "k").as_deref(), Some("v2"));
    }

    #[test]
    fn webhook_delivery_dedupe() {
        let db = test_db();
        assert!(record_webhook_delivery(&db, "sondev", "D-1"));
        assert!(!record_webhook_delivery(&db, "sondev", "D-1"));
        assert!(record_webhook_delivery(&db, "sondev", "D-2"));
        // Tanpa ID: tidak bisa dedupe — selalu proses.
        assert!(record_webhook_delivery(&db, "sondev", ""));
        assert!(record_webhook_delivery(&db, "sondev", ""));
    }

    #[test]
    fn foreign_keys_enforced_for_wallets() {
        // Bukti keputusan PERTAHANKAN foreign_keys = ON: insert wallet
        // tanpa baris users ditolak, setelah user dibuat diterima.
        let db = test_db();
        let conn = db.lock().unwrap();
        let err = conn
            .execute(
                "INSERT INTO wallets (tg_id, balance_idr, total_topup_idr, total_spent_idr, updated_at)
                 VALUES (999, 0, 0, 0, '2026-11-21T00:00:00.000Z')",
                [],
            )
            .unwrap_err();
        assert!(
            err.to_string().contains("FOREIGN KEY"),
            "harus gagal FK, dapat: {err}"
        );
        conn.execute(
            "INSERT INTO users (tg_id, first_name, username, lang, referrer_tg_id, created_at, updated_at)
             VALUES (999, NULL, NULL, 'id', NULL, '2026-11-21T00:00:00.000Z', '2026-11-21T00:00:00.000Z')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO wallets (tg_id, balance_idr, total_topup_idr, total_spent_idr, updated_at)
             VALUES (999, 0, 0, 0, '2026-11-21T00:00:00.000Z')",
            [],
        )
        .unwrap();
    }

    #[test]
    fn now_iso_is_rfc3339_utc() {
        let s = now_iso();
        assert!(s.ends_with('Z'), "harus UTC Z: {s}");
        let dt = chrono::DateTime::parse_from_rfc3339(&s).expect("parse RFC3339");
        assert_eq!(dt.offset().local_minus_utc(), 0);
    }
}
