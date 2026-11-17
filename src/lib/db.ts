// Database — SQLite bawaan Node (node:sqlite, tanpa dependency tambahan).
// File database: DATABASE_URL di .env.local (default file:./dev.db).
// Produksi: ganti file ini dengan driver Postgres (mis. `pg` + tabel yang sama)
// — kode pemanggil (src/lib/orders.ts) tidak perlu berubah.
//
// PENTING: koneksi dibuka LAZY (saat pertama dipakai), bukan saat modul
// di-import — supaya `next build` tidak membuka database, dan supaya web
// server + bot (2 proses) bisa berbagi file yang sama. Mode WAL diaktifkan
// agar baca/tulis dari 2 proses tidak saling mengunci.
import { DatabaseSync } from "node:sqlite";
import path from "node:path";

function dbPath(): string {
  const url = process.env.DATABASE_URL ?? "file:./dev.db";
  const file = url.replace(/^file:/, "");
  return path.isAbsolute(file) ? file : path.join(process.cwd(), file);
}

const globalForDb = globalThis as unknown as { __unicodesDb?: DatabaseSync };

function openDb(): DatabaseSync {
  const db = new DatabaseSync(dbPath());
  // WAL: pembaca tidak memblokir penulis (web + bot jalan bersamaan).
  db.exec("PRAGMA journal_mode = WAL");
  db.exec(`
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
    -- Dedupe webhook Sondev: X-Sondev-Delivery unik per grup percobaan (docs v1.2).
    -- Webhook yang sama (retry) tidak boleh diproses dua kali.
    CREATE TABLE IF NOT EXISTS webhook_deliveries (
      delivery_id TEXT PRIMARY KEY,
      source TEXT NOT NULL,
      received_at TEXT NOT NULL
    );
  `);
  // Migrasi ringan: tambah kolom target kalau belum ada (order lama tetap jalan).
  const cols = db
    .prepare("PRAGMA table_info(store_orders)")
    .all() as { name: string }[];
  if (!cols.some((c) => c.name === "target")) {
    db.exec("ALTER TABLE store_orders ADD COLUMN target TEXT");
  }
  return db;
}

/** Koneksi database (singleton, dibuka saat pertama dipakai). */
export function getDb(): DatabaseSync {
  if (!globalForDb.__unicodesDb) {
    globalForDb.__unicodesDb = openDb();
  }
  return globalForDb.__unicodesDb;
}

/**
 * Catat satu pengiriman webhook. Mengembalikan true kalau ini pertama kalinya
 * ID itu diterima (false = duplikat/retry, jangan diproses lagi).
 * Dipakai dedupe webhook Sondev via header X-Sondev-Delivery (docs v1.2).
 */
export function recordWebhookDelivery(
  source: string,
  deliveryId: string,
): boolean {
  if (!deliveryId) return true; // tanpa ID, tidak bisa dedupe — proses saja
  const db = getDb();
  const result = db
    .prepare(
      "INSERT OR IGNORE INTO webhook_deliveries (delivery_id, source, received_at) VALUES (?, ?, ?)",
    )
    .run(deliveryId, source, new Date().toISOString());
  return result.changes > 0;
}
