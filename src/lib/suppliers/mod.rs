//! Registry supplier — port dari `src/lib/suppliers/index.ts` (versi TS).
//!
//! Semua kode lain (katalog, order, checkout) hanya kenal trait
//! [`SupplierAdapter`] — mereka tidak tahu/tidak peduli supplier mana
//! yang dipakai di baliknya.
//!
//! CARA TAMBAH STORE BARU (ke-3, ke-4, dst):
//!   1. Buat file `src/lib/suppliers/namastore.rs` — struct yang implement
//!      [`SupplierAdapter`] dari `./types` (contoh: `sondev.rs` / `lahastore.rs`).
//!   2. Tambah id-nya ke union [`SupplierId`] di `./types.rs`.
//!   3. Daftarkan di [`get_supplier`] di bawah ini (+ varian [`AnySupplier`]).
//!   4. Tambah produknya di `src/catalog.rs` (supplier: "namastore").
//!      Selesai — tidak ada kode lain yang perlu diubah.

pub mod lahastore;
pub mod sondev;
pub mod types;

pub use lahastore::LahaStoreAdapter;
pub use sondev::SondevAdapter;
pub use types::{
    CreateOrderInput, SheeridJob, SupplierAdapter, SupplierBalance, SupplierId,
    SupplierOrderResult, SupplierProduct,
};

use crate::config::Config;
use lahastore::{CreateXiaomiOrderInput, LahaCurrency};

/// Enum dispatch untuk semua adapter supplier.
///
/// Dipakai sebagai pengganti `dyn SupplierAdapter`: trait memakai
/// `impl Future` di metode-metodenya sehingga tidak object-safe,
/// dan enum ini menjaga future tetap `Send` + murah (tanpa Box).
/// Tambah store baru = tambah satu varian + satu cabang `match` di bawah.
#[derive(Debug, Clone)]
pub enum AnySupplier {
    /// Sondev = Muse
    Sondev(SondevAdapter),
    /// LahaStore = AI lain (Gemini dkk)
    LahaStore(LahaStoreAdapter),
}

impl AnySupplier {
    /// Adapter konkret di balik enum (mis. untuk memanggil metode khusus
    /// supplier seperti `LahaStoreAdapter::create_xiaomi_order`).
    pub fn as_lahastore(&self) -> Option<&LahaStoreAdapter> {
        match self {
            AnySupplier::LahaStore(a) => Some(a),
            _ => None,
        }
    }
}

impl SupplierAdapter for AnySupplier {
    fn id(&self) -> SupplierId {
        match self {
            AnySupplier::Sondev(a) => a.id(),
            AnySupplier::LahaStore(a) => a.id(),
        }
    }

    fn is_configured(&self) -> bool {
        match self {
            AnySupplier::Sondev(a) => a.is_configured(),
            AnySupplier::LahaStore(a) => a.is_configured(),
        }
    }

    async fn get_balance(&self) -> anyhow::Result<SupplierBalance> {
        match self {
            AnySupplier::Sondev(a) => a.get_balance().await,
            AnySupplier::LahaStore(a) => a.get_balance().await,
        }
    }

    async fn list_products(&self) -> anyhow::Result<Vec<SupplierProduct>> {
        match self {
            AnySupplier::Sondev(a) => a.list_products().await,
            AnySupplier::LahaStore(a) => a.list_products().await,
        }
    }

    async fn get_stock<'a>(
        &'a self,
        supplier_product_code: &'a str,
    ) -> anyhow::Result<Option<i64>> {
        match self {
            AnySupplier::Sondev(a) => a.get_stock(supplier_product_code).await,
            AnySupplier::LahaStore(a) => a.get_stock(supplier_product_code).await,
        }
    }

    async fn create_order(&self, input: CreateOrderInput) -> anyhow::Result<SupplierOrderResult> {
        match self {
            AnySupplier::Sondev(a) => a.create_order(input).await,
            AnySupplier::LahaStore(a) => a.create_order(input).await,
        }
    }

    async fn get_order<'a>(
        &'a self,
        supplier_order_id: &'a str,
    ) -> anyhow::Result<SupplierOrderResult> {
        match self {
            AnySupplier::Sondev(a) => a.get_order(supplier_order_id).await,
            AnySupplier::LahaStore(a) => a.get_order(supplier_order_id).await,
        }
    }

    // --- LahaStore-only: delegasi ke adapter konkret (cermin logika yang
    // dulu ada di `bot/deps.rs`). Varian Sondev -> default Err trait.
    async fn create_xiaomi_order(
        &self,
        client_order_id: &str,
        accounts: &[String],
    ) -> anyhow::Result<Vec<SupplierOrderResult>> {
        let Some(laha) = self.as_lahastore() else {
            return Err(anyhow::anyhow!("Produk Xiaomi harus lewat LahaStore"));
        };
        // Mata uang default USD (sama seperti pemakaian di TS).
        let res = laha
            .create_xiaomi_order(CreateXiaomiOrderInput {
                client_order_id: client_order_id.to_string(),
                currency: LahaCurrency::Usd,
                accounts: accounts.to_vec(),
            })
            .await?;
        Ok(res.orders)
    }

    async fn get_xiaomi_order(
        &self,
        supplier_order_id: i64,
    ) -> anyhow::Result<SupplierOrderResult> {
        let Some(laha) = self.as_lahastore() else {
            return Err(anyhow::anyhow!("Produk Xiaomi harus lewat LahaStore"));
        };
        laha.get_xiaomi_order(supplier_order_id).await
    }

    async fn create_sheerid_verification(
        &self,
        sheerid_url: &str,
        client_order_id: &str,
    ) -> anyhow::Result<SheeridJob> {
        let Some(laha) = self.as_lahastore() else {
            return Err(anyhow::anyhow!("Produk SheerID harus lewat LahaStore"));
        };
        let res = laha
            .create_sheerid_verification(sheerid_url, client_order_id, None)
            .await?;
        Ok(SheeridJob {
            job_id: res.job_id,
            status: res.status,
            result_link: None,
        })
    }

    async fn get_sheerid_job(&self, job_id: &str) -> anyhow::Result<SheeridJob> {
        let Some(laha) = self.as_lahastore() else {
            return Err(anyhow::anyhow!("Produk SheerID harus lewat LahaStore"));
        };
        let res = laha.get_sheerid_job(job_id).await?;
        Ok(SheeridJob {
            job_id: job_id.to_string(),
            status: res.status,
            result_link: res.result_link,
        })
    }
}

/// Ambil adapter untuk satu supplier (dibangun dari [`Config`] — kunci
/// dibaca dari env, tidak pernah di-hardcode).
pub fn get_supplier(id: SupplierId, config: &Config) -> anyhow::Result<AnySupplier> {
    match id {
        SupplierId::Sondev => Ok(AnySupplier::Sondev(SondevAdapter::from_config(config)?)),
        SupplierId::LahaStore => Ok(AnySupplier::LahaStore(LahaStoreAdapter::from_config(config)?)),
    }
}
