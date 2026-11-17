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

import { databaseUrl } from "./config";

function dbPath(): string {
  const url = databaseUrl();
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
    -- Pengguna bot Telegram (satu baris per chat id).
    CREATE TABLE IF NOT EXISTS users (
      tg_id INTEGER PRIMARY KEY,
      first_name TEXT,
      username TEXT,
      lang TEXT NOT NULL DEFAULT 'id',
      referrer_tg_id INTEGER,
      created_at TEXT NOT NULL,
      updated_at TEXT NOT NULL
    );
    -- Dompet pembeli (saldo dipakai bayar order; diisi via QRIS).
    CREATE TABLE IF NOT EXISTS wallets (
      tg_id INTEGER PRIMARY KEY,
      balance_idr INTEGER NOT NULL DEFAULT 0,
      total_topup_idr INTEGER NOT NULL DEFAULT 0,
      total_spent_idr INTEGER NOT NULL DEFAULT 0,
      updated_at TEXT NOT NULL,
      FOREIGN KEY (tg_id) REFERENCES users(tg_id)
    );
    -- Riwayat mutasi dompet (+masuk, -keluar).
    CREATE TABLE IF NOT EXISTS wallet_txns (
      id INTEGER PRIMARY KEY AUTOINCREMENT,
      tg_id INTEGER NOT NULL,
      kind TEXT NOT NULL,
      amount_idr INTEGER NOT NULL,
      balance_after_idr INTEGER NOT NULL,
      ref TEXT,
      note TEXT,
      created_at TEXT NOT NULL
    );
    CREATE INDEX IF NOT EXISTS idx_wallet_txns_tg ON wallet_txns(tg_id, created_at);
    -- Isi saldo via QRIS (reference pembayaran QRIS = id ini).
    CREATE TABLE IF NOT EXISTS wallet_topups (
      id TEXT PRIMARY KEY,
      tg_id INTEGER NOT NULL,
      amount_idr INTEGER NOT NULL,
      status TEXT NOT NULL DEFAULT 'awaiting_payment',
      created_at TEXT NOT NULL,
      paid_at TEXT
    );
    -- Kode promo (dibuat admin via /promo_add).
    CREATE TABLE IF NOT EXISTS promo_codes (
      code TEXT PRIMARY KEY,
      kind TEXT NOT NULL,
      value INTEGER NOT NULL,
      max_uses INTEGER,
      uses_count INTEGER NOT NULL DEFAULT 0,
      valid_until TEXT,
      created_by TEXT,
      created_at TEXT NOT NULL
    );
    CREATE TABLE IF NOT EXISTS promo_redemptions (
      id INTEGER PRIMARY KEY AUTOINCREMENT,
      code TEXT NOT NULL,
      tg_id INTEGER NOT NULL,
      order_id TEXT,
      discount_idr INTEGER NOT NULL,
      created_at TEXT NOT NULL,
      UNIQUE(code, tg_id)
    );
    -- Referral: siapa mengundang siapa; reward dibayar sekali per teman.
    CREATE TABLE IF NOT EXISTS referrals (
      id INTEGER PRIMARY KEY AUTOINCREMENT,
      referrer_tg_id INTEGER NOT NULL,
      referred_tg_id INTEGER NOT NULL UNIQUE,
      rewarded INTEGER NOT NULL DEFAULT 0,
      rewarded_at TEXT,
      created_at TEXT NOT NULL
    );
    CREATE INDEX IF NOT EXISTS idx_referrals_referrer ON referrals(referrer_tg_id);
    -- Pembayaran QRIS dinamis (Fase 1): satu baris per QR yang dibuat.
    -- unique_amount_idr = nominal + delta unik; matcher memakai nominal ini
    -- untuk membedakan pembayaran yang nominal dasarnya sama.
    CREATE TABLE IF NOT EXISTS qris_payments (
      id TEXT PRIMARY KEY,
      reference TEXT NOT NULL,
      amount_idr INTEGER NOT NULL,
      unique_amount_idr INTEGER NOT NULL,
      qr_string TEXT NOT NULL,
      status TEXT NOT NULL DEFAULT 'pending',
      created_at TEXT NOT NULL,
      expires_at TEXT NOT NULL
    );
    CREATE INDEX IF NOT EXISTS idx_qris_payments_status ON qris_payments(status, unique_amount_idr);
    CREATE INDEX IF NOT EXISTS idx_qris_payments_ref ON qris_payments(reference);
    -- Pengaturan admin (key-value): mis. string QRIS statis GoPay.
    -- Nilai rahasia di sini TIDAK boleh di-commit (hanya di database).
    CREATE TABLE IF NOT EXISTS settings (
      key TEXT PRIMARY KEY,
      value TEXT NOT NULL,
      updated_at TEXT NOT NULL
    );
  `);
  // Migrasi ringan: tambah kolom target kalau belum ada (order lama tetap jalan).
  const cols = db
    .prepare("PRAGMA table_info(store_orders)")
    .all() as { name: string }[];
  if (!cols.some((c) => c.name === "target")) {
    db.exec("ALTER TABLE store_orders ADD COLUMN target TEXT");
  }
  // Migrasi bot v2: diskon promo + cara bayar (untuk refund dompet).
  if (!cols.some((c) => c.name === "discount_idr")) {
    db.exec("ALTER TABLE store_orders ADD COLUMN discount_idr INTEGER NOT NULL DEFAULT 0");
  }
  if (!cols.some((c) => c.name === "promo_code")) {
    db.exec("ALTER TABLE store_orders ADD COLUMN promo_code TEXT");
  }
  if (!cols.some((c) => c.name === "paid_via")) {
    db.exec("ALTER TABLE store_orders ADD COLUMN paid_via TEXT");
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

// ---------------------------------------------------------------------------
// Settings admin (key-value generik)
// ---------------------------------------------------------------------------

/** Baca satu setting; undefined kalau key belum pernah disimpan. */
export function getSetting(key: string): string | undefined {
  const row = getDb()
    .prepare("SELECT value FROM settings WHERE key = ?")
    .get(key) as { value: string } | undefined;
  return row?.value;
}

/** Simpan/ubah satu setting (upsert). */
export function setSetting(key: string, value: string): void {
  getDb()
    .prepare(
      "INSERT INTO settings (key, value, updated_at) VALUES (?, ?, ?) ON CONFLICT(key) DO UPDATE SET value = excluded.value, updated_at = excluded.updated_at",
    )
    .run(key, value, new Date().toISOString());
}
