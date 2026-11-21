//! Tipe + trait supplier — port dari `src/lib/suppliers/types.ts` (versi TS).
//!
//! Interface yang WAJIB dipenuhi semua supplier. Mau tambah store ke-3,
//! ke-4, dst: cukup bikin 1 file adapter baru yang implement trait
//! [`SupplierAdapter`] + daftarkan di [`crate::lib::suppliers::get_supplier`].
//! Tidak ada kode lain (katalog, order, checkout) yang perlu diubah.
//!
//! Catatan port:
//! - Angka TS (`number`) jadi `f64` untuk uang/harga dan `i64` untuk stok/qty.
//! - `raw: unknown` di TS jadi `serde_json::Value` (`Value::Null` kalau kosong).
//! - Metode trait dideklarasikan eksplisit sebagai `impl Future + Send`
//!   (bukan `async fn` polos) supaya future-nya bisa dipakai di
//!   `tokio::spawn` oleh kelompok lain. Implementasi tetap ditulis sebagai
//!   `async fn` biasa — kompiler memeriksa kecocokannya.

use serde::{Deserialize, Serialize};
use std::fmt;
use std::future::Future;

/// ID supplier yang terdaftar. Tambah varian baru di sini kalau ada store baru.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SupplierId {
    Sondev,
    LahaStore,
}

impl SupplierId {
    pub fn as_str(&self) -> &'static str {
        match self {
            SupplierId::Sondev => "sondev",
            SupplierId::LahaStore => "lahastore",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "sondev" => Some(Self::Sondev),
            "lahastore" => Some(Self::LahaStore),
            _ => None,
        }
    }
}

impl fmt::Display for SupplierId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Saldo dompet supplier yang bisa dipakai belanja.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SupplierBalance {
    /// Saldo dalam IDR kalau bisa dihitung.
    pub idr: f64,
    /// Saldo USD/USDT.
    pub usd: f64,
    /// Kredit internal supplier (kalau ada).
    pub credit: f64,
    /// Respons mentah supplier (untuk debug/admin).
    pub raw: serde_json::Value,
}

/// Satu produk dari katalog supplier beserta harga grosir.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SupplierProduct {
    /// product_code (Sondev) / productId atau code (LahaStore).
    pub code: String,
    pub name: String,
    pub wholesale_price_idr: Option<f64>,
    pub wholesale_price_usd: Option<f64>,
    /// Stok. `None` = tidak diketahui / tidak terbatas.
    pub stock: Option<i64>,
    /// Batas jumlah per order (dari /products atau /stock).
    pub min_qty: Option<i64>,
    pub max_qty: Option<i64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SupplierOrderStatus {
    Pending,
    Processing,
    Success,
    Failed,
    Cancelled,
    Unknown,
}

impl fmt::Display for SupplierOrderStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl SupplierOrderStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            SupplierOrderStatus::Pending => "pending",
            SupplierOrderStatus::Processing => "processing",
            SupplierOrderStatus::Success => "success",
            SupplierOrderStatus::Failed => "failed",
            SupplierOrderStatus::Cancelled => "cancelled",
            SupplierOrderStatus::Unknown => "unknown",
        }
    }

    /// `true` kalau status sudah final (tidak akan berubah lagi).
    pub fn is_terminal(&self) -> bool {
        matches!(
            self,
            SupplierOrderStatus::Success
                | SupplierOrderStatus::Failed
                | SupplierOrderStatus::Cancelled
        )
    }
}

/// Hasil fulfillment satu order supplier.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SupplierOrderResult {
    pub supplier_order_id: String,
    pub status: SupplierOrderStatus,
    /// Barang hasil fulfillment: akun, kode, link, dll (format bebas per supplier).
    pub delivered_items: Vec<String>,
    pub result_link: Option<String>,
    pub error_code: Option<String>,
    pub error_message: Option<String>,
    pub raw: serde_json::Value,
}

/// Input pembuatan order ke supplier.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateOrderInput {
    /// Kunci idempotency — pakai ID order internal kita (unik per order pembeli).
    /// Kirim ulang dengan key yang sama = aman, supplier tidak men-charge 2x.
    pub idempotency_key: String,
    pub supplier_product_code: String,
    pub quantity: i64,
    /// Opsional — target pesanan (mis. kode invite / username / email)
    /// kalau produk membutuhkannya.
    pub target: Option<String>,
}

/// Job verifikasi SheerID (LahaStore): dibuat sekali per order, lalu
/// di-poll sampai `success` / `failed` / `cancelled`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SheeridJob {
    pub job_id: String,
    /// Status mentah dari supplier ("pending" / "success" / ...).
    pub status: String,
    pub result_link: Option<String>,
}

/// Interface yang wajib dipenuhi semua adapter supplier.
pub trait SupplierAdapter: Send + Sync {
    /// Identitas supplier — permukaan interface; pemanggil memakai enum
    /// `AnySupplier`/`SupplierId` untuk dispatch, jadi metode ini tidak
    /// dipanggil langsung.
    #[allow(dead_code)]
    fn id(&self) -> SupplierId;

    /// Saldo dompet supplier saat ini.
    fn get_balance(&self) -> impl Future<Output = anyhow::Result<SupplierBalance>> + Send + '_;

    /// Katalog produk supplier beserta harga grosir.
    fn list_products(&self) -> impl Future<Output = anyhow::Result<Vec<SupplierProduct>>> + Send + '_;

    /// Stok real-time satu produk (`None` kalau tidak diketahui).
    ///
    /// `code` harus hidup selama borrow `&self` (lihat `'_` di bawah):
    /// pemanggil cukup menahan `&str`-nya selama `.await`, yang sudah
    /// terjadi secara alami pada pemanggilan `adapter.get_stock(x).await`.
    fn get_stock<'a>(
        &'a self,
        supplier_product_code: &'a str,
    ) -> impl Future<Output = anyhow::Result<Option<i64>>> + Send + 'a;

    /// Buat order ke supplier.
    fn create_order(
        &self,
        input: CreateOrderInput,
    ) -> impl Future<Output = anyhow::Result<SupplierOrderResult>> + Send + '_;

    /// Cek status order supplier.
    fn get_order<'a>(
        &'a self,
        supplier_order_id: &'a str,
    ) -> impl Future<Output = anyhow::Result<SupplierOrderResult>> + Send + 'a;

    /// `true` kalau adapter ini tidak bisa dipakai (mis. kunci kosong).
    /// Default: bisa dipakai. Permukaan interface untuk fail-open /
    /// "Segera hadir" — saat ini belum ada pemanggil (penanda dihitung
    /// di tempat lain), jadi diizinkan mati.
    #[allow(dead_code)]
    fn is_configured(&self) -> bool {
        true
    }

    // --- Metode LahaStore-only (default: tidak didukung) ---
    //
    // Dipakai `fulfill_order` untuk produk Gemini (Xiaomi / SheerID).
    // Adapter Sondev tidak perlu mengimplementasikannya; `AnySupplier`
    // mendelegasikan ke `LahaStoreAdapter` di `suppliers/mod.rs`.

    /// LahaStore Xiaomi: 1 akun = 1 order supplier. Mengembalikan hasil
    /// awal tiap order dalam batch.
    async fn create_xiaomi_order(
        &self,
        _client_order_id: &str,
        _accounts: &[String],
    ) -> anyhow::Result<Vec<SupplierOrderResult>> {
        Err(anyhow::anyhow!("Produk Xiaomi harus lewat LahaStore"))
    }

    async fn get_xiaomi_order(
        &self,
        _supplier_order_id: i64,
    ) -> anyhow::Result<SupplierOrderResult> {
        Err(anyhow::anyhow!("Produk Xiaomi harus lewat LahaStore"))
    }

    async fn create_sheerid_verification(
        &self,
        _sheerid_url: &str,
        _client_order_id: &str,
    ) -> anyhow::Result<SheeridJob> {
        Err(anyhow::anyhow!("Produk SheerID harus lewat LahaStore"))
    }

    async fn get_sheerid_job(&self, _job_id: &str) -> anyhow::Result<SheeridJob> {
        Err(anyhow::anyhow!("Produk SheerID harus lewat LahaStore"))
    }
}

// ---------------------------------------------------------------------------
// Helper baca field JSON (dipakai adapter-adapter; mirror akses longgar TS).
// ---------------------------------------------------------------------------

/// Ambil field string dari objek JSON.
pub(crate) fn json_str(v: &serde_json::Value, key: &str) -> Option<String> {
    v.get(key)?.as_str().map(|s| s.to_string())
}

/// Ambil field angka (f64) dari objek JSON. String numerik ("12.5") juga diterima.
pub(crate) fn json_f64(v: &serde_json::Value, key: &str) -> Option<f64> {
    match v.get(key)? {
        serde_json::Value::Number(n) => n.as_f64(),
        serde_json::Value::String(s) => s.trim().parse().ok(),
        _ => None,
    }
}

/// Ambil field angka bulat (i64) dari objek JSON.
pub(crate) fn json_i64(v: &serde_json::Value, key: &str) -> Option<i64> {
    match v.get(key)? {
        serde_json::Value::Number(n) => n.as_i64().or_else(|| n.as_f64().map(|f| f as i64)),
        serde_json::Value::String(s) => s.trim().parse().ok(),
        _ => None,
    }
}

