//! Order service — port `src/lib/orders.ts`.
//! SATU alur order untuk web & bot Telegram.
//!   awaiting_payment -> paid -> processing -> delivered
//!                                    \-> failed -> (refund manual / coba supplier lain)
//!
//! ATURAN PENTING (dari docs kedua supplier):
//! - Satu order pembeli = SATU idempotency key = ID order internal (order.id).
//!   Key ini dipakai ulang sebagai shopOrderId/clientOrderId (LahaStore) dan
//!   ref_id (Sondev). Retry aman, tidak akan ke-charge 2x.
//! - Jangan pernah buat order supplier baru kalau status masih unknown /
//!   timeout — cek dulu status order lamanya.
//! - Poll status tiap 5–10 detik; berhenti saat success / failed / cancelled.
//!
//! ## Supplier — antarmuka (satu sumber kebenaran)
//! Adapter supplier (Sondev/LahaStore) mengimplementasikan trait
//! [`SupplierAdapter`] dari `crate::lib::suppliers::types`, dan tipe
//! `SupplierBalance`, `SupplierProduct`, `SupplierOrderResult`,
//! `CreateOrderInput`, dan `SheeridJob` di-re-export dari sana (lihat
//! blok "tipe supplier" di bawah) — jangan mendefinisikan ulang.
//! `fulfill_order` generik atas `S: SupplierAdapter`, jadi adapter apa pun
//! bisa dipakai tanpa mengubah kode ini.

use crate::catalog::{get_catalog_product, Fulfillment};
use crate::db::{now_iso, Db};
use crate::lib::notify::{notify_admin, NotifyCtx};
use crate::lib::pricing::{IDR_PER_USD, VND_PER_USD};
use crate::lib::wallet::{base36, refund_wallet_for_order, tg_id_from_buyer_ref};
use rand::Rng;

/// Batas jumlah per order (bot + API tolak 400 kalau lebih).
pub const MAX_QTY: i64 = 30;
/// Satu kode invite Muse maks 30x penebusan.
pub const MAX_CODE_USES: i64 = 30;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StoreOrderStatus {
    AwaitingPayment,
    Paid,
    Processing,
    Delivered,
    Failed,
    Refunded,
}

impl StoreOrderStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            StoreOrderStatus::AwaitingPayment => "awaiting_payment",
            StoreOrderStatus::Paid => "paid",
            StoreOrderStatus::Processing => "processing",
            StoreOrderStatus::Delivered => "delivered",
            StoreOrderStatus::Failed => "failed",
            StoreOrderStatus::Refunded => "refunded",
        }
    }

    pub fn from_str(s: &str) -> StoreOrderStatus {
        match s {
            "paid" => StoreOrderStatus::Paid,
            "processing" => StoreOrderStatus::Processing,
            "delivered" => StoreOrderStatus::Delivered,
            "failed" => StoreOrderStatus::Failed,
            "refunded" => StoreOrderStatus::Refunded,
            _ => StoreOrderStatus::AwaitingPayment,
        }
    }
}

#[derive(Debug, Clone)]
pub struct StoreOrder {
    pub id: String,
    pub catalog_product_id: String,
    pub quantity: i64,
    pub retail_total_idr: i64,
    pub status: StoreOrderStatus,
    pub supplier: String,
    pub supplier_order_id: Option<String>,
    pub delivered_items: Vec<String>,
    pub result_link: Option<String>,
    pub error: Option<String>,
    /// ID Telegram pembeli (bot) atau "web".
    pub buyer_ref: Option<String>,
    /// Khusus Xiaomi: akun pembeli "email|password|2FA" (1 baris per akun).
    pub xiaomi_accounts: Option<Vec<String>>,
    /// Khusus SheerID: URL verifikasi dari pembeli.
    pub sheerid_url: Option<String>,
    /// Target pesanan (Sondev, produk on-demand): kode invite / email target.
    pub target: Option<String>,
    /// Diskon promo yang diterapkan (Rp) — total = harga*qty - discount.
    pub discount_idr: i64,
    /// Kode promo yang dipakai (kalau ada).
    pub promo_code: Option<String>,
    /// Cara bayar: "qris" | "wallet" | "midtrans" | null (belum dibayar).
    pub paid_via: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

// ------------------------------------------------- tipe supplier (satu sumber kebenaran) ---
//
// Tipe + trait supplier didefinisikan SATU kali di
// `crate::lib::suppliers::types` dan dipakai ulang di sini — jangan
// mendefinisikan ulang. Dulu ada duplikat dengan `wholesale_price_idr`
// `Option<i64>` vs `Option<f64>` yang bisa drift; kanoniknya adalah
// `Option<f64>` (angka TS `number` -> `f64` untuk uang/harga).

pub use crate::lib::suppliers::types::{
    CreateOrderInput, SheeridJob, SupplierAdapter, SupplierBalance, SupplierOrderResult,
    SupplierOrderStatus, SupplierProduct,
};

// ---------------------------------------------------------------- helpers ---

fn row_to_order(row: &rusqlite::Row) -> rusqlite::Result<StoreOrder> {
    let delivered: String = row.get(7)?;
    let xiaomi: Option<String> = row.get(11)?;
    Ok(StoreOrder {
        id: row.get(0)?,
        catalog_product_id: row.get(1)?,
        quantity: row.get(2)?,
        retail_total_idr: row.get(3)?,
        status: StoreOrderStatus::from_str(&row.get::<_, String>(4)?),
        supplier: row.get(5)?,
        supplier_order_id: row.get(6)?,
        delivered_items: serde_json::from_str(&delivered).unwrap_or_default(),
        result_link: row.get(8)?,
        error: row.get(9)?,
        buyer_ref: row.get(10)?,
        xiaomi_accounts: xiaomi.and_then(|s| serde_json::from_str(&s).ok()),
        sheerid_url: row.get(12)?,
        target: row.get(13)?,
        discount_idr: row.get(14)?,
        promo_code: row.get(15)?,
        paid_via: row.get(16)?,
        created_at: row.get(17)?,
        updated_at: row.get(18)?,
    })
}

const ORDER_COLS: &str = "id, catalog_product_id, quantity, retail_total_idr, status, supplier, \
    supplier_order_id, delivered_items, result_link, error, buyer_ref, xiaomi_accounts, \
    sheerid_url, target, discount_idr, promo_code, paid_via, created_at, updated_at";

fn rand_suffix() -> String {
    const CHARS: &[u8] = b"0123456789abcdefghijklmnopqrstuvwxyz";
    let mut rng = rand::thread_rng();
    (0..6)
        .map(|_| CHARS[rng.gen_range(0..CHARS.len())] as char)
        .collect()
}

pub fn new_order_id() -> String {
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);
    format!("UD-{}-{}", base36(ts), rand_suffix()).to_uppercase()
}

#[derive(Default)]
struct OrderPatch {
    status: Option<StoreOrderStatus>,
    supplier_order_id: Option<Option<String>>,
    delivered_items: Option<Vec<String>>,
    result_link: Option<Option<String>>,
    error: Option<Option<String>>,
    target: Option<Option<String>>,
    paid_via: Option<Option<String>>,
}

fn update_order(db: &Db, id: &str, patch: OrderPatch) -> anyhow::Result<StoreOrder> {
    let mut sets: Vec<String> = vec!["updated_at = ?".to_string()];
    let mut values: Vec<rusqlite::types::Value> =
        vec![rusqlite::types::Value::Text(now_iso())];
    if let Some(s) = patch.status {
        sets.push("status = ?".to_string());
        values.push(rusqlite::types::Value::Text(s.as_str().to_string()));
    }
    if let Some(v) = patch.supplier_order_id {
        sets.push("supplier_order_id = ?".to_string());
        values.push(v.map_or(rusqlite::types::Value::Null, rusqlite::types::Value::Text));
    }
    if let Some(items) = patch.delivered_items {
        sets.push("delivered_items = ?".to_string());
        values.push(rusqlite::types::Value::Text(
            serde_json::to_string(&items).unwrap_or_else(|_| "[]".to_string()),
        ));
    }
    if let Some(v) = patch.result_link {
        sets.push("result_link = ?".to_string());
        values.push(v.map_or(rusqlite::types::Value::Null, rusqlite::types::Value::Text));
    }
    if let Some(v) = patch.error {
        sets.push("error = ?".to_string());
        values.push(v.map_or(rusqlite::types::Value::Null, rusqlite::types::Value::Text));
    }
    if let Some(v) = patch.target {
        sets.push("target = ?".to_string());
        values.push(v.map_or(rusqlite::types::Value::Null, rusqlite::types::Value::Text));
    }
    if let Some(v) = patch.paid_via {
        sets.push("paid_via = ?".to_string());
        values.push(v.map_or(rusqlite::types::Value::Null, rusqlite::types::Value::Text));
    }
    values.push(rusqlite::types::Value::Text(id.to_string()));
    {
        let conn = db.lock().map_err(|e| anyhow::anyhow!("db lock poisoned: {e}"))?;
        conn.execute(
            &format!("UPDATE store_orders SET {} WHERE id = ?", sets.join(", ")),
            rusqlite::params_from_iter(values.iter().map(|v| v as &dyn rusqlite::types::ToSql)),
        )?;
    }
    get_store_order(db, id).ok_or_else(|| anyhow::anyhow!("Order tidak ditemukan: {id}"))
}

// ------------------------------------------------------------ create/read ---

pub struct CreateStoreOrderInput {
    pub id: Option<String>,
    pub catalog_product_id: String,
    pub quantity: Option<i64>,
    pub buyer_ref: Option<String>,
    pub xiaomi_accounts: Option<Vec<String>>,
    pub sheerid_url: Option<String>,
    pub target: Option<String>,
    pub discount_idr: Option<i64>,
    pub promo_code: Option<String>,
}

pub fn create_store_order(db: &Db, input: CreateStoreOrderInput) -> anyhow::Result<StoreOrder> {
    let product = get_catalog_product(db, &input.catalog_product_id)?;
    let quantity = input.quantity.unwrap_or(1);
    let discount_idr = (input.discount_idr.unwrap_or(0))
        .max(0)
        .min(product.retail_price_idr * quantity);
    let now = now_iso();
    let order = StoreOrder {
        id: input.id.unwrap_or_else(new_order_id),
        catalog_product_id: product.id.to_string(),
        quantity,
        retail_total_idr: (product.retail_price_idr * quantity - discount_idr).max(0),
        status: StoreOrderStatus::AwaitingPayment,
        supplier: product.supplier.to_string(),
        supplier_order_id: None,
        delivered_items: vec![],
        result_link: None,
        error: None,
        buyer_ref: input.buyer_ref,
        xiaomi_accounts: input.xiaomi_accounts,
        sheerid_url: input.sheerid_url,
        target: input.target,
        discount_idr,
        promo_code: input.promo_code,
        paid_via: None,
        created_at: now.clone(),
        updated_at: now,
    };
    let conn = db.lock().map_err(|e| anyhow::anyhow!("db lock poisoned: {e}"))?;
    conn.execute(
        "INSERT INTO store_orders
         (id, catalog_product_id, quantity, retail_total_idr, status, supplier,
          supplier_order_id, delivered_items, result_link, error, buyer_ref,
          xiaomi_accounts, sheerid_url, target, discount_idr, promo_code, paid_via,
          created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19)",
        rusqlite::params![
            order.id,
            order.catalog_product_id,
            order.quantity,
            order.retail_total_idr,
            order.status.as_str(),
            order.supplier,
            order.supplier_order_id,
            serde_json::to_string(&order.delivered_items).unwrap(),
            order.result_link,
            order.error,
            order.buyer_ref,
            order.xiaomi_accounts.as_ref().map(|a| serde_json::to_string(a).unwrap()),
            order.sheerid_url,
            order.target,
            order.discount_idr,
            order.promo_code,
            order.paid_via,
            order.created_at,
            order.updated_at,
        ],
    )?;
    Ok(order)
}

pub fn get_store_order(db: &Db, id: &str) -> Option<StoreOrder> {
    let conn = db.lock().ok()?;
    conn.query_row(
        &format!("SELECT {ORDER_COLS} FROM store_orders WHERE id = ?1"),
        rusqlite::params![id],
        row_to_order,
    )
    .ok()
}

pub fn list_store_orders(db: &Db, limit: i64) -> Vec<StoreOrder> {
    let conn = match db.lock() {
        Ok(g) => g,
        Err(_) => return Vec::new(),
    };
    let mut stmt = match conn.prepare(&format!(
        "SELECT {ORDER_COLS} FROM store_orders ORDER BY created_at DESC LIMIT ?1"
    )) {
        Ok(s) => s,
        Err(_) => return Vec::new(),
    };
    let rows = match stmt.query_map(rusqlite::params![limit], row_to_order) {
        Ok(m) => m.filter_map(|r| r.ok()).collect(),
        Err(_) => Vec::new(),
    };
    rows
}

/// Order milik satu pembeli (bot: buyerRef `tg:<id>`), terbaru dulu.
pub fn list_orders_by_buyer(db: &Db, buyer_ref: &str, limit: i64) -> Vec<StoreOrder> {
    let conn = match db.lock() {
        Ok(g) => g,
        Err(_) => return Vec::new(),
    };
    let mut stmt = match conn.prepare(&format!(
        "SELECT {ORDER_COLS} FROM store_orders WHERE buyer_ref = ?1 ORDER BY created_at DESC LIMIT ?2"
    )) {
        Ok(s) => s,
        Err(_) => return Vec::new(),
    };
    let rows = match stmt.query_map(rusqlite::params![buyer_ref, limit], row_to_order) {
        Ok(m) => m.filter_map(|r| r.ok()).collect(),
        Err(_) => Vec::new(),
    };
    rows
}

/// Statistik pembeli untuk layar /start: jumlah order berbayar + total belanja.
pub fn get_buyer_stats(db: &Db, buyer_ref: &str) -> (i64, i64) {
    let conn = match db.lock() {
        Ok(g) => g,
        Err(_) => return (0, 0),
    };
    conn.query_row(
        "SELECT COUNT(*), COALESCE(SUM(retail_total_idr), 0) FROM store_orders
         WHERE buyer_ref = ?1 AND status IN ('paid', 'processing', 'delivered')",
        rusqlite::params![buyer_ref],
        |row| Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?)),
    )
    .unwrap_or((0, 0))
}

/// Berapa kali sebuah kode invite sudah terpakai (order berbayar).
/// Satu kode Muse maks 30x penebusan — dihitung dari order di bot ini
/// (paid/processing/delivered). Target tersimpan bisa berupa kode polos
/// ("UVI0QD") atau link lengkap (".../invite/UVI0QD") — keduanya dihitung.
/// Kode yang dipakai di luar bot ini (mis. langsung di bot Sondev) tidak
/// terlihat dari sini; penebusan ke-31+ tetap gagal di Sondev lalu refund.
pub fn count_code_uses(db: &Db, code: &str) -> i64 {
    let normalized = code.trim().to_uppercase();
    if normalized.is_empty() {
        return 0;
    }
    // Escape karakter LIKE (kode bisa mengandung "_").
    let like_code: String = normalized.chars().flat_map(|c| match c {
        '\\' => vec!['\\', '\\'],
        '%' => vec!['\\', '%'],
        '_' => vec!['\\', '_'],
        other => vec![other],
    }).collect();
    let conn = match db.lock() {
        Ok(g) => g,
        Err(_) => return 0,
    };
    conn.query_row(
        "SELECT COUNT(*) FROM store_orders
         WHERE (target = ?1 OR target LIKE '%/invite/' || ?2 ESCAPE '\\')
           AND status IN ('paid', 'processing', 'delivered')",
        rusqlite::params![normalized, like_code],
        |row| row.get::<_, i64>(0),
    )
    .unwrap_or(0)
}

/// Statistik toko untuk layar /start ala Lhie: jumlah pengguna terdaftar
/// + jumlah order yang sudah berhasil (delivered). Angka nyata dari DB.
pub fn get_store_stats(db: &Db) -> (i64, i64) {
    let conn = match db.lock() {
        Ok(g) => g,
        Err(_) => return (0, 0),
    };
    let users: i64 = conn
        .query_row("SELECT COUNT(*) FROM users", [], |row| row.get(0))
        .unwrap_or(0);
    let delivered: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM store_orders WHERE status = 'delivered'",
            [],
            |row| row.get(0),
        )
        .unwrap_or(0);
    (users, delivered)
}

// ------------------------------------------------------- status transitions ---

/// Isi/ubah target order (mis. dari bot Telegram setelah pembeli mengirimnya).
/// Dipakai tes + jalur bot lama; jalur Sondev baru menyimpan target lewat
/// `create_store_order` langsung.
/// Dijaga sebagai API publik modul (cerminan TS).
#[allow(dead_code)]
pub fn set_order_target(db: &Db, id: &str, target: &str) -> anyhow::Result<StoreOrder> {
    if get_store_order(db, id).is_none() {
        return Err(anyhow::anyhow!("Order tidak ditemukan: {id}"));
    }
    update_order(
        db,
        id,
        OrderPatch {
            target: Some(Some(target.to_string())),
            ..Default::default()
        },
    )
}

/// Catat cara bayar order (dipanggil sebelum markPaid dari jalur bot / poller).
pub fn set_order_paid_via(db: &Db, id: &str, paid_via: &str) -> anyhow::Result<StoreOrder> {
    update_order(
        db,
        id,
        OrderPatch {
            paid_via: Some(Some(paid_via.to_string())),
            ..Default::default()
        },
    )
}

/// Dipanggil poller QRIS setelah bayar lunas (feed GoBiz cocok).
/// Idempotent + klaim atomik: dua pemanggil bersamaan (poller QRIS +
/// poller Midtrans / webhook) hanya satu yang berhasil mengubah status.
pub fn mark_paid(db: &Db, id: &str) -> anyhow::Result<StoreOrder> {
    let order = get_store_order(db, id).ok_or_else(|| anyhow::anyhow!("Order tidak ditemukan: {id}"))?;
    if order.status != StoreOrderStatus::AwaitingPayment {
        return Ok(order); // idempotent
    }
    let claimed = {
        let conn = db.lock().map_err(|e| anyhow::anyhow!("db lock poisoned: {e}"))?;
        conn.execute(
            "UPDATE store_orders SET status = 'paid', updated_at = ?1 WHERE id = ?2 AND status = 'awaiting_payment'",
            rusqlite::params![now_iso(), id],
        )?
    };
    if claimed == 0 {
        return Ok(get_store_order(db, id).unwrap_or(order));
    }
    Ok(get_store_order(db, id).unwrap_or(order))
}

/// Batalkan order yang QR-nya kedaluwarsa sebelum lunas (dipanggil poller).
/// Hanya dari status awaiting_payment — order yang sudah dibayar tidak
/// disentuh. Status akhir "failed" dengan alasan jelas supaya pembeli
/// tahu harus buat order baru (bukan error supplier).
pub fn cancel_order_for_expired_payment(
    db: &Db,
    id: &str,
    reason: &str,
) -> anyhow::Result<StoreOrder> {
    let order = get_store_order(db, id).ok_or_else(|| anyhow::anyhow!("Order tidak ditemukan: {id}"))?;
    if order.status != StoreOrderStatus::AwaitingPayment {
        return Ok(order); // sudah dibayar — jangan batalkan
    }
    let claimed = {
        let conn = db.lock().map_err(|e| anyhow::anyhow!("db lock poisoned: {e}"))?;
        conn.execute(
            "UPDATE store_orders SET status = 'failed', error = ?1, updated_at = ?2 WHERE id = ?3 AND status = 'awaiting_payment'",
            rusqlite::params![reason, now_iso(), id],
        )?
    };
    if claimed == 0 {
        return Ok(get_store_order(db, id).unwrap_or(order));
    }
    Ok(get_store_order(db, id).unwrap_or(order))
}

/// Dipanggil webhook Sondev saat order selesai (order.completed / DELIVERED).
/// Idempotent: hanya mengubah order yang masih processing/paid.
pub fn mark_order_delivered(
    db: &Db,
    id: &str,
    delivered_items: Vec<String>,
    result_link: Option<String>,
) -> anyhow::Result<StoreOrder> {
    let order = get_store_order(db, id).ok_or_else(|| anyhow::anyhow!("Order tidak ditemukan: {id}"))?;
    if order.status == StoreOrderStatus::Delivered {
        return Ok(order);
    }
    if order.status != StoreOrderStatus::Processing && order.status != StoreOrderStatus::Paid {
        return Ok(order);
    }
    update_order(
        db,
        id,
        OrderPatch {
            status: Some(StoreOrderStatus::Delivered),
            delivered_items: Some(delivered_items),
            result_link: Some(result_link.or(order.result_link)),
            ..Default::default()
        },
    )
}

/// Dipanggil webhook Sondev saat order gagal (order.failed / FAILED / REFUNDED).
/// Idempotent: hanya mengubah order yang masih processing/paid.
pub fn mark_order_failed(db: &Db, id: &str, error: &str) -> anyhow::Result<StoreOrder> {
    let order = get_store_order(db, id).ok_or_else(|| anyhow::anyhow!("Order tidak ditemukan: {id}"))?;
    if order.status == StoreOrderStatus::Failed || order.status == StoreOrderStatus::Delivered {
        return Ok(order);
    }
    if order.status != StoreOrderStatus::Processing && order.status != StoreOrderStatus::Paid {
        return Ok(order);
    }
    update_order(
        db,
        id,
        OrderPatch {
            status: Some(StoreOrderStatus::Failed),
            error: Some(Some(error.to_string())),
            ..Default::default()
        },
    )
}

// ------------------------------------------------------- supplier funds ---

/// Cek apakah saldo supplier cukup untuk meng-cover harga grosir produk x qty.
/// Dipakai SEBELUM pembeli bayar (POST /api/orders) dan di awal fulfillOrder.
/// FAIL OPEN: kalau API supplier tidak bisa dicek (mis. Cloudflare 403 dari
/// VM ini) atau harga grosir tidak diketahui, order tetap jalan seperti dulu
/// — jangan blokir pembeli karena cek yang gagal. Hanya tolak kalau SUDAH
/// PASTI saldo tidak cukup (saldo + harga grosir sama-sama terbaca).
pub async fn check_supplier_funds<S: SupplierAdapter>(
    db: &Db,
    supplier: &S,
    catalog_product_id: &str,
    quantity: i64,
) -> (bool, Option<String>) {
    let product = match get_catalog_product(db, catalog_product_id) {
        Ok(p) => p,
        Err(_) => return (true, None),
    };
    let res: anyhow::Result<(SupplierBalance, Vec<SupplierProduct>)> = async {
        let balance = supplier.get_balance().await?;
        let products = supplier.list_products().await?;
        Ok((balance, products))
    }
    .await;
    let Ok((balance, products)) = res else {
        return (true, None); // cek gagal — fail open
    };
    let Some(wholesale) = products.iter().find(|p| p.code == product.supplier_product_code) else {
        return (true, None); // produk tidak ada di katalog live — jangan blokir
    };
    let unit_idr: Option<f64> = wholesale.wholesale_price_idr.or_else(|| {
        wholesale
            .wholesale_price_usd
            .map(|usd| usd * IDR_PER_USD as f64)
    });
    let Some(unit_idr) = unit_idr else {
        return (true, None); // harga grosir tidak diketahui — jangan blokir
    };
    let vnd = balance
        .raw
        .get("vnd")
        .and_then(|v| v.as_f64())
        .unwrap_or(0.0);
    let funds_idr = balance.idr + balance.usd * IDR_PER_USD as f64 + vnd * IDR_PER_USD as f64 / VND_PER_USD as f64;
    let need_idr = unit_idr * quantity as f64;
    if funds_idr < need_idr {
        return (
            false,
            Some(
                "Produk ini sedang tidak bisa dibeli — stok supplier belum tersedia. Coba lagi nanti."
                    .to_string(),
            ),
        );
    }
    (true, None)
}

// ------------------------------------------------------------- fail/fill ---

/// Tandai order gagal. Refund tergantung cara bayar:
///  - Dompet (paid_via = "wallet") -> saldo dikembalikan ke dompet otomatis,
///    status jadi "refunded".
///  - QRIS (paid_via = "qris") -> TIDAK ADA refund otomatis (QRIS tidak
///    punya API refund). Status tetap "failed" + catatan refund MANUAL +
///    notifikasi ke admin supaya uangnya ditransfer balik manual.
///    Tidak pernah throw — kegagalan refund/notifikasi tidak boleh menutupi
///    status gagal ordernya.
async fn fail_order(
    db: &Db,
    notify: Option<&NotifyCtx>,
    order_id: &str,
    error: &str,
    delivered_items: Vec<String>,
    result_link: Option<String>,
) -> StoreOrder {
    let failed = match update_order(
        db,
        order_id,
        OrderPatch {
            status: Some(StoreOrderStatus::Failed),
            delivered_items: Some(delivered_items),
            result_link: Some(result_link),
            error: Some(Some(error.to_string())),
            ..Default::default()
        },
    ) {
        Ok(o) => o,
        Err(_) => return get_store_order(db, order_id).unwrap_or_else(|| StoreOrder {
            id: order_id.to_string(),
            catalog_product_id: String::new(),
            quantity: 0,
            retail_total_idr: 0,
            status: StoreOrderStatus::Failed,
            supplier: String::new(),
            supplier_order_id: None,
            delivered_items: vec![],
            result_link: None,
            error: Some(error.to_string()),
            buyer_ref: None,
            xiaomi_accounts: None,
            sheerid_url: None,
            target: None,
            discount_idr: 0,
            promo_code: None,
            paid_via: None,
            created_at: now_iso(),
            updated_at: now_iso(),
        }),
    };
    // Order dompet -> refund ke dompet (bukan QRIS).
    if failed.paid_via.as_deref() == Some("wallet") {
        let tg_id = tg_id_from_buyer_ref(failed.buyer_ref.as_deref());
        if let Some(tg_id) = tg_id {
            if failed.retail_total_idr > 0 {
                match refund_wallet_for_order(db, tg_id, failed.retail_total_idr, &failed.id) {
                    Ok(_) => {
                        return update_order(
                            db,
                            &failed.id,
                            OrderPatch {
                                status: Some(StoreOrderStatus::Refunded),
                                error: Some(Some(format!(
                                    "{error} — saldo sudah dikembalikan ke dompet kamu."
                                ))),
                                ..Default::default()
                            },
                        )
                        .unwrap_or(failed);
                    }
                    Err(e) => {
                        tracing::error!("[orders] refund dompet {} gagal: {e}", failed.id);
                    }
                }
            }
        }
        return update_order(
            db,
            &failed.id,
            OrderPatch {
                error: Some(Some(format!(
                    "{error} — refund ke dompet gagal, hubungi admin."
                ))),
                ..Default::default()
            },
        )
        .unwrap_or(failed);
    }
    // Order QRIS -> refund MANUAL oleh admin (transfer balik ke pembeli).
    if let Some(ctx) = notify {
        let total = crate::i18n::rupiah(failed.retail_total_idr);
        notify_admin(
            ctx,
            &format!(
                "⚠️ <b>Refund manual dibutuhkan</b>\n\nOrder {} ({} × {}, {total}) gagal setelah dibayar via QRIS.\nPembeli: {}\nAlasan: {error}\n\nQRIS tidak punya refund otomatis — transfer balik {total} ke pembeli secara manual, lalu kabari pembeli.",
                failed.id,
                failed.catalog_product_id,
                failed.quantity,
                failed.buyer_ref.as_deref().unwrap_or("web"),
            ),
        )
        .await;
    }
    update_order(
        db,
        &failed.id,
        OrderPatch {
            error: Some(Some(format!(
                "{error} — refund manual: admin akan transfer balik ke kamu, hubungi support kalau belum masuk."
            ))),
            ..Default::default()
        },
    )
    .unwrap_or(failed)
}

/// Jalankan fulfillment ke supplier. Hanya untuk order yang sudah "paid".
/// Polling bisa makan waktu sampai ~5 menit — di produksi jalankan dari
/// worker/queue atau tanpa await dari webhook, JANGAN tahan request HTTP.
/// `notify`: konteks notifikasi admin (None = lewati notifikasi refund manual).
pub async fn fulfill_order<S: SupplierAdapter>(
    db: &Db,
    notify: Option<&NotifyCtx>,
    supplier: &S,
    order_id: &str,
) -> anyhow::Result<StoreOrder> {
    let order = get_store_order(db, order_id).ok_or_else(|| anyhow::anyhow!("Order tidak ditemukan: {order_id}"))?;
    // Sudah selesai (delivered/failed): jangan proses ulang — kembalikan saja.
    if order.status == StoreOrderStatus::Delivered || order.status == StoreOrderStatus::Failed {
        return Ok(order);
    }
    if order.status != StoreOrderStatus::Paid && order.status != StoreOrderStatus::Processing {
        return Err(anyhow::anyhow!(
            "Order {order_id} belum dibayar (status: {})",
            order.status.as_str()
        ));
    }
    let product = get_catalog_product(db, &order.catalog_product_id)?;
    // Klaim atomik paid -> processing: kalau dua fulfill jalan bersamaan
    // (webhook + poller, atau dua jalur bayar), hanya satu yang mengeksekusi
    // order ke supplier — yang lain kembalikan status terbaru tanpa belanja
    // dua kali. Ini yang mencegah double charge Sondev.
    let claimed = {
        let conn = db.lock().map_err(|e| anyhow::anyhow!("db lock poisoned: {e}"))?;
        conn.execute(
            "UPDATE store_orders SET status = 'processing', updated_at = ?1 WHERE id = ?2 AND status = 'paid'",
            rusqlite::params![now_iso(), order.id],
        )?
    };
    if claimed == 0 {
        return Ok(get_store_order(db, &order.id).unwrap_or(order));
    }

    let result: anyhow::Result<Vec<SupplierOrderResult>> = async {
        // 1) Cek saldo supplier sebelum belanja. Kalau SUDAH PASTI tidak cukup,
        //    lempar error -> order gagal -> refund otomatis (lihat fail_order).
        //    Kalau ceknya gagal (API tidak bisa dihubungi), fail open: lanjut.
        let (ok, reason) = check_supplier_funds(db, supplier, product.id, order.quantity).await;
        if !ok {
            return Err(anyhow::anyhow!(
                reason.unwrap_or_else(|| "Saldo supplier tidak cukup".to_string())
            ));
        }

        // 2) Buat order ke supplier — idempotency key = ID order internal.
        if product.fulfillment == Fulfillment::LahastoreXiaomi3 {
            let accounts = order.xiaomi_accounts.clone().unwrap_or_default();
            if accounts.is_empty() {
                return Err(anyhow::anyhow!(
                    "Order Xiaomi butuh akun pembeli (email|password|2FA)"
                ));
            }
            let mut results = supplier.create_xiaomi_order(&order.id, &accounts).await?;
            if let Some(first) = results.first() {
                update_order(
                    db,
                    &order.id,
                    OrderPatch {
                        supplier_order_id: Some(Some(first.supplier_order_id.clone())),
                        ..Default::default()
                    },
                )?;
            }
            // Poll tiap order dalam batch (1 akun = 1 order supplier).
            let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(5 * 60);
            while results.iter().any(|r| !r.status.is_terminal())
                && tokio::time::Instant::now() < deadline
            {
                tokio::time::sleep(std::time::Duration::from_secs(8)).await;
                let mut next = Vec::with_capacity(results.len());
                for r in &results {
                    let id: i64 = r.supplier_order_id.parse().unwrap_or(0);
                    next.push(supplier.get_xiaomi_order(id).await?);
                }
                results = next;
            }
            Ok(results)
        } else if product.fulfillment == Fulfillment::LahastoreSheerid {
            let sheerid_url = order.sheerid_url.clone().unwrap_or_default();
            if sheerid_url.is_empty() {
                return Err(anyhow::anyhow!("Order SheerID butuh URL verifikasi dari pembeli"));
            }
            let job = supplier
                .create_sheerid_verification(&sheerid_url, &order.id)
                .await?;
            update_order(
                db,
                &order.id,
                OrderPatch {
                    supplier_order_id: Some(Some(job.job_id.clone())),
                    ..Default::default()
                },
            )?;
            let mut current = SheeridJob {
                job_id: job.job_id.clone(),
                status: job.status.clone(),
                result_link: None,
            };
            let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(5 * 60);
            while !matches!(current.status.as_str(), "success" | "failed" | "cancelled")
                && tokio::time::Instant::now() < deadline
            {
                tokio::time::sleep(std::time::Duration::from_secs(8)).await;
                current = supplier.get_sheerid_job(&job.job_id).await?;
            }
            Ok(vec![SupplierOrderResult {
                supplier_order_id: job.job_id,
                status: match current.status.as_str() {
                    "success" => SupplierOrderStatus::Success,
                    "failed" => SupplierOrderStatus::Failed,
                    _ => SupplierOrderStatus::Unknown,
                },
                delivered_items: vec![],
                result_link: current.result_link,
                error_code: None,
                error_message: None,
                raw: serde_json::Value::Null,
            }])
        } else {
            let created = supplier
                .create_order(CreateOrderInput {
                    idempotency_key: order.id.clone(),
                    supplier_product_code: product.supplier_product_code.to_string(),
                    quantity: order.quantity,
                    // Sondev: target WAJIB untuk produk on-demand (docs v1.2).
                    target: order.target.clone(),
                })
                .await?;
            update_order(
                db,
                &order.id,
                OrderPatch {
                    supplier_order_id: Some(Some(created.supplier_order_id.clone())),
                    ..Default::default()
                },
            )?;
            let mut current = created;
            // Polling ladder Sondev (docs v1.2): 5s → 10s → 20s → 30s, berhenti 10 menit.
            let ladder = [5u64, 10, 20, 30];
            let mut step = 0usize;
            let poll_deadline =
                tokio::time::Instant::now() + std::time::Duration::from_secs(10 * 60);
            while !current.status.is_terminal() && tokio::time::Instant::now() < poll_deadline {
                tokio::time::sleep(std::time::Duration::from_secs(
                    ladder[step.min(ladder.len() - 1)],
                ))
                .await;
                step += 1;
                current = supplier.get_order(&current.supplier_order_id).await?;
            }
            Ok(vec![current])
        }
    }
    .await;

    // 3) Simpan hasil & tentukan status akhir.
    match result {
        Ok(results) => {
            let delivered_items: Vec<String> = results
                .iter()
                .flat_map(|r| r.delivered_items.clone())
                .collect();
            let result_link = results.iter().filter_map(|r| r.result_link.clone()).next();
            let failed = results.iter().find(|r| {
                r.status == SupplierOrderStatus::Failed
                    || r.status == SupplierOrderStatus::Cancelled
            });
            let all_success = !results.is_empty()
                && results.iter().all(|r| r.status == SupplierOrderStatus::Success);
            if all_success {
                return update_order(
                    db,
                    &order.id,
                    OrderPatch {
                        status: Some(StoreOrderStatus::Delivered),
                        delivered_items: Some(delivered_items),
                        result_link: Some(result_link),
                        ..Default::default()
                    },
                );
            }
            if let Some(f) = failed {
                let msg = f
                    .error_message
                    .clone()
                    .or_else(|| f.error_code.clone())
                    .unwrap_or_else(|| "Supplier gagal memproses order".to_string());
                return Ok(fail_order(db, notify, &order.id, &msg, delivered_items, result_link).await);
            }
            Ok(fail_order(
                db,
                notify,
                &order.id,
                "Timeout menunggu supplier — admin akan memproses refund manual, hubungi support kalau belum ada kabar.",
                delivered_items,
                result_link,
            )
            .await)
        }
        Err(e) => Ok(fail_order(db, notify, &order.id, &e.to_string(), vec![], None).await),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    fn test_db() -> Db {
        db::open(":memory:").expect("db tes")
    }

    fn order_input(buyer: &str, target: &str) -> CreateStoreOrderInput {
        CreateStoreOrderInput {
            id: None,
            catalog_product_id: "muse-token-1b".to_string(),
            quantity: Some(1),
            buyer_ref: Some(buyer.to_string()),
            xiaomi_accounts: None,
            sheerid_url: None,
            target: Some(target.to_string()),
            discount_idr: None,
            promo_code: None,
        }
    }

    fn seed_paid_order(db: &Db, buyer: &str, target: &str) -> StoreOrder {
        let o = create_store_order(db, order_input(buyer, target)).unwrap();
        mark_paid(db, &o.id).unwrap()
    }

    /// Supplier palsu: tanpa HTTP, menghitung berapa kali create_order
    /// dipanggil (S7b: fulfill ganda tidak boleh belanja 2x).
    struct FakeSupplier {
        balance_idr: f64,
        balance_err: bool,
        wholesale_idr: Option<f64>,
        fail: bool,
        calls: Arc<AtomicUsize>,
    }

    impl FakeSupplier {
        fn ok() -> (Self, Arc<AtomicUsize>) {
            let calls = Arc::new(AtomicUsize::new(0));
            (
                Self {
                    balance_idr: 100_000.0,
                    balance_err: false,
                    wholesale_idr: Some(800.0),
                    fail: false,
                    calls: calls.clone(),
                },
                calls,
            )
        }
        fn failing() -> Self {
            Self {
                balance_idr: 100_000.0,
                balance_err: false,
                wholesale_idr: Some(800.0),
                fail: true,
                calls: Arc::new(AtomicUsize::new(0)),
            }
        }
    }

    impl SupplierAdapter for FakeSupplier {
        fn id(&self) -> crate::lib::suppliers::SupplierId {
            crate::lib::suppliers::SupplierId::Sondev
        }

        async fn get_balance(&self) -> anyhow::Result<SupplierBalance> {
            if self.balance_err {
                return Err(anyhow::anyhow!("API supplier down"));
            }
            Ok(SupplierBalance {
                idr: self.balance_idr,
                usd: 0.0,
                credit: 0.0,
                raw: serde_json::Value::Null,
            })
        }

        async fn list_products(&self) -> anyhow::Result<Vec<SupplierProduct>> {
            if self.balance_err {
                return Err(anyhow::anyhow!("API supplier down"));
            }
            Ok(vec![SupplierProduct {
                code: "muse_token".to_string(),
                name: "Muse Token".to_string(),
                wholesale_price_idr: self.wholesale_idr,
                wholesale_price_usd: None,
                stock: Some(30),
                min_qty: Some(1),
                max_qty: Some(30),
            }])
        }

        async fn get_stock<'a>(
            &'a self,
            _supplier_product_code: &'a str,
        ) -> anyhow::Result<Option<i64>> {
            Ok(Some(30))
        }

        async fn create_order(
            &self,
            input: CreateOrderInput,
        ) -> anyhow::Result<SupplierOrderResult> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            if self.fail {
                return Ok(SupplierOrderResult {
                    supplier_order_id: "SONDEV-FAIL".to_string(),
                    status: SupplierOrderStatus::Failed,
                    delivered_items: vec![],
                    result_link: None,
                    error_code: Some("SUPPLIER_FAIL".to_string()),
                    error_message: Some("Supplier gagal memproses order".to_string()),
                    raw: serde_json::Value::Null,
                });
            }
            Ok(SupplierOrderResult {
                supplier_order_id: "SONDEV-1".to_string(),
                status: SupplierOrderStatus::Success,
                delivered_items: vec!["MUSE-TOKEN-TEST-1B".to_string()],
                result_link: Some("https://muse.ai/invite/TESTCODE1B".to_string()),
                error_code: None,
                error_message: None,
                raw: serde_json::json!({"ref_id": input.idempotency_key}),
            })
        }

        async fn get_order(
            &self,
            _supplier_order_id: &str,
        ) -> anyhow::Result<SupplierOrderResult> {
            unreachable!("fake sukses langsung terminal — tidak dipoll")
        }
    }

    #[test]
    fn create_order_totals_and_discount_clamp() {
        let db = test_db();
        let o = create_store_order(
            &db,
            CreateStoreOrderInput {
                quantity: Some(5),
                discount_idr: Some(2000),
                promo_code: Some("TEST10".to_string()),
                ..order_input("tg:111", "UVI0QD")
            },
        )
        .unwrap();
        assert!(o.id.starts_with("UD-"));
        assert_eq!(o.retail_total_idr, 5 * 2000 - 2000);
        assert_eq!(o.discount_idr, 2000);
        assert_eq!(o.promo_code.as_deref(), Some("TEST10"));
        assert_eq!(o.status, StoreOrderStatus::AwaitingPayment);
        // Diskon lebih besar dari total -> total 0 (bukan negatif).
        let free = create_store_order(
            &db,
            CreateStoreOrderInput {
                discount_idr: Some(99999),
                ..order_input("tg:111", "UVI0QD")
            },
        )
        .unwrap();
        assert_eq!(free.retail_total_idr, 0);
        // Produk tidak ada -> error.
        assert!(create_store_order(
            &db,
            CreateStoreOrderInput {
                catalog_product_id: "tidak-ada".to_string(),
                ..order_input("tg:111", "UVI0QD")
            },
        )
        .is_err());
    }

    #[test]
    fn new_order_id_format() {
        let id = new_order_id();
        assert!(id.starts_with("UD-"));
        assert!(id.len() > 10);
        assert_ne!(new_order_id(), id);
    }

    #[test]
    fn get_and_list_orders() {
        let db = test_db();
        assert!(get_store_order(&db, "UD-TIDAKADA").is_none());
        let o = create_store_order(&db, order_input("tg:111", "UVI0QD")).unwrap();
        assert_eq!(get_store_order(&db, &o.id).unwrap().id, o.id);
        assert_eq!(list_orders_by_buyer(&db, "tg:111", 10).len(), 1);
        assert!(list_orders_by_buyer(&db, "tg:999", 10).is_empty());
        assert_eq!(list_store_orders(&db, 10).len(), 1);
        // Statistik pembeli + toko (S1/S7).
        assert_eq!(get_buyer_stats(&db, "tg:111"), (0, 0));
        mark_paid(&db, &o.id).unwrap();
        assert_eq!(get_buyer_stats(&db, "tg:111"), (1, 2000));
        assert_eq!(get_store_stats(&db), (0, 0));
    }

    #[test]
    fn count_code_uses_plain_and_link() {
        // S4b: kode polos dan link lengkap dihitung sama.
        let db = test_db();
        assert_eq!(count_code_uses(&db, "UVI0QD"), 0);
        assert_eq!(count_code_uses(&db, ""), 0);
        seed_paid_order(&db, "tg:111", "UVI0QD");
        seed_paid_order(&db, "tg:222", "https://muse.ai/invite/UVI0QD");
        // Order awaiting_payment TIDAK dihitung.
        create_store_order(&db, order_input("tg:333", "UVI0QD")).unwrap();
        assert_eq!(count_code_uses(&db, "UVI0QD"), 2);
        assert_eq!(count_code_uses(&db, "uvi0qd"), 2);
        assert_eq!(count_code_uses(&db, "LAIN"), 0);
    }

    #[test]
    fn count_code_uses_escapes_like_wildcards() {
        // Kode mengandung "_" harus dihitung harfiah, bukan sebagai wildcard
        // LIKE (kalau tidak di-escape, "AB_CD" akan cocok dengan "ABXCD").
        let db = test_db();
        seed_paid_order(&db, "tg:111", "AB_CD");
        seed_paid_order(&db, "tg:222", "ABXCD");
        assert_eq!(count_code_uses(&db, "AB_CD"), 1);
        assert_eq!(count_code_uses(&db, "ABXCD"), 1);
    }

    #[test]
    fn count_code_uses_reaches_30() {
        // S4b: seed 30 order berbayar -> kode habis.
        let db = test_db();
        for _ in 0..30 {
            seed_paid_order(&db, "tg:999", "CAP30X");
        }
        assert_eq!(count_code_uses(&db, "CAP30X"), 30);
        assert!(count_code_uses(&db, "CAP30X") >= MAX_CODE_USES);
        assert_eq!(MAX_CODE_USES, 30);
        assert_eq!(MAX_QTY, 30);
    }

    #[test]
    fn mark_paid_is_idempotent() {
        let db = test_db();
        let o = create_store_order(&db, order_input("tg:111", "UVI0QD")).unwrap();
        let first = mark_paid(&db, &o.id).unwrap();
        assert_eq!(first.status, StoreOrderStatus::Paid);
        let second = mark_paid(&db, &o.id).unwrap();
        assert_eq!(second.status, StoreOrderStatus::Paid);
        assert!(mark_paid(&db, "UD-TIDAKADA").is_err());
    }

    #[test]
    fn mark_paid_concurrent_double_claim() {
        // S7b: dua markPaid bersamaan -> keduanya paid, tanpa throw.
        let db = test_db();
        let o = create_store_order(&db, order_input("tg:111", "UVI0QD")).unwrap();
        let id = o.id.clone();
        let db2 = db.clone();
        let id2 = id.clone();
        let h = std::thread::spawn(move || mark_paid(&db2, &id2));
        let b = mark_paid(&db, &id).unwrap();
        let a = h.join().unwrap().unwrap();
        assert_eq!(a.status, StoreOrderStatus::Paid);
        assert_eq!(b.status, StoreOrderStatus::Paid);
        assert_eq!(
            get_store_order(&db, &o.id).unwrap().status,
            StoreOrderStatus::Paid
        );
    }

    #[test]
    fn cancel_expired_only_from_awaiting_payment() {
        // S30: QR kedaluwarsa -> order dibatalkan; yang sudah dibayar aman.
        let db = test_db();
        let o = create_store_order(&db, order_input("tg:111", "UVI0QD")).unwrap();
        let cancelled = cancel_order_for_expired_payment(
            &db,
            &o.id,
            "Pembayaran QRIS kedaluwarsa (5 menit) sebelum lunas",
        )
        .unwrap();
        assert_eq!(cancelled.status, StoreOrderStatus::Failed);
        assert!(cancelled.error.as_deref().unwrap().contains("kedaluwarsa"));
        // Panggil lagi: idempotent.
        let again = cancel_order_for_expired_payment(&db, &o.id, "x").unwrap();
        assert_eq!(again.status, StoreOrderStatus::Failed);
        // Order yang sudah paid tidak dibatalkan.
        let paid = seed_paid_order(&db, "tg:111", "UVI0QD");
        let kept = cancel_order_for_expired_payment(&db, &paid.id, "x").unwrap();
        assert_eq!(kept.status, StoreOrderStatus::Paid);
    }

    #[test]
    fn mark_delivered_and_failed_quirks() {
        // markOrderFailed TS: hanya dari processing/paid; dari
        // awaiting_payment = no-op. markOrderDelivered: sama.
        let db = test_db();
        let awaiting = create_store_order(&db, order_input("tg:111", "UVI0QD")).unwrap();
        assert_eq!(
            mark_order_failed(&db, &awaiting.id, "x").unwrap().status,
            StoreOrderStatus::AwaitingPayment
        );
        assert_eq!(
            mark_order_delivered(&db, &awaiting.id, vec![], None)
                .unwrap()
                .status,
            StoreOrderStatus::AwaitingPayment
        );
        let paid = seed_paid_order(&db, "tg:111", "UVI0QD");
        let delivered =
            mark_order_delivered(&db, &paid.id, vec!["TOKEN-1".to_string()], None).unwrap();
        assert_eq!(delivered.status, StoreOrderStatus::Delivered);
        assert_eq!(delivered.delivered_items, vec!["TOKEN-1".to_string()]);
        // Sudah delivered: idempotent, tidak berubah.
        let again = mark_order_delivered(&db, &paid.id, vec!["LAIN".to_string()], None).unwrap();
        assert_eq!(again.status, StoreOrderStatus::Delivered);
        assert_eq!(again.delivered_items, vec!["TOKEN-1".to_string()]);
        assert_eq!(
            mark_order_failed(&db, &paid.id, "x").unwrap().status,
            StoreOrderStatus::Delivered
        );
        // Dari paid -> failed.
        let paid2 = seed_paid_order(&db, "tg:111", "UVI0QD");
        let failed = mark_order_failed(&db, &paid2.id, "Supplier gagal").unwrap();
        assert_eq!(failed.status, StoreOrderStatus::Failed);
        assert_eq!(failed.error.as_deref(), Some("Supplier gagal"));
        assert_eq!(
            mark_order_failed(&db, &paid2.id, "y").unwrap().status,
            StoreOrderStatus::Failed
        );
    }

    #[test]
    fn set_target_and_paid_via() {
        let db = test_db();
        let o = create_store_order(&db, order_input("tg:111", "UVI0QD")).unwrap();
        let with_target = set_order_target(&db, &o.id, "https://muse.ai/invite/ABC").unwrap();
        assert_eq!(
            with_target.target.as_deref(),
            Some("https://muse.ai/invite/ABC")
        );
        let via = set_order_paid_via(&db, &o.id, "wallet").unwrap();
        assert_eq!(via.paid_via.as_deref(), Some("wallet"));
        assert!(set_order_target(&db, "UD-TIDAKADA", "x").is_err());
    }

    #[tokio::test]
    async fn fulfill_success_delivers() {
        // S7: bayar saldo -> delivered, item + link tersimpan.
        let db = test_db();
        let (supplier, calls) = FakeSupplier::ok();
        let o = seed_paid_order(&db, "tg:111", "UVI0QD");
        let done = fulfill_order(&db, None, &supplier, &o.id).await.unwrap();
        assert_eq!(done.status, StoreOrderStatus::Delivered);
        assert_eq!(done.delivered_items, vec!["MUSE-TOKEN-TEST-1B".to_string()]);
        assert_eq!(
            done.result_link.as_deref(),
            Some("https://muse.ai/invite/TESTCODE1B")
        );
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn fulfill_twice_calls_supplier_once() {
        // S7b: fulfill pada order yang sudah processing tidak belanja lagi.
        let db = test_db();
        let (supplier, calls) = FakeSupplier::ok();
        let o = seed_paid_order(&db, "tg:111", "UVI0QD");
        {
            let conn = db.lock().unwrap();
            conn.execute(
                "UPDATE store_orders SET status = 'processing' WHERE id = ?1",
                rusqlite::params![o.id],
            )
            .unwrap();
        }
        let again = fulfill_order(&db, None, &supplier, &o.id).await.unwrap();
        assert_eq!(again.status, StoreOrderStatus::Processing);
        assert_eq!(calls.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn fulfill_concurrent_calls_supplier_once() {
        // Dua fulfill bersamaan pada satu order paid: hanya satu yang
        // mengeksekusi (klaim atomik paid -> processing).
        let db = test_db();
        let (supplier, calls) = FakeSupplier::ok();
        let o = seed_paid_order(&db, "tg:111", "UVI0QD");
        let (r1, r2) = tokio::join!(
            fulfill_order(&db, None, &supplier, &o.id),
            fulfill_order(&db, None, &supplier, &o.id),
        );
        assert!(r1.is_ok() && r2.is_ok());
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert_eq!(
            get_store_order(&db, &o.id).unwrap().status,
            StoreOrderStatus::Delivered
        );
    }

    #[tokio::test]
    async fn fulfill_fails_on_unpaid_order() {
        let db = test_db();
        let (supplier, _) = FakeSupplier::ok();
        let o = create_store_order(&db, order_input("tg:111", "UVI0QD")).unwrap();
        assert!(fulfill_order(&db, None, &supplier, &o.id).await.is_err());
        assert!(fulfill_order(&db, None, &supplier, "UD-TIDAKADA")
            .await
            .is_err());
    }

    #[tokio::test]
    async fn fulfill_failure_wallet_refunds() {
        // S27a: bayar saldo -> gagal -> refunded + saldo kembali utuh.
        let db = test_db();
        let supplier = FakeSupplier::failing();
        crate::lib::wallet::ensure_user(
            &db,
            crate::lib::wallet::EnsureUserInput {
                tg_id: 111,
                first_name: Some("Budi".to_string()),
                username: Some("budi".to_string()),
                referrer_tg_id: None,
            },
        );
        crate::lib::wallet::credit_wallet(&db, 111, 50000, "topup", Some("TEST-S27"), None)
            .unwrap();
        let bal_before = crate::lib::wallet::get_wallet(&db, 111).balance_idr;
        let o = create_store_order(
            &db,
            CreateStoreOrderInput {
                id: Some("UD-FAIL-1".to_string()),
                ..order_input("tg:111", "UVI0QD")
            },
        )
        .unwrap();
        crate::lib::wallet::debit_wallet(&db, 111, 2000, "purchase", Some(&o.id), None)
            .unwrap();
        set_order_paid_via(&db, &o.id, "wallet").unwrap();
        mark_paid(&db, &o.id).unwrap();
        let done = fulfill_order(&db, None, &supplier, &o.id).await.unwrap();
        assert_eq!(done.status, StoreOrderStatus::Refunded);
        assert_eq!(
            crate::lib::wallet::get_wallet(&db, 111).balance_idr,
            bal_before
        );
    }

    #[tokio::test]
    async fn fulfill_failure_qris_is_manual_refund() {
        // S27b: bayar QRIS -> TIDAK ADA refund otomatis: failed + catatan
        // refund manual.
        let db = test_db();
        let supplier = FakeSupplier::failing();
        let o = create_store_order(
            &db,
            CreateStoreOrderInput {
                id: Some("UD-FAIL-2".to_string()),
                ..order_input("tg:111", "UVI0QD")
            },
        )
        .unwrap();
        set_order_paid_via(&db, &o.id, "qris").unwrap();
        mark_paid(&db, &o.id).unwrap();
        let done = fulfill_order(&db, None, &supplier, &o.id).await.unwrap();
        assert_eq!(done.status, StoreOrderStatus::Failed);
        assert!(done.error.as_deref().unwrap().contains("manual"));
    }

    #[tokio::test]
    async fn check_supplier_funds_insufficient_and_fail_open() {
        // S28: saldo supplier kurang -> tolak; API error -> fail open.
        let db = test_db();
        let (mut supplier, _) = FakeSupplier::ok();
        supplier.balance_idr = 500.0; // < grosir 800
        let (ok, reason) = check_supplier_funds(&db, &supplier, "muse-token-1b", 1).await;
        assert!(!ok);
        assert!(reason.is_some());
        // Saldo cukup -> lolos.
        supplier.balance_idr = 100_000.0;
        let (ok2, _) = check_supplier_funds(&db, &supplier, "muse-token-1b", 1).await;
        assert!(ok2);
        // API supplier down -> fail open (jangan blokir pembeli).
        supplier.balance_err = true;
        let (ok3, _) = check_supplier_funds(&db, &supplier, "muse-token-1b", 1).await;
        assert!(ok3);
        // Harga grosir tidak diketahui -> fail open.
        let (mut supplier2, _) = FakeSupplier::ok();
        supplier2.wholesale_idr = None;
        let (ok4, _) = check_supplier_funds(&db, &supplier2, "muse-token-1b", 1).await;
        assert!(ok4);
    }
}
