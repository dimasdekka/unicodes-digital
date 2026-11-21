//! Dependensi bot dari kelompok lain (A/B/C).
//!
//! Modul ini hanya berisi helper murni. Dulu ada adapter
//! `SupplierPort`-untuk-`AnySupplier` di sini (dengan konversi
//! field-per-field antar dua definisi tipe supplier yang duplikat);
//! adapter itu SUDAH DIHAPUS 2026-11-21 karena tipenya disatukan —
//! `orders.rs` sekarang me-re-export tipe dari `suppliers/types.rs`
//! dan `AnySupplier` mengimplementasikan `SupplierAdapter` (termasuk
//! delegasi LahaStore-only) langsung di `suppliers/mod.rs`.

use crate::config::Config;

// ---------------------------------------------------------------------------
// Helper murni (bukan milik kelompok lain).
// ---------------------------------------------------------------------------

/// `file_id` banner produk per katalog (env
/// `PRODUCT_BANNER_FILE_ID_<ID>`). `Config` hanya membawa field untuk
/// muse-token-1b (satu-satunya banner yang ada); produk lain -> "".
/// Cerminan `productBannerFileId()` di `src/lib/config.ts`.
pub fn product_banner_file_id(cfg: &Config, product_id: &str) -> String {
    match product_id {
        "muse-token-1b" => cfg.product_banner_file_id_muse_token_1b.clone(),
        _ => String::new(),
    }
}

/// Jam WIB (0-23) saat ini, untuk sapaan waktu di /start.
pub fn wib_hour_now() -> i64 {
    use chrono::{FixedOffset, Timelike, Utc};
    let wib = FixedOffset::east_opt(7 * 3600).unwrap();
    Utc::now().with_timezone(&wib).hour() as i64
}
