//! Katalog gabungan — port `src/lib/catalog.ts`.
//! SATU etalase untuk semua supplier. Tiap produk menunjuk ke suppliernya +
//! cara fulfillment-nya.
//!   - Sondev    = Muse
//!   - LahaStore = AI lain (Gemini dkk)
//!
//! Tambah produk baru = tambah 1 entri di `CATALOG`.
//! Tambah store baru = lihat `src/lib/suppliers/` (1 adapter + daftarkan).
//!
//! CATATAN (dari TS): katalog asli LahaStore BELUM BISA ditarik — API key
//! ditolak 401. JANGAN mengarang produk/kategori LahaStore; katalog di sini
//! masih 3 produk seed.

use crate::db::Db;
use crate::lib::product_prices::get_effective_retail_price;

pub type SupplierId = &'static str;
pub const SUPPLIER_SONDEV: SupplierId = "sondev";
pub const SUPPLIER_LAHASTORE: SupplierId = "lahastore";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Fulfillment {
    /// Order shop biasa via adapter.createOrder
    Standard,
    /// Gemini Pro Xiaomi 3 bln — butuh akun pembeli (email|password|2FA)
    LahastoreXiaomi3,
    /// Gemini Student Pro — verifikasi SheerID
    LahastoreSheerid,
}

impl Fulfillment {
    pub fn as_str(&self) -> &'static str {
        match self {
            Fulfillment::Standard => "standard",
            Fulfillment::LahastoreXiaomi3 => "lahastore-xiaomi3",
            Fulfillment::LahastoreSheerid => "lahastore-sheerid",
        }
    }
}

/// Status stok produk — menentukan badge & apakah produk bisa dibeli.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StockState {
    Available,
    OutOfStock,
    ComingSoon,
}

impl StockState {
    pub fn as_str(&self) -> &'static str {
        match self {
            StockState::Available => "available",
            StockState::OutOfStock => "out_of_stock",
            StockState::ComingSoon => "coming_soon",
        }
    }
}

#[derive(Debug, Clone)]
pub struct CatalogProduct {
    /// slug internal, dipakai di URL & order
    pub id: &'static str,
    pub name: &'static str,
    pub description: &'static str,
    pub supplier: SupplierId,
    /// product_code (Sondev) / productId atau code (LahaStore)
    pub supplier_product_code: &'static str,
    pub fulfillment: Fulfillment,
    /// Kelompok tampilan di katalog ("Muse" / "AI Lain").
    pub category: &'static str,
    /// Status stok — sumber kebenaran untuk badge & tombol beli.
    pub stock_state: StockState,
    /// Harga jual ke pembeli (IDR).
    pub retail_price_idr: i64,
    pub active: bool,
    /// Produk on-demand (mis. Muse) WAJIB target: kode invite / email target.
    pub requires_target: bool,
    /// Label input target di checkout (mis. "Link invite Muse").
    pub target_label: Option<&'static str>,
    /// DEPRECATED — turunan dari stockState ("coming_soon"). Selalu jaga
    /// selaras: coming_soon == true hanya bila stock_state == ComingSoon.
    pub coming_soon: bool,
    /// Label kecil di kartu (mis. "Terlaris").
    pub badge: Option<&'static str>,
    /// Satuan harga (mis. "per 1B token").
    pub unit: Option<&'static str>,
    /// Poin-poin keunggulan buat pembeli.
    pub features: &'static [&'static str],
    /// Warna aksen kartu (gradient).
    pub accent: Option<&'static str>,
}

pub static CATALOG: &[CatalogProduct] = &[
    // --- Sondev = Muse ---
    CatalogProduct {
        id: "muse-token-1b",
        name: "Muse AI Token 1B",
        description: "Recharge 1 miliar token Muse ke akun kamu.",
        supplier: SUPPLIER_SONDEV,
        supplier_product_code: "muse_token",
        fulfillment: Fulfillment::Standard,
        // Harga retail diputuskan Dim 2026-11-21: Rp2.000/1B (grosir Rp800 ->
        // margin Rp1.200). Sondev sendiri jual Rp1.000/1B di botnya — harga
        // ini di atas itu atas keputusan Dim, bukan harga contoh.
        retail_price_idr: 2000,
        active: true,
        category: "Muse",
        stock_state: StockState::Available,
        badge: Some("Terlaris"),
        unit: Some("per 1B token"),
        features: &[
            "Masuk ke akun kamu dalam hitungan menit",
            "Token masuk ke akun Muse kamu (via kode invite)",
            "Harga per 1B — beli sesuai kebutuhan",
        ],
        accent: Some("#7c3aed"),
        requires_target: true,
        target_label: Some("Link invite Muse"),
        coming_soon: false,
    },
    // --- LahaStore = AI lain (Gemini) ---
    CatalogProduct {
        id: "gemini-xiaomi-3bln",
        name: "Gemini Pro — Xiaomi 3 bulan",
        description: "Gemini Pro 3 bulan via Xiaomi. Saat checkout, isi akun (email|password|2FA) yang mau di-upgrade.",
        supplier: SUPPLIER_LAHASTORE,
        supplier_product_code: "xiaomi3",
        fulfillment: Fulfillment::LahastoreXiaomi3,
        retail_price_idr: 35000,
        active: true,
        category: "AI Lain",
        stock_state: StockState::ComingSoon,
        coming_soon: true,
        badge: Some("Hemat"),
        unit: Some("per 3 bulan"),
        features: &[
            "Gemini Pro penuh selama 3 bulan",
            "Isi akun kamu saat checkout",
            "Tanpa ribet — kami yang proses",
        ],
        accent: Some("#2563eb"),
        requires_target: false,
        target_label: None,
    },
    CatalogProduct {
        id: "gemini-student-pro",
        name: "Gemini Student Pro",
        description: "Gemini Student Pro dengan verifikasi pelajar (SheerID).",
        supplier: SUPPLIER_LAHASTORE,
        supplier_product_code: "sheerid",
        fulfillment: Fulfillment::LahastoreSheerid,
        retail_price_idr: 25000,
        active: true,
        category: "AI Lain",
        stock_state: StockState::ComingSoon,
        coming_soon: true,
        badge: None,
        unit: Some("per akun"),
        features: &[
            "Harga pelajar, fitur Pro",
            "Verifikasi pelajar dibantu sampai lolos",
        ],
        accent: Some("#059669"),
        requires_target: false,
        target_label: None,
    },
];

/// Harga efektif: override dari web admin kalau ada (lihat product-prices),
/// kalau tidak harga dasar di CATALOG. Selalu kembalikan salinan.
fn with_effective_price(db: &Db, p: &CatalogProduct) -> CatalogProduct {
    let mut out = p.clone();
    out.retail_price_idr =
        get_effective_retail_price(db, p.id, p.retail_price_idr);
    out
}

pub fn get_catalog_product(db: &Db, id: &str) -> anyhow::Result<CatalogProduct> {
    let product = CATALOG
        .iter()
        .find(|p| p.id == id)
        .ok_or_else(|| anyhow::anyhow!("Produk tidak ditemukan: {id}"))?;
    Ok(with_effective_price(db, product))
}

pub fn list_active_products(db: &Db) -> Vec<CatalogProduct> {
    CATALOG
        .iter()
        .filter(|p| p.active)
        .map(|p| with_effective_price(db, p))
        .collect()
}

/// Daftar kategori (urutan kemunculan pertama di katalog) dari produk aktif.
pub fn list_categories(db: &Db) -> Vec<&'static str> {
    let mut seen: Vec<&'static str> = Vec::new();
    for p in list_active_products(db) {
        if !seen.contains(&p.category) {
            seen.push(p.category);
        }
    }
    seen
}

/// Produk aktif per kategori; "Semua" = semua produk aktif.
pub fn list_active_products_by_category(db: &Db, category: &str) -> Vec<CatalogProduct> {
    if category == "Semua" {
        return list_active_products(db);
    }
    CATALOG
        .iter()
        .filter(|p| p.active && p.category == category)
        .map(|p| with_effective_price(db, p))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db;

    fn test_db() -> Db {
        db::open(":memory:").expect("db tes")
    }

    #[test]
    fn muse_token_price_is_2000() {
        // Harga diputuskan Dim 2026-11-21: retail Rp2.000/1B (grosir Rp800).
        let db = test_db();
        let p = get_catalog_product(&db, "muse-token-1b").unwrap();
        assert_eq!(p.retail_price_idr, 2000);
        assert_eq!(p.supplier, SUPPLIER_SONDEV);
        assert_eq!(p.supplier_product_code, "muse_token");
        assert!(p.requires_target);
        assert_eq!(p.stock_state, StockState::Available);
    }

    #[test]
    fn catalog_has_three_seed_products() {
        let db = test_db();
        let all = list_active_products(&db);
        assert_eq!(all.len(), 3);
        assert!(all.iter().any(|p| p.id == "muse-token-1b"));
        assert!(all.iter().any(|p| p.id == "gemini-xiaomi-3bln"));
        assert!(all.iter().any(|p| p.id == "gemini-student-pro"));
    }

    #[test]
    fn gemini_products_are_coming_soon() {
        // S3/S19: bot hanya menampilkan Muse; Gemini "Segera hadir".
        let db = test_db();
        for id in ["gemini-xiaomi-3bln", "gemini-student-pro"] {
            let p = get_catalog_product(&db, id).unwrap();
            assert_eq!(p.stock_state, StockState::ComingSoon);
            assert!(p.coming_soon);
            assert_eq!(p.supplier, SUPPLIER_LAHASTORE);
        }
    }

    #[test]
    fn unknown_product_errors() {
        let db = test_db();
        assert!(get_catalog_product(&db, "tidak-ada").is_err());
    }

    #[test]
    fn categories_and_filtering() {
        let db = test_db();
        assert_eq!(list_categories(&db), vec!["Muse", "AI Lain"]);
        let muse = list_active_products_by_category(&db, "Muse");
        assert_eq!(muse.len(), 1);
        assert_eq!(muse[0].id, "muse-token-1b");
        let all = list_active_products_by_category(&db, "Semua");
        assert_eq!(all.len(), 3);
        // Kategori tak dikenal = daftar kosong.
        assert!(list_active_products_by_category(&db, "TidakAda").is_empty());
    }

    #[test]
    fn effective_price_uses_admin_override() {
        let db = test_db();
        assert_eq!(
            get_catalog_product(&db, "muse-token-1b").unwrap().retail_price_idr,
            2000
        );
        crate::lib::product_prices::set_price_override(&db, "muse-token-1b", 2500).unwrap();
        assert_eq!(
            get_catalog_product(&db, "muse-token-1b").unwrap().retail_price_idr,
            2500
        );
        crate::lib::product_prices::clear_price_override(&db, "muse-token-1b").unwrap();
        assert_eq!(
            get_catalog_product(&db, "muse-token-1b").unwrap().retail_price_idr,
            2000
        );
    }
}
