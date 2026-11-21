//! Override harga retail dari web admin — port `src/lib/product-prices.ts`.
//! Harga dasar tetap di CATALOG (kode); kalau admin mengubah harga lewat
//! /admin, nilainya tersimpan di tabel ini dan dipakai sebagai harga efektif
//! di mana pun harga dibaca (bot, web, order baru). Kosongkan override =
//! kembali ke harga dasar di kode.
//!
//! Tabel `product_price_overrides` dibuat di `crate::db::ensure_tables`
//! (dulu lazy via `ensureProductPriceTables()` di TS).

use crate::db::{now_iso, Db};

/// Harga override (IDR) untuk satu produk, atau None kalau belum diubah.
pub fn get_price_override(db: &Db, product_id: &str) -> Option<i64> {
    let conn = db.lock().ok()?;
    conn.query_row(
        "SELECT retail_price_idr FROM product_price_overrides WHERE product_id = ?1",
        rusqlite::params![product_id],
        |row| row.get::<_, i64>(0),
    )
    .ok()
}

/// Simpan harga retail baru (IDR) untuk satu produk.
pub fn set_price_override(db: &Db, product_id: &str, price_idr: i64) -> anyhow::Result<()> {
    let conn = db.lock().map_err(|e| anyhow::anyhow!("db lock poisoned: {e}"))?;
    conn.execute(
        "INSERT INTO product_price_overrides (product_id, retail_price_idr, updated_at)
         VALUES (?1, ?2, ?3)
         ON CONFLICT(product_id) DO UPDATE SET
           retail_price_idr = excluded.retail_price_idr,
           updated_at = excluded.updated_at",
        rusqlite::params![product_id, price_idr, now_iso()],
    )?;
    Ok(())
}

/// Hapus override — harga kembali ke harga dasar di kode.
/// Dipakai tes + admin; jalur produksi hanya membaca harga efektif.
#[allow(dead_code)]
pub fn clear_price_override(db: &Db, product_id: &str) -> anyhow::Result<()> {
    let conn = db.lock().map_err(|e| anyhow::anyhow!("db lock poisoned: {e}"))?;
    conn.execute(
        "DELETE FROM product_price_overrides WHERE product_id = ?1",
        rusqlite::params![product_id],
    )?;
    Ok(())
}

/// Harga efektif: override admin kalau ada, kalau tidak harga dasar.
/// `base_price_idr` = harga di CATALOG.
pub fn get_effective_retail_price(db: &Db, product_id: &str, base_price_idr: i64) -> i64 {
    get_price_override(db, product_id).unwrap_or(base_price_idr)
}

/// Semua override yang tersimpan (untuk tabel admin).
/// Dipakai tes + admin.
#[allow(dead_code)]
pub fn list_price_overrides(db: &Db) -> Vec<(String, i64, String)> {
    let conn = match db.lock() {
        Ok(g) => g,
        Err(_) => return Vec::new(),
    };
    let mut stmt = match conn.prepare(
        "SELECT product_id, retail_price_idr, updated_at FROM product_price_overrides ORDER BY product_id",
    ) {
        Ok(s) => s,
        Err(_) => return Vec::new(),
    };
    let rows = stmt.query_map([], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, i64>(1)?,
            row.get::<_, String>(2)?,
        ))
    });
    match rows {
        Ok(mapped) => mapped.filter_map(|r| r.ok()).collect(),
        Err(_) => Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db;

    fn test_db() -> Db {
        db::open(":memory:").expect("db tes")
    }

    #[test]
    fn override_round_trip() {
        let db = test_db();
        assert_eq!(get_price_override(&db, "muse-token-1b"), None);
        assert_eq!(get_effective_retail_price(&db, "muse-token-1b", 2000), 2000);
        set_price_override(&db, "muse-token-1b", 2500).unwrap();
        assert_eq!(get_price_override(&db, "muse-token-1b"), Some(2500));
        assert_eq!(get_effective_retail_price(&db, "muse-token-1b", 2000), 2500);
        // Upsert: harga berubah tanpa duplikat.
        set_price_override(&db, "muse-token-1b", 3000).unwrap();
        assert_eq!(get_price_override(&db, "muse-token-1b"), Some(3000));
        assert_eq!(list_price_overrides(&db).len(), 1);
        clear_price_override(&db, "muse-token-1b").unwrap();
        assert_eq!(get_price_override(&db, "muse-token-1b"), None);
        assert_eq!(get_effective_retail_price(&db, "muse-token-1b", 2000), 2000);
    }

    #[test]
    fn list_overrides_sorted() {
        let db = test_db();
        set_price_override(&db, "gemini-xiaomi-3bln", 30000).unwrap();
        set_price_override(&db, "muse-token-1b", 2500).unwrap();
        let list = list_price_overrides(&db);
        assert_eq!(list.len(), 2);
        assert_eq!(list[0].0, "gemini-xiaomi-3bln");
        assert_eq!(list[1].0, "muse-token-1b");
        assert_eq!(list[1].1, 2500);
    }

    #[test]
    fn clear_missing_is_noop() {
        let db = test_db();
        clear_price_override(&db, "tidak-ada").unwrap();
        assert!(list_price_overrides(&db).is_empty());
    }
}
