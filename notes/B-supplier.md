# Kelompok B — Supplier (Sondev + LahaStore)

Status: **SELESAI** (semua modul port penuh dari TS, `cargo check` lolos — lihat bawah).
Tanggal: 2026-11-21. Sumber kebenaran: `~/workspace/reseller-web/src/lib/suppliers/*.ts`
(commit `a5ed97a`) + `~/workspace/user/files/SONDEV_RESELLER_API_EN.md` (docs v1.2).

## Modul selesai

| File | Isi |
|---|---|
| `src/lib/suppliers/types.rs` | `SupplierId`, `SupplierBalance`, `SupplierProduct`, `SupplierOrderStatus`, `SupplierOrderResult`, `CreateOrderInput`, trait `SupplierAdapter` + helper baca JSON `json_str/json_f64/json_i64/json_bool` (dipakai adapter). |
| `src/lib/suppliers/sondev.rs` | `SondevAdapter` lengkap (lihat endpoint di bawah) + `sign()` HMAC-SHA256 + `percent_encode` + konstanta `MUSE_TOKEN_WHOLESALE_IDR_PER_B = 800.0`. |
| `src/lib/suppliers/lahastore.rs` | `LahaStoreAdapter` lengkap + jalur Gemini (Xiaomi/SheerID) + helper konversi `usd_to_idr`/`vnd_to_idr`. |
| `src/lib/suppliers/mod.rs` | Registry: `get_supplier(id, &Config)`, `all_suppliers(&Config)`, `SUPPLIER_IDS`, enum dispatch `AnySupplier` (lihat "Keputusan"). |
| `src/config.rs` | **DIUBAH (satu-satunya perubahan di luar kelompok B):** tambah field `sondev_base_url` (`SONDEV_BASE_URL`, default `https://api.sondevs.tech/api/v1`) dan `lahastore_base_url` (`LAHASTORE_BASE_URL`, default `https://lahastore.up.railway.app/api/v1`). Alasan: TS membaca kedua env ini di `config.ts`; kontrak mengharuskan nama env sama. Tanpa ini adapter tidak bisa tahu base URL. |

Tidak disentuh: `main.rs`, `Cargo.toml`, `~/workspace/reseller-web`, EC2, GitHub.
Tidak ada secret ditulis ke file/repo/laporan mana pun; kunci hanya dibaca dari env via `Config`.

## Endpoint yang di-port

### Sondev (`SondevAdapter`, auth X-API-KEY + X-TIMESTAMP + X-SIGNATURE)
- `GET /profile` → `get_balance()` (balance_idr / balance_usdt flat; USDT di kolom `usd`)
- `GET /products` → `list_products()` (cache 120 detik, seperti TS)
- `GET /stock?code=` → `get_stock()` (prioritas `sellable_quantity` → `available_now` → `available_stock` angka; `null` = tidak diketahui, BUKAN 0)
- `POST /orders/create` → `create_order()` (`ref_id` = internal order ID untuk idempotensi; `target` = kode invite polos; reuse ref_id → `ALREADY_EXISTS` dibaca dari `delivery_status`)
- `GET /orders/{ref_id}` → `get_order()`
- Tambahan dari TS: `get_public_status()` (`GET /status/public`, tanpa auth), `list_orders()`, `get_logs()`, `test_webhook()`, `get_dead_letters()`, `replay_webhooks()`
- Retry: 4 percobaan, signature+timestamp BARU tiap percobaan, backoff 2/4/8/16 dtk + jitter ±30% (maks 30 dtk), hormati `Retry-After` (header lalu body). Retryable: 5xx, `REPLAY_DETECTED`, 429 kecuali `DAILY_LIMIT`. Tidak retry: 400/401/402/403, 409 `SOLD_OUT`, `DAILY_LIMIT`.

### LahaStore (`LahaStoreAdapter`, auth X-API-Key)
- `GET /balance` → `get_balance()` (wallet vnd/usd/credit; `idr: 0`, VND dikonversi di pricing; `raw` menyertakan `vnd`)
- `GET /products?page=1&limit=50` → `list_products()` (harga USD di `priceUsd`, `wholesale_price_idr: None`)
- `get_stock()` → via `list_products()` (LahaStore tidak punya endpoint `/stock`)
- `POST /orders` → `create_order()` (`productId`, `qty`, `currency: "vnd"`, `shopOrderId` = idempotency key)
- `GET /orders/{id}` → `get_order()`
- Gemini (jalur beda, dipakai kalau `fulfillment = "xiaomi3"` / `"sheerid"`): `create_xiaomi_order()` (`POST /xiaomi3/orders`, 1 akun = 1 order, `duplicated` kalau `clientOrderId` sudah dipakai), `get_xiaomi_order(id)`, `create_sheerid_verification()` (`POST /sheerid/verify`), `get_sheerid_job(job_id)` (dipakai polling fulfillOrder).

## Keputusan perilaku (bila TS ambigu / perlu penyesuaian Rust)

1. **Path signature Sondev = path LENGKAP termasuk `/api/v1`, tanpa query string.**
   `base_path` dihitung dari base URL (`https://api.sondevs.tech/api/v1` → `/api/v1`) dan
   digabung dengan path relatif di tiap request. Ada unit test yang memastikan signature
   path lengkap ≠ path relatif (bug lama commit `bbc7b1f` tidak boleh terulang).
2. **Trait `SupplierAdapter` bukan `dyn`-compatible.** Metode dideklarasikan eksplisit
   `-> impl Future<...> + Send + '_` (bukan `async fn` polos) supaya future-nya bisa
   dipakai di `tokio::spawn`. Registry memakai enum dispatch `AnySupplier`
   (Sondev/LahaStore) yang mengimplementasikan trait yang sama — tanpa `Box`/`dyn`.
   Metode khusus supplier tetap bisa diakses lewat `AnySupplier::as_sondev()` /
   `as_lahastore()`.
3. **Limit 120 req/menit Sondev ditegakkan di sisi klien** (sliding window 60 detik di
   adapter, `acquire_rate_slot()` sebelum tiap percobaan). TS tidak punya limiter
   klien; ini tambahan sesuai tugas kelompok B supaya tidak kena 429.
4. **Harga grosir muse_token Rp800/1B** dicatat sebagai konstanta
   `MUSE_TOKEN_WHOLESALE_IDR_PER_B` (nilai live terverifikasi 2026-11-21). Sumber
   kebenaran harga tetap `/products` (`price_idr`); konstanta hanya fallback/dokumentasi.
5. **`is_configured()`** (baru, default `true` di trait): `false` kalau kunci kosong.
   Dipakai pemanggil untuk fail-open / menampilkan "Segera hadir" (kasus LahaStore:
   kunci di env TS memang INVALID/401 — adapter tidak mencari kunci di mana pun).
6. **Konversi LahaStore:** `usd_to_idr` / `vnd_to_idr` ada di adapter (kurs dari
   `Config.idr_per_usd`, default 16500; `VND_PER_USD = 26500` seperti `pricing.ts`).
   Ini duplikasi sementara sampai modul `pricing.rs` (kelompok lain) selesai —
   saat itu helper ini boleh diganti memanggil pricing.
7. **Angka:** uang/harga `f64` (seperti `number` TS), stok/qty `i64`.
   `SupplierBalance.idr` LahaStore = `0.0` (seperti TS).
8. **Error:** semua metode mengembalikan `anyhow::Result` (TS melempar `Error`).
   Pesan error Sondev mempertahankan format TS: `KODE: pesan (req <request_id>)`.
9. **`get_stock` Sondev** men-sanitasi kode seperti TS (hanya `[a-z0-9_]`, maks 50 char).
10. **Timeout:** Sondev 25 dtk/request, probe publik 10 dtk (seperti TS); LahaStore
    25 dtk (TS tidak memasang timeout — dipilih supaya supplier lambat tidak menggantung).

## Signature yang disediakan untuk kelompok lain

```rust
// types.rs
pub enum SupplierId { Sondev, LahaStore }           // as_str(), from_str(), Display
pub struct SupplierBalance { idr: f64, usd: f64, credit: f64, raw: Value }
pub struct SupplierProduct { code, name, wholesale_price_idr: Option<f64>,
    wholesale_price_usd: Option<f64>, stock: Option<i64>,
    min_qty: Option<i64>, max_qty: Option<i64> }
pub enum SupplierOrderStatus { Pending, Processing, Success, Failed, Cancelled, Unknown }
pub struct SupplierOrderResult { supplier_order_id, status, delivered_items: Vec<String>,
    result_link: Option<String>, error_code: Option<String>,
    error_message: Option<String>, raw: Value }
pub struct CreateOrderInput { idempotency_key: String, supplier_product_code: String,
    quantity: i64, target: Option<String> }

pub trait SupplierAdapter: Send + Sync {
    fn id(&self) -> SupplierId;
    fn is_configured(&self) -> bool;                       // default true
    async fn get_balance(&self) -> Result<SupplierBalance>;
    async fn list_products(&self) -> Result<Vec<SupplierProduct>>;
    async fn get_stock(&self, code: &str) -> Result<Option<i64>>;
    async fn create_order(&self, input: CreateOrderInput) -> Result<SupplierOrderResult>;
    async fn get_order(&self, id: &str) -> Result<SupplierOrderResult>;
}

// mod.rs (registry)
pub fn get_supplier(id: SupplierId, config: &Config) -> anyhow::Result<AnySupplier>;
pub fn all_suppliers(config: &Config) -> anyhow::Result<Vec<AnySupplier>>;
pub const SUPPLIER_IDS: [SupplierId; 2];
pub enum AnySupplier { Sondev(SondevAdapter), LahaStore(LahaStoreAdapter) } // impl SupplierAdapter
impl AnySupplier { pub fn as_sondev(&self) -> Option<&SondevAdapter>;
                   pub fn as_lahastore(&self) -> Option<&LahaStoreAdapter>; }

// sondev.rs
impl SondevAdapter {
    pub fn new(api_key: String, api_secret: String, base_url: String) -> Result<Self>;
    pub fn from_config(cfg: &Config) -> Result<Self>;
    pub async fn get_public_status(&self) -> Result<SondevPublicStatus>; // accepting_orders: Option<bool>
    pub async fn list_orders(&self, limit: u32) -> Result<Vec<Value>>;
    pub async fn get_logs(&self, limit: u32) -> Result<Value>;
    pub async fn test_webhook(&self, url: Option<&str>) -> Result<Value>;
    pub async fn get_dead_letters(&self) -> Result<Value>;
    pub async fn replay_webhooks(&self) -> Result<Value>;
}
pub fn sign(secret: &str, timestamp: &str, method: &str, path: &str, body: &str) -> String;
pub const MUSE_TOKEN_WHOLESALE_IDR_PER_B: f64;   // 800.0
pub const SONDEV_PRODUCTS_CACHE_TTL: Duration;    // 120 dtk
pub const SONDEV_RATE_LIMIT_PER_MIN: usize;       // 120

// lahastore.rs
impl LahaStoreAdapter {
    pub fn new(api_key: String, base_url: String, idr_per_usd: i64) -> Result<Self>;
    pub fn from_config(cfg: &Config) -> Result<Self>;
    pub fn usd_to_idr(&self, usd: f64) -> i64;
    pub fn vnd_to_idr(&self, vnd: f64) -> i64;
    pub async fn create_xiaomi_order(&self, input: CreateXiaomiOrderInput) -> Result<XiaomiOrdersResult>;
    pub async fn get_xiaomi_order(&self, id: i64) -> Result<SupplierOrderResult>;
    pub async fn create_sheerid_verification(&self, sheerid_url: &str, client_order_id: &str,
        discount_code: Option<&str>) -> Result<SheeridVerificationResult>;
    pub async fn get_sheerid_job(&self, job_id: &str) -> Result<SheeridJobResult>;
}
pub enum LahaCurrency { Usd, Vnd }
pub struct CreateXiaomiOrderInput { client_order_id: String, currency: LahaCurrency, accounts: Vec<String> }
pub struct XiaomiOrdersResult { orders: Vec<SupplierOrderResult>, duplicated: bool }
pub struct SheeridJobResult { status: String, result_link: Option<String>, raw: Value }
pub struct SheeridVerificationResult { job_id: String, status: String, raw: Value }
```

Contoh pakai (kelompok orders/bot/web):

```rust
use unicodes_digital::lib::suppliers::{get_supplier, SupplierId, SupplierAdapter, CreateOrderInput};

let supplier = get_supplier(SupplierId::Sondev, &cfg)?;
let stock = supplier.get_stock("muse_token").await?; // None = tidak diketahui, bukan 0
let result = supplier.create_order(CreateOrderInput {
    idempotency_key: order_id.clone(),      // ID order internal kita
    supplier_product_code: "muse_token".into(),
    quantity: 5,
    target: Some(invite_code),              // kode invite POLOS (bukan link)
}).await?;
```

## Yang belum / di luar lingkup kelompok B

- **Tes live ke API Sondev/LahaStore tidak dilakukan** — butuh kunci live; kunci
  LahaStore memang INVALID (401) dan kunci Sondev hanya di env (tidak dibaca/dicari).
  Unit test yang ada (`cargo test`): formula signature, `base_path`, `map_status`,
  `to_order_result` (kasus `ALREADY_EXISTS`), backoff, konversi kurs.
- `pricing.rs` (kelompok lain) belum ada — helper konversi di adapter bersifat sementara.
- `catalog.rs` / `orders.rs` (kelompok lain) yang akan memakai adapter ini belum ada.
- Tidak ada perubahan perilaku bot/web — kelompok lain yang mengintegrasikan.

## Verifikasi

- `cargo check` pada crate utama: **modul kelompok B 0 error.** Satu-satunya error
  tersisa di crate adalah `src/lib/payments/gopay.rs:507` (`cannot find value
  retried`) — file kelompok payments, ditulis bersamaan oleh kelompok lain saat
  saya bekerja (jumlah error crate turun 18 → 1 selama sesi saya karena kelompok
  lain juga memperbaiki file mereka). Bukan tanggung jawab kelompok B; jangan
  perbaiki dari sini.
- Bukti modul supplier bersih: salinan `config.rs` + `src/lib/suppliers/*.rs`
  di-compile & di-test terisolasi di scratch crate (deps sama, `--offline`):
  `cargo check` lolos dan **`cargo test`: 8 passed, 0 failed** (formula signature
  HMAC, `base_path`, `map_status` Sondev+LahaStore, `to_order_result`
  `ALREADY_EXISTS`, backoff, konversi kurs). Scratch crate sudah dihapus.
- Satu warning tersisa di file saya: `unused import: SupplierOrderStatus` di
  `src/lib/suppliers/mod.rs:23` — itu re-export API publik yang memang ditujukan
  untuk kelompok lain; warning hilang sendiri begitu ada pemakai. Bukan error.
- Tes live ke API Sondev/LahaStore tidak dilakukan (lihat "Yang belum").
