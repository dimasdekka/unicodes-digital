//! Dompet pembeli, pengguna bot, kode promo, dan referral — port `src/lib/wallet.ts`.
//!
//! Semua mutasi saldo berjalan dalam SATU transaksi SQLite
//! (BEGIN IMMEDIATE ... COMMIT) supaya saldo + riwayat tidak pernah
//! setengah jalan.
//!
//! Aturan bisnis (default, bisa diubah Dim):
//! - Reward referral: referrer dapat kredit dompet Rp1.500 setelah teman yang
//!   diundang menyelesaikan order berbayar PERTAMA (dibayar sekali per teman).
//! - Min isi saldo Rp10.000 (pilihan cepat 10rb/25rb/50rb/100rb + custom).
//!
//! Konvensi Rust di sini: fungsi publik menerima `&Db` (Arc<Mutex<Connection>>)
//! dan mengunci sekali; helper internal menerima `&Connection` supaya tidak
//! ada deadlock (Mutex tidak re-entrant) saat dipanggil di dalam transaksi.

use crate::db::{now_iso, Db};
use rand::Rng;
use rusqlite::Connection;

pub const REFERRAL_REWARD_IDR: i64 = 1500;
pub const MIN_TOPUP_IDR: i64 = 10000;
pub const TOPUP_AMOUNTS: [i64; 4] = [10000, 25000, 50000, 100000];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lang {
    Id,
    En,
}

impl Lang {
    pub fn as_str(&self) -> &'static str {
        match self {
            Lang::Id => "id",
            Lang::En => "en",
        }
    }

    pub fn from_str(s: &str) -> Lang {
        if s == "en" {
            Lang::En
        } else {
            Lang::Id
        }
    }
}

/// Baris tabel `users` — cerminan baris DB; tidak semua field dibaca
/// jalur produksi (dipakai tes + admin).
#[allow(dead_code)]
#[derive(Debug, Clone)]
pub struct BotUser {
    pub tg_id: i64,
    pub first_name: Option<String>,
    pub username: Option<String>,
    pub lang: Lang,
    pub referrer_tg_id: Option<i64>,
}

/// Baris tabel `wallets` — cerminan baris DB; tidak semua field dibaca
/// jalur produksi (dipakai tes + admin).
#[allow(dead_code)]
#[derive(Debug, Clone)]
pub struct Wallet {
    pub tg_id: i64,
    pub balance_idr: i64,
    pub total_topup_idr: i64,
    pub total_spent_idr: i64,
}

/// Baris tabel `wallet_txns` — cerminan baris DB; tidak semua field
/// dibaca jalur produksi (dipakai tes + admin).
#[allow(dead_code)]
#[derive(Debug, Clone)]
pub struct WalletTxn {
    pub id: i64,
    pub kind: String,
    pub amount_idr: i64,
    pub balance_after_idr: i64,
    pub ref_: Option<String>,
    pub note: Option<String>,
    pub created_at: String,
}

/// Baris tabel `wallet_topups` — cerminan baris DB; tidak semua field
/// dibaca jalur produksi (dipakai tes + admin).
#[allow(dead_code)]
#[derive(Debug, Clone)]
pub struct WalletTopup {
    pub id: String,
    pub tg_id: i64,
    pub amount_idr: i64,
    pub status: String,
    pub created_at: String,
    pub paid_at: Option<String>,
}

/// Baris tabel `promo_codes` — cerminan baris DB; tidak semua field
/// dibaca jalur produksi (dipakai tes + admin).
#[allow(dead_code)]
#[derive(Debug, Clone)]
pub struct PromoCode {
    pub code: String,
    pub kind: String, // "percent" | "fixed"
    pub value: i64,
    pub max_uses: Option<i64>,
    pub uses_count: i64,
    pub valid_until: Option<String>,
    pub created_by: Option<String>,
    pub created_at: String,
}

/// Statistik referral satu user — cerminan hasil query; tidak semua
/// field dibaca jalur produksi (dipakai tes + admin).
#[allow(dead_code)]
#[derive(Debug, Clone)]
pub struct ReferralStats {
    pub invited: i64,
    pub completed: i64, // teman yang sudah order berbayar pertama
    pub rewarded: i64,  // yang reward-nya sudah dibayar
    pub credit_earned: i64,
}

/// Jalankan `f` di dalam satu transaksi SQLite (atomic, BEGIN IMMEDIATE).
/// Persis `withTx` di TS.
pub fn with_tx<T>(db: &Db, f: impl FnOnce(&Connection) -> anyhow::Result<T>) -> anyhow::Result<T> {
    let conn = db.lock().map_err(|e| anyhow::anyhow!("db lock poisoned: {e}"))?;
    conn.execute_batch("BEGIN IMMEDIATE")?;
    match f(&conn) {
        Ok(v) => {
            conn.execute_batch("COMMIT")?;
            Ok(v)
        }
        Err(e) => {
            let _ = conn.execute_batch("ROLLBACK");
            Err(e)
        }
    }
}

/// Sufiks acak base36 (huruf+angka kecil), meniru
/// `Math.random().toString(36).slice(2, 8)` di TS.
fn rand_base36(n: usize) -> String {
    const CHARS: &[u8] = b"0123456789abcdefghijklmnopqrstuvwxyz";
    let mut rng = rand::thread_rng();
    (0..n)
        .map(|_| CHARS[rng.gen_range(0..CHARS.len())] as char)
        .collect()
}

fn lock(db: &Db) -> anyhow::Result<std::sync::MutexGuard<'_, Connection>> {
    db.lock().map_err(|e| anyhow::anyhow!("db lock poisoned: {e}"))
}

// ---------------------------------------------------------------- users ---

fn get_user_conn(conn: &Connection, tg_id: i64) -> Option<BotUser> {
    conn.query_row(
        "SELECT tg_id, first_name, username, lang, referrer_tg_id FROM users WHERE tg_id = ?1",
        rusqlite::params![tg_id],
        |row| {
            Ok(BotUser {
                tg_id: row.get(0)?,
                first_name: row.get(1)?,
                username: row.get(2)?,
                lang: Lang::from_str(&row.get::<_, String>(3)?),
                referrer_tg_id: row.get(4)?,
            })
        },
    )
    .ok()
}

pub struct EnsureUserInput {
    pub tg_id: i64,
    pub first_name: Option<String>,
    pub username: Option<String>,
    pub referrer_tg_id: Option<i64>,
}

pub fn ensure_user(db: &Db, input: EnsureUserInput) -> BotUser {
    let now = now_iso();
    if let Ok(conn) = lock(db) {
        let _ = conn.execute(
            "INSERT INTO users (tg_id, first_name, username, lang, referrer_tg_id, created_at, updated_at)
             VALUES (?1, ?2, ?3, 'id', ?4, ?5, ?5)
             ON CONFLICT(tg_id) DO UPDATE SET
               first_name = COALESCE(excluded.first_name, users.first_name),
               username = COALESCE(excluded.username, users.username),
               referrer_tg_id = COALESCE(users.referrer_tg_id, excluded.referrer_tg_id),
               updated_at = excluded.updated_at",
            rusqlite::params![
                input.tg_id,
                input.first_name,
                input.username,
                input.referrer_tg_id,
                now,
            ],
        );
        let _ = conn.execute(
            "INSERT OR IGNORE INTO wallets (tg_id, balance_idr, total_topup_idr, total_spent_idr, updated_at)
             VALUES (?1, 0, 0, 0, ?2)",
            rusqlite::params![input.tg_id, now],
        );
    }
    get_user(db, input.tg_id).unwrap_or(BotUser {
        tg_id: input.tg_id,
        first_name: input.first_name,
        username: input.username,
        lang: Lang::Id,
        referrer_tg_id: input.referrer_tg_id,
    })
}

pub fn get_user(db: &Db, tg_id: i64) -> Option<BotUser> {
    let conn = db.lock().ok()?;
    get_user_conn(&conn, tg_id)
}

pub fn set_user_lang(db: &Db, tg_id: i64, lang: Lang) -> anyhow::Result<()> {
    let conn = lock(db)?;
    conn.execute(
        "UPDATE users SET lang = ?1, updated_at = ?2 WHERE tg_id = ?3",
        rusqlite::params![lang.as_str(), now_iso(), tg_id],
    )?;
    Ok(())
}

// --------------------------------------------------------------- wallet ---

/// Mutasi saldo + catat riwayat. Harus dipanggil di dalam with_tx.
/// `amount_idr`: +masuk, -keluar. Mengembalikan saldo akhir.
/// Error "Saldo tidak cukup" kalau saldo akhir < 0.
fn apply_wallet_change(
    conn: &Connection,
    tg_id: i64,
    amount_idr: i64,
    kind: &str,
    ref_: Option<&str>,
    note: Option<&str>,
) -> anyhow::Result<i64> {
    let balance: i64 = conn
        .query_row(
            "SELECT balance_idr FROM wallets WHERE tg_id = ?1",
            rusqlite::params![tg_id],
            |row| row.get(0),
        )
        .map_err(|_| anyhow::anyhow!("Dompet tidak ditemukan: {tg_id}"))?;
    let balance_after = balance + amount_idr;
    if balance_after < 0 {
        return Err(anyhow::anyhow!("Saldo tidak cukup"));
    }
    conn.execute(
        "UPDATE wallets SET balance_idr = ?1,
           total_topup_idr = total_topup_idr + ?2,
           total_spent_idr = total_spent_idr + ?3,
           updated_at = ?4 WHERE tg_id = ?5",
        rusqlite::params![
            balance_after,
            if amount_idr > 0 && kind == "topup" { amount_idr } else { 0 },
            if amount_idr < 0 { -amount_idr } else { 0 },
            now_iso(),
            tg_id,
        ],
    )?;
    conn.execute(
        "INSERT INTO wallet_txns (tg_id, kind, amount_idr, balance_after_idr, ref, note, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        rusqlite::params![tg_id, kind, amount_idr, balance_after, ref_, note, now_iso()],
    )?;
    Ok(balance_after)
}

pub fn get_wallet(db: &Db, tg_id: i64) -> Wallet {
    let row = db.lock().ok().and_then(|conn| {
        conn.query_row(
            "SELECT tg_id, balance_idr, total_topup_idr, total_spent_idr FROM wallets WHERE tg_id = ?1",
            rusqlite::params![tg_id],
            |row| {
                Ok(Wallet {
                    tg_id: row.get(0)?,
                    balance_idr: row.get(1)?,
                    total_topup_idr: row.get(2)?,
                    total_spent_idr: row.get(3)?,
                })
            },
        )
        .ok()
    });
    match row {
        Some(w) => w,
        None => {
            ensure_user(
                db,
                EnsureUserInput {
                    tg_id,
                    first_name: None,
                    username: None,
                    referrer_tg_id: None,
                },
            );
            Wallet {
                tg_id,
                balance_idr: 0,
                total_topup_idr: 0,
                total_spent_idr: 0,
            }
        }
    }
}

/// Tambah saldo. Mengembalikan saldo akhir.
/// Dipakai tes; jalur produksi memakai `mark_topup_paid` (idempotent).
#[allow(dead_code)]
pub fn credit_wallet(
    db: &Db,
    tg_id: i64,
    amount_idr: i64,
    kind: &str,
    ref_: Option<&str>,
    note: Option<&str>,
) -> anyhow::Result<i64> {
    if amount_idr <= 0 {
        return Err(anyhow::anyhow!("Jumlah kredit harus positif"));
    }
    with_tx(db, |conn| {
        apply_wallet_change(conn, tg_id, amount_idr, kind, ref_, note)
    })
}

/// Potong saldo. Mengembalikan saldo akhir, atau None kalau tidak cukup.
pub fn debit_wallet(
    db: &Db,
    tg_id: i64,
    amount_idr: i64,
    kind: &str,
    ref_: Option<&str>,
    note: Option<&str>,
) -> anyhow::Result<Option<i64>> {
    if amount_idr <= 0 {
        return Err(anyhow::anyhow!("Jumlah debit harus positif"));
    }
    match with_tx(db, |conn| {
        apply_wallet_change(conn, tg_id, -amount_idr, kind, ref_, note)
    }) {
        Ok(v) => Ok(Some(v)),
        Err(e) if e.to_string() == "Saldo tidak cukup" => Ok(None),
        Err(e) => Err(e),
    }
}

pub fn list_wallet_txns(db: &Db, tg_id: i64, limit: i64) -> Vec<WalletTxn> {
    let conn = match db.lock() {
        Ok(g) => g,
        Err(_) => return Vec::new(),
    };
    let mut stmt = match conn.prepare(
        "SELECT id, kind, amount_idr, balance_after_idr, ref, note, created_at \
         FROM wallet_txns WHERE tg_id = ?1 ORDER BY id DESC LIMIT ?2",
    ) {
        Ok(s) => s,
        Err(_) => return Vec::new(),
    };
    let rows = stmt.query_map(rusqlite::params![tg_id, limit], |row| {
        Ok(WalletTxn {
            id: row.get(0)?,
            kind: row.get(1)?,
            amount_idr: row.get(2)?,
            balance_after_idr: row.get(3)?,
            ref_: row.get(4)?,
            note: row.get(5)?,
            created_at: row.get(6)?,
        })
    });
    match rows {
        Ok(m) => m.filter_map(|r| r.ok()).collect(),
        Err(_) => Vec::new(),
    }
}

// --------------------------------------------------------------- topup ----

fn new_topup_id() -> String {
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    format!("TOPUP-{}-{}", base36(ts as u64), rand_base36(6)).to_uppercase()
}

/// `Date.now().toString(36)` — dipakai juga untuk ID order.
pub fn base36(mut n: u64) -> String {
    if n == 0 {
        return "0".to_string();
    }
    const CHARS: &[u8] = b"0123456789abcdefghijklmnopqrstuvwxyz";
    let mut out = Vec::new();
    while n > 0 {
        out.push(CHARS[(n % 36) as usize] as char);
        n /= 36;
    }
    out.into_iter().rev().collect()
}

pub fn create_topup(db: &Db, tg_id: i64, amount_idr: i64) -> anyhow::Result<WalletTopup> {
    if amount_idr < MIN_TOPUP_IDR {
        return Err(anyhow::anyhow!("Minimal isi saldo {MIN_TOPUP_IDR}"));
    }
    let topup = WalletTopup {
        id: new_topup_id(),
        tg_id,
        amount_idr,
        status: "awaiting_payment".to_string(),
        created_at: now_iso(),
        paid_at: None,
    };
    let conn = lock(db)?;
    conn.execute(
        "INSERT INTO wallet_topups (id, tg_id, amount_idr, status, created_at, paid_at) VALUES (?1, ?2, ?3, ?4, ?5, NULL)",
        rusqlite::params![topup.id, topup.tg_id, topup.amount_idr, topup.status, topup.created_at],
    )?;
    Ok(topup)
}

fn row_to_topup(row: &rusqlite::Row) -> rusqlite::Result<WalletTopup> {
    Ok(WalletTopup {
        id: row.get(0)?,
        tg_id: row.get(1)?,
        amount_idr: row.get(2)?,
        status: row.get(3)?,
        created_at: row.get(4)?,
        paid_at: row.get(5)?,
    })
}

pub fn get_topup(db: &Db, id: &str) -> Option<WalletTopup> {
    let conn = db.lock().ok()?;
    conn.query_row(
        "SELECT id, tg_id, amount_idr, status, created_at, paid_at FROM wallet_topups WHERE id = ?1",
        rusqlite::params![id],
        row_to_topup,
    )
    .ok()
}

/// Tandai top-up lunas + kredit dompet. Idempotent: top-up yang sudah "paid"
/// tidak dikredit dua kali (dipakai poller QRIS — feed bisa mengulang
/// transaksi yang sama di tick berikutnya).
pub fn mark_topup_paid(db: &Db, id: &str) -> (bool, Option<WalletTopup>) {
    let topup = match get_topup(db, id) {
        Some(t) => t,
        None => return (false, None),
    };
    if topup.status == "paid" {
        return (false, Some(topup));
    }
    let tg_id = topup.tg_id;
    let amount = topup.amount_idr;
    let done = with_tx(db, |conn| {
        let status: Option<String> = conn
            .query_row(
                "SELECT status FROM wallet_topups WHERE id = ?1",
                rusqlite::params![id],
                |row| row.get(0),
            )
            .ok();
        if status.as_deref() != Some("awaiting_payment") {
            return Ok(false);
        }
        conn.execute(
            "UPDATE wallet_topups SET status = 'paid', paid_at = ?1 WHERE id = ?2",
            rusqlite::params![now_iso(), id],
        )?;
        apply_wallet_change(conn, tg_id, amount, "topup", Some(id), Some("Isi saldo via QRIS"))?;
        Ok(true)
    })
    .unwrap_or(false);
    (done, get_topup(db, id))
}

// ---------------------------------------------------------------- promo ---

pub struct CreatePromoInput {
    pub code: String,
    pub kind: String, // "percent" | "fixed"
    pub value: i64,
    pub max_uses: Option<i64>,
    pub valid_days: Option<i64>,
    pub created_by: Option<String>,
}

fn is_valid_promo_code(code: &str) -> bool {
    (3..=20).contains(&code.len())
        && code.chars().all(|c| c.is_ascii_alphanumeric())
}

pub fn create_promo_code(db: &Db, input: CreatePromoInput) -> anyhow::Result<PromoCode> {
    let code = input.code.trim().to_uppercase();
    if !is_valid_promo_code(&code) {
        return Err(anyhow::anyhow!("Kode promo hanya huruf/angka, 3–20 karakter"));
    }
    if input.kind == "percent" && !(1..=100).contains(&input.value) {
        return Err(anyhow::anyhow!("Persen diskon harus 1–100"));
    }
    if input.kind == "fixed" && input.value < 100 {
        return Err(anyhow::anyhow!("Potongan tetap minimal Rp100"));
    }
    let valid_until = input.valid_days.map(|days| {
        let dt = chrono::Utc::now() + chrono::Duration::days(days);
        dt.to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
    });
    let promo = PromoCode {
        code: code.clone(),
        kind: input.kind.clone(),
        value: input.value,
        max_uses: input.max_uses,
        uses_count: 0,
        valid_until: valid_until.clone(),
        created_by: input.created_by.clone(),
        created_at: now_iso(),
    };
    let conn = lock(db)?;
    conn.execute(
        "INSERT INTO promo_codes (code, kind, value, max_uses, uses_count, valid_until, created_by, created_at)
         VALUES (?1, ?2, ?3, ?4, 0, ?5, ?6, ?7)",
        rusqlite::params![
            promo.code,
            promo.kind,
            promo.value,
            promo.max_uses,
            promo.valid_until,
            promo.created_by,
            promo.created_at,
        ],
    )?;
    Ok(promo)
}

fn row_to_promo(row: &rusqlite::Row) -> rusqlite::Result<PromoCode> {
    Ok(PromoCode {
        code: row.get(0)?,
        kind: row.get(1)?,
        value: row.get(2)?,
        max_uses: row.get(3)?,
        uses_count: row.get(4)?,
        valid_until: row.get(5)?,
        created_by: row.get(6)?,
        created_at: row.get(7)?,
    })
}

pub fn list_promo_codes(db: &Db) -> Vec<PromoCode> {
    let conn = match db.lock() {
        Ok(g) => g,
        Err(_) => return Vec::new(),
    };
    let mut stmt = match conn.prepare("SELECT * FROM promo_codes ORDER BY created_at DESC") {
        Ok(s) => s,
        Err(_) => return Vec::new(),
    };
    let rows = match stmt.query_map([], row_to_promo) {
        Ok(m) => m.filter_map(|r| r.ok()).collect(),
        Err(_) => Vec::new(),
    };
    rows
}

pub fn get_promo_code(db: &Db, code: &str) -> Option<PromoCode> {
    let conn = db.lock().ok()?;
    conn.query_row(
        "SELECT * FROM promo_codes WHERE code = ?1",
        rusqlite::params![code.trim().to_uppercase()],
        row_to_promo,
    )
    .ok()
}

pub fn has_redeemed_promo(db: &Db, code: &str, tg_id: i64) -> bool {
    let conn = match db.lock() {
        Ok(g) => g,
        Err(_) => return false,
    };
    conn.query_row(
        "SELECT 1 FROM promo_redemptions WHERE code = ?1 AND tg_id = ?2",
        rusqlite::params![code.trim().to_uppercase(), tg_id],
        |_| Ok(()),
    )
    .is_ok()
}

pub enum PromoValidation {
    Ok(PromoCode),
    NotFound,
    Expired,
    Exhausted,
    AlreadyUsed,
}

impl PromoValidation {
    /// Alasan string persis TS (`validatePromo` reason): not_found / expired /
    /// exhausted / already_used. None kalau valid.
    pub fn reason(&self) -> Option<&'static str> {
        match self {
            PromoValidation::Ok(_) => None,
            PromoValidation::NotFound => Some("not_found"),
            PromoValidation::Expired => Some("expired"),
            PromoValidation::Exhausted => Some("exhausted"),
            PromoValidation::AlreadyUsed => Some("already_used"),
        }
    }
}

/// Validasi kode untuk satu pengguna.
pub fn validate_promo(db: &Db, code: &str, tg_id: i64) -> PromoValidation {
    let promo = match get_promo_code(db, code) {
        Some(p) => p,
        None => return PromoValidation::NotFound,
    };
    if let Some(until) = &promo.valid_until {
        if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(until) {
            if dt < chrono::Utc::now() {
                return PromoValidation::Expired;
            }
        }
    }
    if let Some(max) = promo.max_uses {
        if promo.uses_count >= max {
            return PromoValidation::Exhausted;
        }
    }
    if has_redeemed_promo(db, &promo.code, tg_id) {
        return PromoValidation::AlreadyUsed;
    }
    PromoValidation::Ok(promo)
}

/// Hitung potongan untuk satu total belanja (tanpa menyimpan apa pun).
pub fn calc_promo_discount(promo: &PromoCode, total_idr: i64) -> i64 {
    if promo.kind == "percent" {
        ((total_idr * promo.value) as f64 / 100.0).round() as i64
    } else {
        promo.value
    }
    .min(total_idr)
}

/// Tukar promo (dipanggil saat checkout dibayar). Menambah uses_count +
/// mencatat redemption. Mengembalikan false kalau kuota habis / sudah
/// dipakai (race).
pub fn redeem_promo(
    db: &Db,
    code: &str,
    tg_id: i64,
    order_id: &str,
    discount_idr: i64,
) -> bool {
    with_tx(db, |conn| {
        let promo: Option<(Option<i64>, i64, Option<String>)> = conn
            .query_row(
                "SELECT max_uses, uses_count, valid_until FROM promo_codes WHERE code = ?1",
                rusqlite::params![code],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .ok();
        let Some((max_uses, uses_count, valid_until)) = promo else {
            return Ok(false);
        };
        if let Some(until) = valid_until {
            if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(&until) {
                if dt < chrono::Utc::now() {
                    return Ok(false);
                }
            }
        }
        if let Some(max) = max_uses {
            if uses_count >= max {
                return Ok(false);
            }
        }
        let dup: bool = conn
            .query_row(
                "SELECT 1 FROM promo_redemptions WHERE code = ?1 AND tg_id = ?2",
                rusqlite::params![code, tg_id],
                |_| Ok(()),
            )
            .is_ok();
        if dup {
            return Ok(false);
        }
        conn.execute(
            "INSERT INTO promo_redemptions (code, tg_id, order_id, discount_idr, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            rusqlite::params![code, tg_id, order_id, discount_idr, now_iso()],
        )?;
        conn.execute(
            "UPDATE promo_codes SET uses_count = uses_count + 1 WHERE code = ?1",
            rusqlite::params![code],
        )?;
        Ok(true)
    })
    .unwrap_or(false)
}

// ------------------------------------------------------------- referral ---

pub fn referral_code(tg_id: i64) -> String {
    format!("UD-{tg_id}")
}

pub fn referral_link(tg_id: i64) -> String {
    format!(
        "https://t.me/unicodesdigital_bot?start=ref_{}",
        referral_code(tg_id)
    )
}

/// Catat undangan (sekali per teman). Mengembalikan false kalau sudah ada / self.
pub fn record_referral(db: &Db, referrer_tg_id: i64, referred_tg_id: i64) -> bool {
    if referrer_tg_id == referred_tg_id {
        return false;
    }
    let conn = match db.lock() {
        Ok(g) => g,
        Err(_) => return false,
    };
    match conn.execute(
        "INSERT OR IGNORE INTO referrals (referrer_tg_id, referred_tg_id, rewarded, created_at)
         VALUES (?1, ?2, 0, ?3)",
        rusqlite::params![referrer_tg_id, referred_tg_id, now_iso()],
    ) {
        Ok(changed) => changed > 0,
        Err(_) => false,
    }
}

pub fn get_referral_stats(db: &Db, tg_id: i64) -> ReferralStats {
    let conn = match db.lock() {
        Ok(g) => g,
        Err(_) => {
            return ReferralStats {
                invited: 0,
                completed: 0,
                rewarded: 0,
                credit_earned: 0,
            }
        }
    };
    let count = |sql: &str| -> i64 {
        conn.query_row(sql, rusqlite::params![tg_id], |row| row.get::<_, i64>(0))
            .unwrap_or(0)
    };
    let invited = count("SELECT COUNT(*) FROM referrals WHERE referrer_tg_id = ?1");
    let completed = count(
        "SELECT COUNT(*) FROM referrals r
         WHERE r.referrer_tg_id = ?1
           AND EXISTS (SELECT 1 FROM store_orders o
                        WHERE o.buyer_ref = 'tg:' || r.referred_tg_id
                          AND o.status IN ('paid', 'processing', 'delivered')
                          AND o.retail_total_idr > 0)",
    );
    let rewarded = count(
        "SELECT COUNT(*) FROM referrals WHERE referrer_tg_id = ?1 AND rewarded = 1",
    );
    let credit_earned: i64 = conn
        .query_row(
            "SELECT COALESCE(SUM(amount_idr), 0) FROM wallet_txns
             WHERE tg_id = ?1 AND kind = 'referral_reward'",
            rusqlite::params![tg_id],
            |row| row.get(0),
        )
        .unwrap_or(0);
    ReferralStats {
        invited,
        completed,
        rewarded,
        credit_earned,
    }
}

pub fn top_referrers(db: &Db, limit: i64) -> Vec<(i64, i64, i64)> {
    let conn = match db.lock() {
        Ok(g) => g,
        Err(_) => return Vec::new(),
    };
    let mut stmt = match conn.prepare(
        "SELECT referrer_tg_id, COUNT(*) AS invited,
                COALESCE(SUM(rewarded), 0) AS rewarded
         FROM referrals GROUP BY referrer_tg_id
         ORDER BY rewarded DESC, invited DESC LIMIT ?1",
    ) {
        Ok(s) => s,
        Err(_) => return Vec::new(),
    };
    let rows = match stmt.query_map(rusqlite::params![limit], |row| {
        Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?, row.get::<_, i64>(2)?))
    }) {
        Ok(m) => m.filter_map(|r| r.ok()).collect(),
        Err(_) => Vec::new(),
    };
    rows
}

/// Bayar reward referral setelah teman menyelesaikan order berbayar PERTAMA.
/// Idempotent (rewarded=0 dicek + di-update atomik) + hanya untuk order pertama
/// teman itu. Dipanggil setelah markPaid (poller QRIS & bayar saldo).
/// Mengembalikan true kalau reward dibayarkan.
pub fn maybe_reward_referrer(db: &Db, referred_tg_id: i64) -> bool {
    // Referrer bisa saja belum pernah memakai bot (tidak punya baris users /
    // wallets) — buatkan dulu supaya kredit reward ada tempatnya.
    let referrer: Option<i64> = db.lock().ok().and_then(|conn| {
        conn.query_row(
            "SELECT referrer_tg_id FROM referrals WHERE referred_tg_id = ?1 AND rewarded = 0",
            rusqlite::params![referred_tg_id],
            |row| row.get(0),
        )
        .ok()
    });
    if let Some(r) = referrer {
        ensure_user(
            db,
            EnsureUserInput {
                tg_id: r,
                first_name: None,
                username: None,
                referrer_tg_id: None,
            },
        );
    }
    with_tx(db, |conn| {
        let referrer: Option<i64> = conn
            .query_row(
                "SELECT referrer_tg_id FROM referrals WHERE referred_tg_id = ?1 AND rewarded = 0",
                rusqlite::params![referred_tg_id],
                |row| row.get(0),
            )
            .ok();
        let Some(referrer) = referrer else {
            return Ok(false);
        };
        // Hanya order berbayar pertama yang memicu reward (total > 0 — order
        // gratis dari promo 100% tidak dihitung).
        let paid_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM store_orders
                 WHERE buyer_ref = ?1 AND status IN ('paid', 'processing', 'delivered')
                   AND retail_total_idr > 0",
                rusqlite::params![format!("tg:{referred_tg_id}")],
                |row| row.get(0),
            )
            .unwrap_or(0);
        if paid_count != 1 {
            return Ok(false);
        }
        let claimed = conn.execute(
            "UPDATE referrals SET rewarded = 1, rewarded_at = ?1 WHERE referred_tg_id = ?2 AND rewarded = 0",
            rusqlite::params![now_iso(), referred_tg_id],
        )?;
        if claimed == 0 {
            return Ok(false);
        }
        apply_wallet_change(
            conn,
            referrer,
            REFERRAL_REWARD_IDR,
            "referral_reward",
            Some(&format!("REF-{referred_tg_id}")),
            Some(&format!("Bonus referral — teman {referred_tg_id} order pertama")),
        )?;
        Ok(true)
    })
    .unwrap_or(false)
}

/// Refund order yang dibayar dari dompet kembali ke dompet pembeli.
pub fn refund_wallet_for_order(db: &Db, tg_id: i64, amount_idr: i64, order_id: &str) -> anyhow::Result<i64> {
    with_tx(db, |conn| {
        apply_wallet_change(
            conn,
            tg_id,
            amount_idr,
            "refund",
            Some(order_id),
            Some("Refund — fulfillment gagal"),
        )
    })
}

/// Ambil tg id dari buyerRef format `tg:<id>` (None kalau bukan format itu).
pub fn tg_id_from_buyer_ref(buyer_ref: Option<&str>) -> Option<i64> {
    let s = buyer_ref?;
    let rest = s.strip_prefix("tg:")?;
    let n: i64 = rest.parse().ok()?;
    if n > 0 {
        Some(n)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db;

    fn test_db() -> Db {
        db::open(":memory:").expect("db tes")
    }

    fn user(db: &Db, tg_id: i64) -> BotUser {
        ensure_user(
            db,
            EnsureUserInput {
                tg_id,
                first_name: Some("Test".to_string()),
                username: Some(format!("user{tg_id}")),
                referrer_tg_id: None,
            },
        )
    }

    #[test]
    fn ensure_user_creates_wallet_and_lang() {
        let db = test_db();
        let u = user(&db, 111);
        assert_eq!(u.tg_id, 111);
        assert_eq!(u.lang, Lang::Id);
        assert_eq!(get_wallet(&db, 111).balance_idr, 0);
        // Panggil ulang: tidak duplikat, data dipertahankan.
        let u2 = ensure_user(
            &db,
            EnsureUserInput {
                tg_id: 111,
                first_name: Some("Budi".to_string()),
                username: None,
                referrer_tg_id: None,
            },
        );
        assert_eq!(u2.first_name.as_deref(), Some("Budi"));
        assert_eq!(u2.username.as_deref(), Some("user111"));
    }

    #[test]
    fn set_user_lang_round_trip() {
        let db = test_db();
        user(&db, 111);
        set_user_lang(&db, 111, Lang::En).unwrap();
        assert_eq!(get_user(&db, 111).unwrap().lang, Lang::En);
        set_user_lang(&db, 111, Lang::Id).unwrap();
        assert_eq!(get_user(&db, 111).unwrap().lang, Lang::Id);
        assert_eq!(Lang::from_str("en"), Lang::En);
        assert_eq!(Lang::from_str("xx"), Lang::Id);
    }

    #[test]
    fn credit_and_debit_wallet() {
        // S5/S7: dompet Rp0 -> top-up -> bayar -> saldo berkurang.
        let db = test_db();
        user(&db, 111);
        assert_eq!(credit_wallet(&db, 111, 25000, "topup", Some("T1"), None).unwrap(), 25000);
        let w = get_wallet(&db, 111);
        assert_eq!(w.balance_idr, 25000);
        assert_eq!(w.total_topup_idr, 25000);
        assert_eq!(debit_wallet(&db, 111, 2000, "purchase", Some("UD-1"), None).unwrap(), Some(23000));
        assert_eq!(get_wallet(&db, 111).total_spent_idr, 2000);
        // Riwayat tercatat.
        let txns = list_wallet_txns(&db, 111, 10);
        assert_eq!(txns.len(), 2);
        assert_eq!(txns[0].kind, "purchase");
        assert_eq!(txns[0].amount_idr, -2000);
        assert_eq!(txns[0].balance_after_idr, 23000);
    }

    #[test]
    fn debit_insufficient_returns_none() {
        let db = test_db();
        user(&db, 111);
        credit_wallet(&db, 111, 1000, "topup", None, None).unwrap();
        assert_eq!(debit_wallet(&db, 111, 14000, "purchase", None, None).unwrap(), None);
        assert_eq!(get_wallet(&db, 111).balance_idr, 1000);
        // Tidak ada txn gagal yang tercatat.
        assert_eq!(list_wallet_txns(&db, 111, 10).len(), 1);
    }

    #[test]
    fn credit_and_debit_reject_non_positive() {
        let db = test_db();
        user(&db, 111);
        assert!(credit_wallet(&db, 111, 0, "topup", None, None).is_err());
        assert!(credit_wallet(&db, 111, -5, "topup", None, None).is_err());
        assert!(debit_wallet(&db, 111, 0, "purchase", None, None).is_err());
    }

    #[test]
    fn concurrent_credits_are_serialized() {
        // 8 thread kredit bersamaan: semua masuk, saldo akhir tepat.
        let db = test_db();
        user(&db, 111);
        let mut handles = Vec::new();
        for i in 0..8 {
            let db2 = db.clone();
            handles.push(std::thread::spawn(move || {
                credit_wallet(&db2, 111, 1000, "topup", Some(&format!("T{i}")), None)
            }));
        }
        for h in handles {
            assert!(h.join().unwrap().is_ok());
        }
        assert_eq!(get_wallet(&db, 111).balance_idr, 8000);
        assert_eq!(list_wallet_txns(&db, 111, 20).len(), 8);
    }

    #[test]
    fn topup_min_amount_and_mark_paid_idempotent() {
        // S5/S6/S21: minimal Rp10.000; tick duplikat tidak kredit 2x.
        let db = test_db();
        user(&db, 111);
        assert!(create_topup(&db, 111, 5000).is_err());
        assert!(create_topup(&db, 111, 9999).is_err());
        let topup = create_topup(&db, 111, 25000).unwrap();
        assert_eq!(topup.status, "awaiting_payment");
        assert!(topup.id.starts_with("TOPUP-"));
        let (paid, row) = mark_topup_paid(&db, &topup.id);
        assert!(paid);
        assert_eq!(row.unwrap().status, "paid");
        assert_eq!(get_wallet(&db, 111).balance_idr, 25000);
        // Tick kedua: idempotent.
        let (paid2, _) = mark_topup_paid(&db, &topup.id);
        assert!(!paid2);
        assert_eq!(get_wallet(&db, 111).balance_idr, 25000);
        // Top-up tidak ada: false.
        let (paid3, row3) = mark_topup_paid(&db, "TOPUP-TIDAKADA");
        assert!(!paid3);
        assert!(row3.is_none());
        assert_eq!(get_topup(&db, &topup.id).unwrap().amount_idr, 25000);
    }

    #[test]
    fn promo_create_validation() {
        let db = test_db();
        assert!(create_promo_code(
            &db,
            CreatePromoInput {
                code: "AB".to_string(),
                kind: "percent".to_string(),
                value: 10,
                max_uses: None,
                valid_days: None,
                created_by: None,
            }
        )
        .is_err());
        assert!(create_promo_code(
            &db,
            CreatePromoInput {
                code: "P101".to_string(),
                kind: "percent".to_string(),
                value: 101,
                max_uses: None,
                valid_days: None,
                created_by: None,
            }
        )
        .is_err());
        let p = create_promo_code(
            &db,
            CreatePromoInput {
                code: "test10".to_string(),
                kind: "percent".to_string(),
                value: 10,
                max_uses: Some(5),
                valid_days: Some(30),
                created_by: Some("admin".to_string()),
            },
        )
        .unwrap();
        assert_eq!(p.code, "TEST10"); // dinormalisasi ke huruf besar
        assert_eq!(p.uses_count, 0);
        assert_eq!(list_promo_codes(&db).len(), 1);
    }

    #[test]
    fn promo_validate_and_discount() {
        // S9: TEST10 10% dari Rp20.000 = Rp2.000, total Rp18.000.
        let db = test_db();
        create_promo_code(
            &db,
            CreatePromoInput {
                code: "TEST10".to_string(),
                kind: "percent".to_string(),
                value: 10,
                max_uses: Some(5),
                valid_days: Some(30),
                created_by: None,
            },
        )
        .unwrap();
        let promo = get_promo_code(&db, "TEST10").unwrap();
        assert_eq!(calc_promo_discount(&promo, 20000), 2000);
        assert!(matches!(validate_promo(&db, "TEST10", 111), PromoValidation::Ok(_)));
        assert_eq!(validate_promo(&db, "TIDAKADA", 111).reason(), Some("not_found"));
        // fixed: dipotong maksimal total belanja.
        create_promo_code(
            &db,
            CreatePromoInput {
                code: "FIX5000".to_string(),
                kind: "fixed".to_string(),
                value: 5000,
                max_uses: None,
                valid_days: None,
                created_by: None,
            },
        )
        .unwrap();
        let fixed = get_promo_code(&db, "FIX5000").unwrap();
        assert_eq!(calc_promo_discount(&fixed, 2000), 2000);
        assert_eq!(calc_promo_discount(&fixed, 20000), 5000);
    }

    #[test]
    fn promo_redeem_once_per_user() {
        // S9: redemption tercatat, uses_count = 1, pakai lagi ditolak.
        let db = test_db();
        create_promo_code(
            &db,
            CreatePromoInput {
                code: "TEST10".to_string(),
                kind: "percent".to_string(),
                value: 10,
                max_uses: Some(5),
                valid_days: Some(30),
                created_by: None,
            },
        )
        .unwrap();
        assert!(redeem_promo(&db, "TEST10", 111, "UD-1", 2000));
        assert!(has_redeemed_promo(&db, "TEST10", 111));
        assert!(!has_redeemed_promo(&db, "TEST10", 222));
        assert_eq!(get_promo_code(&db, "TEST10").unwrap().uses_count, 1);
        assert_eq!(
            validate_promo(&db, "TEST10", 111).reason(),
            Some("already_used")
        );
        assert!(!redeem_promo(&db, "TEST10", 111, "UD-2", 2000));
        // User lain masih bisa.
        assert!(redeem_promo(&db, "TEST10", 222, "UD-3", 2000));
        assert_eq!(get_promo_code(&db, "TEST10").unwrap().uses_count, 2);
    }

    #[test]
    fn promo_expired_and_exhausted() {
        // S22: kedaluwarsa + habis kuota.
        let db = test_db();
        create_promo_code(
            &db,
            CreatePromoInput {
                code: "EXPIRED1".to_string(),
                kind: "percent".to_string(),
                value: 10,
                max_uses: None,
                valid_days: Some(-1),
                created_by: None,
            },
        )
        .unwrap();
        assert_eq!(validate_promo(&db, "EXPIRED1", 111).reason(), Some("expired"));
        create_promo_code(
            &db,
            CreatePromoInput {
                code: "HABIS1".to_string(),
                kind: "fixed".to_string(),
                value: 1000,
                max_uses: Some(1),
                valid_days: None,
                created_by: None,
            },
        )
        .unwrap();
        assert!(redeem_promo(&db, "HABIS1", 333, "UD-HABIS-1", 1000));
        assert_eq!(validate_promo(&db, "HABIS1", 111).reason(), Some("exhausted"));
        assert!(!redeem_promo(&db, "HABIS1", 111, "UD-HABIS-2", 1000));
        assert!(!redeem_promo(&db, "TIDAKADA", 111, "UD-X", 1000));
    }

    #[test]
    fn referral_record_and_self_block() {
        // S2/S24: referral tercatat; diri sendiri diblokir; duplikat ditolak.
        let db = test_db();
        assert!(record_referral(&db, 222, 333));
        assert!(!record_referral(&db, 222, 333));
        assert!(!record_referral(&db, 111, 111));
        assert_eq!(get_referral_stats(&db, 222).invited, 1);
        assert_eq!(get_referral_stats(&db, 111).invited, 0);
        assert_eq!(referral_code(222), "UD-222");
        assert!(referral_link(222).contains("ref_UD-222"));
    }

    #[test]
    fn referral_reward_once_on_first_paid_order() {
        // S8: referrer dapat Rp1.500 sekali; order kedua tidak dobel.
        let db = test_db();
        user(&db, 222);
        user(&db, 333);
        assert!(record_referral(&db, 222, 333));
        // Belum ada order berbayar -> tidak ada reward.
        assert!(!maybe_reward_referrer(&db, 333));
        // Order pertama teman (paid) -> reward.
        let o1 = crate::lib::orders::create_store_order(
            &db,
            crate::lib::orders::CreateStoreOrderInput {
                id: None,
                catalog_product_id: "muse-token-1b".to_string(),
                quantity: Some(1),
                buyer_ref: Some("tg:333".to_string()),
                xiaomi_accounts: None,
                sheerid_url: None,
                target: Some("UVI0QD".to_string()),
                discount_idr: None,
                promo_code: None,
            },
        )
        .unwrap();
        crate::lib::orders::mark_paid(&db, &o1.id).unwrap();
        assert!(maybe_reward_referrer(&db, 333));
        assert_eq!(get_wallet(&db, 222).balance_idr, REFERRAL_REWARD_IDR);
        let stats = get_referral_stats(&db, 222);
        assert_eq!(stats.rewarded, 1);
        assert_eq!(stats.completed, 1);
        assert_eq!(stats.credit_earned, 1500);
        // Order kedua -> tidak dobel.
        let o2 = crate::lib::orders::create_store_order(
            &db,
            crate::lib::orders::CreateStoreOrderInput {
                id: None,
                catalog_product_id: "muse-token-1b".to_string(),
                quantity: Some(1),
                buyer_ref: Some("tg:333".to_string()),
                xiaomi_accounts: None,
                sheerid_url: None,
                target: Some("UVI0QD".to_string()),
                discount_idr: None,
                promo_code: None,
            },
        )
        .unwrap();
        crate::lib::orders::mark_paid(&db, &o2.id).unwrap();
        assert!(!maybe_reward_referrer(&db, 333));
        assert_eq!(get_wallet(&db, 222).balance_idr, 1500);
    }

    #[test]
    fn referral_reward_skips_free_orders() {
        // Order gratis (total 0 karena promo 100%) tidak memicu reward.
        let db = test_db();
        user(&db, 222);
        user(&db, 333);
        record_referral(&db, 222, 333);
        let o = crate::lib::orders::create_store_order(
            &db,
            crate::lib::orders::CreateStoreOrderInput {
                id: None,
                catalog_product_id: "muse-token-1b".to_string(),
                quantity: Some(1),
                buyer_ref: Some("tg:333".to_string()),
                xiaomi_accounts: None,
                sheerid_url: None,
                target: Some("UVI0QD".to_string()),
                discount_idr: Some(2000),
                promo_code: Some("FREE100".to_string()),
            },
        )
        .unwrap();
        assert_eq!(o.retail_total_idr, 0);
        crate::lib::orders::mark_paid(&db, &o.id).unwrap();
        assert!(!maybe_reward_referrer(&db, 333));
        assert_eq!(get_wallet(&db, 222).balance_idr, 0);
    }

    #[test]
    fn tg_id_from_buyer_ref_parses() {
        assert_eq!(tg_id_from_buyer_ref(Some("tg:111")), Some(111));
        assert_eq!(tg_id_from_buyer_ref(Some("web")), None);
        assert_eq!(tg_id_from_buyer_ref(Some("tg:0")), None);
        assert_eq!(tg_id_from_buyer_ref(Some("tg:abc")), None);
        assert_eq!(tg_id_from_buyer_ref(None), None);
    }

    #[test]
    fn top_referrers_lists() {
        let db = test_db();
        record_referral(&db, 222, 333);
        record_referral(&db, 222, 444);
        record_referral(&db, 555, 666);
        let top = top_referrers(&db, 10);
        assert_eq!(top.len(), 2);
        assert_eq!(top[0].0, 222);
        assert_eq!(top[0].1, 2);
    }

    #[test]
    fn refund_wallet_for_order_restores_balance() {
        let db = test_db();
        user(&db, 111);
        credit_wallet(&db, 111, 50000, "topup", None, None).unwrap();
        debit_wallet(&db, 111, 2000, "purchase", Some("UD-1"), None).unwrap();
        assert_eq!(refund_wallet_for_order(&db, 111, 2000, "UD-1").unwrap(), 50000);
        let txns = list_wallet_txns(&db, 111, 10);
        assert!(txns.iter().any(|t| t.kind == "refund" && t.amount_idr == 2000));
    }

    #[test]
    fn with_tx_rolls_back_on_error() {
        let db = test_db();
        user(&db, 111);
        let res: anyhow::Result<()> = with_tx(&db, |conn| {
            conn.execute(
                "UPDATE wallets SET balance_idr = 999 WHERE tg_id = 111",
                [],
            )?;
            Err(anyhow::anyhow!("gagal sengaja"))
        });
        assert!(res.is_err());
        assert_eq!(get_wallet(&db, 111).balance_idr, 0);
    }
}
