// Override harga retail dari web admin — tanpa edit src/lib/catalog.ts.
// Harga dasar tetap di CATALOG (kode); kalau admin mengubah harga lewat
// /admin, nilainya tersimpan di tabel ini dan dipakai sebagai harga efektif
// di mana pun harga dibaca (bot, web, order baru). Kosongkan override =
// kembali ke harga dasar di kode.
import { getDb } from "./db";

/** Buat tabel override harga kalau belum ada (lazy). */
export function ensureProductPriceTables(): void {
  getDb().exec(`
    CREATE TABLE IF NOT EXISTS product_price_overrides (
      product_id TEXT PRIMARY KEY,
      retail_price_idr INTEGER NOT NULL,
      updated_at TEXT NOT NULL
    );
  `);
}

/** Harga override (IDR) untuk satu produk, atau null kalau belum diubah. */
export function getPriceOverride(productId: string): number | null {
  ensureProductPriceTables();
  const row = getDb()
    .prepare(
      `SELECT retail_price_idr AS price FROM product_price_overrides WHERE product_id = ?`,
    )
    .get(productId) as { price: number } | undefined;
  return row ? row.price : null;
}

/** Simpan harga retail baru (IDR) untuk satu produk. */
export function setPriceOverride(productId: string, priceIdr: number): void {
  ensureProductPriceTables();
  getDb()
    .prepare(
      `INSERT INTO product_price_overrides (product_id, retail_price_idr, updated_at)
        VALUES (?, ?, ?)
        ON CONFLICT(product_id) DO UPDATE SET
          retail_price_idr = excluded.retail_price_idr,
          updated_at = excluded.updated_at`,
    )
    .run(productId, Math.floor(priceIdr), new Date().toISOString());
}

/** Hapus override — harga kembali ke harga dasar di kode. */
export function clearPriceOverride(productId: string): void {
  ensureProductPriceTables();
  getDb()
    .prepare(`DELETE FROM product_price_overrides WHERE product_id = ?`)
    .run(productId);
}

/** Harga efektif: override admin kalau ada, kalau tidak harga dasar. */
export function getEffectiveRetailPrice(product: {
  id: string;
  retailPriceIdr: number;
}): number {
  return getPriceOverride(product.id) ?? product.retailPriceIdr;
}
