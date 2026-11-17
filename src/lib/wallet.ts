// Dompet pembeli, pengguna bot, kode promo, dan referral — SQLite (db.ts).
// Dipakai bot Telegram + webhook Midtrans (top-up). Semua mutasi saldo
// berjalan dalam SATU transaksi SQLite (BEGIN IMMEDIATE ... COMMIT) supaya
// saldo + riwayat tidak pernah setengah jalan.
//
// Aturan bisnis (default, bisa diubah Dim):
// - Reward referral: referrer dapat kredit dompet Rp1.500 setelah teman yang
//   diundang menyelesaikan order berbayar PERTAMA (dibayar sekali per teman).
// - Min isi saldo Rp10.000 (pilihan cepat 10rb/25rb/50rb/100rb + custom).
import type { DatabaseSync } from "node:sqlite";
import { getDb } from "./db";

export const REFERRAL_REWARD_IDR = 1500;
export const MIN_TOPUP_IDR = 10000;
export const TOPUP_AMOUNTS = [10000, 25000, 50000, 100000];

export type Lang = "id" | "en";

export interface BotUser {
  tgId: number;
  firstName: string | null;
  username: string | null;
  lang: Lang;
  referrerTgId: number | null;
}

export interface Wallet {
  tgId: number;
  balanceIdr: number;
  totalTopupIdr: number;
  totalSpentIdr: number;
}

export interface WalletTxn {
  id: number;
  kind: string;
  amountIdr: number;
  balanceAfterIdr: number;
  ref: string | null;
  note: string | null;
  createdAt: string;
}

export interface WalletTopup {
  id: string;
  tgId: number;
  amountIdr: number;
  status: string;
  createdAt: string;
  paidAt: string | null;
}

export interface PromoCode {
  code: string;
  kind: "percent" | "fixed";
  value: number;
  maxUses: number | null;
  usesCount: number;
  validUntil: string | null;
  createdBy: string | null;
  createdAt: string;
}

export interface ReferralStats {
  invited: number;
  completed: number; // teman yang sudah order berbayar pertama
  rewarded: number; // yang reward-nya sudah dibayar
  creditEarned: number; // total kredit referral yang diterima (Rp)
}

function nowIso(): string {
  return new Date().toISOString();
}

/** Jalankan fn di dalam satu transaksi SQLite (atomic). */
function withTx<T>(fn: (db: DatabaseSync) => T): T {
  const db = getDb();
  db.exec("BEGIN IMMEDIATE");
  try {
    const result = fn(db);
    db.exec("COMMIT");
    return result;
  } catch (err) {
    try {
      db.exec("ROLLBACK");
    } catch {
      // sudah rollback / tidak ada transaksi — abaikan
    }
    throw err;
  }
}

// ---------------------------------------------------------------- users ---

export function ensureUser(input: {
  tgId: number;
  firstName?: string | null;
  username?: string | null;
  referrerTgId?: number | null;
}): BotUser {
  const db = getDb();
  const now = nowIso();
  db.prepare(
    `INSERT INTO users (tg_id, first_name, username, lang, referrer_tg_id, created_at, updated_at)
     VALUES (?, ?, ?, 'id', ?, ?, ?)
     ON CONFLICT(tg_id) DO UPDATE SET
       first_name = COALESCE(excluded.first_name, users.first_name),
       username = COALESCE(excluded.username, users.username),
       referrer_tg_id = COALESCE(users.referrer_tg_id, excluded.referrer_tg_id),
       updated_at = excluded.updated_at`,
  ).run(
    input.tgId,
    input.firstName ?? null,
    input.username ?? null,
    input.referrerTgId ?? null,
    now,
    now,
  );
  db.prepare(
    `INSERT OR IGNORE INTO wallets (tg_id, balance_idr, total_topup_idr, total_spent_idr, updated_at)
     VALUES (?, 0, 0, 0, ?)`,
  ).run(input.tgId, now);
  return getUser(input.tgId) ?? {
    tgId: input.tgId,
    firstName: input.firstName ?? null,
    username: input.username ?? null,
    lang: "id",
    referrerTgId: input.referrerTgId ?? null,
  };
}

export function getUser(tgId: number): BotUser | null {
  const row = getDb()
    .prepare("SELECT * FROM users WHERE tg_id = ?")
    .get(tgId) as
    | {
        tg_id: number;
        first_name: string | null;
        username: string | null;
        lang: string;
        referrer_tg_id: number | null;
      }
    | undefined;
  if (!row) return null;
  return {
    tgId: row.tg_id,
    firstName: row.first_name,
    username: row.username,
    lang: row.lang === "en" ? "en" : "id",
    referrerTgId: row.referrer_tg_id,
  };
}

export function setUserLang(tgId: number, lang: Lang): void {
  getDb()
    .prepare("UPDATE users SET lang = ?, updated_at = ? WHERE tg_id = ?")
    .run(lang, nowIso(), tgId);
}

// --------------------------------------------------------------- wallet ---

/** Mutasi saldo + catat riwayat. Harus dipanggil di dalam withTx. */
function applyWalletChange(
  db: DatabaseSync,
  input: {
    tgId: number;
    amountIdr: number; // +masuk, -keluar
    kind: string;
    ref?: string | null;
    note?: string | null;
  },
): number {
  const wallet = db
    .prepare("SELECT * FROM wallets WHERE tg_id = ?")
    .get(input.tgId) as { balance_idr: number } | undefined;
  if (!wallet) throw new Error(`Dompet tidak ditemukan: ${input.tgId}`);
  const balanceAfter = wallet.balance_idr + input.amountIdr;
  if (balanceAfter < 0) throw new Error("Saldo tidak cukup");
  db.prepare(
    `UPDATE wallets SET balance_idr = ?,
       total_topup_idr = total_topup_idr + ?,
       total_spent_idr = total_spent_idr + ?,
       updated_at = ? WHERE tg_id = ?`,
  ).run(
    balanceAfter,
    input.amountIdr > 0 && input.kind === "topup" ? input.amountIdr : 0,
    input.amountIdr < 0 ? -input.amountIdr : 0,
    nowIso(),
    input.tgId,
  );
  db.prepare(
    `INSERT INTO wallet_txns (tg_id, kind, amount_idr, balance_after_idr, ref, note, created_at)
     VALUES (?, ?, ?, ?, ?, ?, ?)`,
  ).run(
    input.tgId,
    input.kind,
    input.amountIdr,
    balanceAfter,
    input.ref ?? null,
    input.note ?? null,
    nowIso(),
  );
  return balanceAfter;
}

export function getWallet(tgId: number): Wallet {
  const row = getDb()
    .prepare("SELECT * FROM wallets WHERE tg_id = ?")
    .get(tgId) as
    | {
        tg_id: number;
        balance_idr: number;
        total_topup_idr: number;
        total_spent_idr: number;
      }
    | undefined;
  if (!row) {
    ensureUser({ tgId });
    return { tgId, balanceIdr: 0, totalTopupIdr: 0, totalSpentIdr: 0 };
  }
  return {
    tgId: row.tg_id,
    balanceIdr: row.balance_idr,
    totalTopupIdr: row.total_topup_idr,
    totalSpentIdr: row.total_spent_idr,
  };
}

/** Tambah saldo. Mengembalikan saldo akhir. */
export function creditWallet(input: {
  tgId: number;
  amountIdr: number;
  kind: string;
  ref?: string | null;
  note?: string | null;
}): number {
  if (input.amountIdr <= 0) throw new Error("Jumlah kredit harus positif");
  return withTx((db) => applyWalletChange(db, { ...input }));
}

/** Potong saldo. Mengembalikan saldo akhir, atau null kalau tidak cukup. */
export function debitWallet(input: {
  tgId: number;
  amountIdr: number;
  kind: string;
  ref?: string | null;
  note?: string | null;
}): number | null {
  if (input.amountIdr <= 0) throw new Error("Jumlah debit harus positif");
  try {
    return withTx((db) => applyWalletChange(db, { ...input, amountIdr: -input.amountIdr }));
  } catch (err) {
    if (err instanceof Error && err.message === "Saldo tidak cukup") return null;
    throw err;
  }
}

export function listWalletTxns(tgId: number, limit = 10): WalletTxn[] {
  const rows = getDb()
    .prepare(
      "SELECT * FROM wallet_txns WHERE tg_id = ? ORDER BY id DESC LIMIT ?",
    )
    .all(tgId, limit) as {
    id: number;
    kind: string;
    amount_idr: number;
    balance_after_idr: number;
    ref: string | null;
    note: string | null;
    created_at: string;
  }[];
  return rows.map((r) => ({
    id: r.id,
    kind: r.kind,
    amountIdr: r.amount_idr,
    balanceAfterIdr: r.balance_after_idr,
    ref: r.ref,
    note: r.note,
    createdAt: r.created_at,
  }));
}

// --------------------------------------------------------------- topup ----

function newTopupId(): string {
  return `TOPUP-${Date.now().toString(36)}-${Math.random()
    .toString(36)
    .slice(2, 8)}`.toUpperCase();
}

export function createTopup(tgId: number, amountIdr: number): WalletTopup {
  if (amountIdr < MIN_TOPUP_IDR) {
    throw new Error(`Minimal isi saldo ${MIN_TOPUP_IDR}`);
  }
  const topup: WalletTopup = {
    id: newTopupId(),
    tgId,
    amountIdr,
    status: "awaiting_payment",
    createdAt: nowIso(),
    paidAt: null,
  };
  getDb()
    .prepare(
      "INSERT INTO wallet_topups (id, tg_id, amount_idr, status, created_at, paid_at) VALUES (?, ?, ?, ?, ?, ?)",
    )
    .run(topup.id, topup.tgId, topup.amountIdr, topup.status, topup.createdAt, null);
  return topup;
}

export function getTopup(id: string): WalletTopup | null {
  const row = getDb()
    .prepare("SELECT * FROM wallet_topups WHERE id = ?")
    .get(id) as
    | {
        id: string;
        tg_id: number;
        amount_idr: number;
        status: string;
        created_at: string;
        paid_at: string | null;
      }
    | undefined;
  if (!row) return null;
  return {
    id: row.id,
    tgId: row.tg_id,
    amountIdr: row.amount_idr,
    status: row.status,
    createdAt: row.created_at,
    paidAt: row.paid_at,
  };
}

/**
 * Tandai top-up lunas + kredit dompet. Idempotent: top-up yang sudah "paid"
 * tidak dikredit dua kali (dipakai webhook Midtrans yang bisa retry).
 */
export function markTopupPaid(id: string): { credited: boolean; topup: WalletTopup | null } {
  const topup = getTopup(id);
  if (!topup) return { credited: false, topup: null };
  if (topup.status === "paid") return { credited: false, topup };
  const done = withTx((db) => {
    const current = db
      .prepare("SELECT status FROM wallet_topups WHERE id = ?")
      .get(id) as { status: string } | undefined;
    if (!current || current.status !== "awaiting_payment") return false;
    db.prepare("UPDATE wallet_topups SET status = 'paid', paid_at = ? WHERE id = ?").run(
      nowIso(),
      id,
    );
    applyWalletChange(db, {
      tgId: topup.tgId,
      amountIdr: topup.amountIdr,
      kind: "topup",
      ref: id,
      note: "Isi saldo via Midtrans",
    });
    return true;
  });
  return { credited: done, topup: getTopup(id) };
}

// ---------------------------------------------------------------- promo ---

export function createPromoCode(input: {
  code: string;
  kind: "percent" | "fixed";
  value: number;
  maxUses?: number | null;
  validDays?: number | null;
  createdBy?: string | null;
}): PromoCode {
  const code = input.code.trim().toUpperCase();
  if (!/^[A-Z0-9]{3,20}$/.test(code)) {
    throw new Error("Kode promo hanya huruf/angka, 3–20 karakter");
  }
  if (input.kind === "percent" && (input.value < 1 || input.value > 100)) {
    throw new Error("Persen diskon harus 1–100");
  }
  if (input.kind === "fixed" && input.value < 100) {
    throw new Error("Potongan tetap minimal Rp100");
  }
  const validUntil =
    input.validDays != null
      ? new Date(Date.now() + input.validDays * 24 * 60 * 60 * 1000).toISOString()
      : null;
  const promo: PromoCode = {
    code,
    kind: input.kind,
    value: input.value,
    maxUses: input.maxUses ?? null,
    usesCount: 0,
    validUntil,
    createdBy: input.createdBy ?? null,
    createdAt: nowIso(),
  };
  getDb()
    .prepare(
      `INSERT INTO promo_codes (code, kind, value, max_uses, uses_count, valid_until, created_by, created_at)
       VALUES (?, ?, ?, ?, 0, ?, ?, ?)`,
    )
    .run(
      promo.code,
      promo.kind,
      promo.value,
      promo.maxUses,
      promo.validUntil,
      promo.createdBy,
      promo.createdAt,
    );
  return promo;
}

export function listPromoCodes(): PromoCode[] {
  const rows = getDb()
    .prepare("SELECT * FROM promo_codes ORDER BY created_at DESC")
    .all() as {
    code: string;
    kind: string;
    value: number;
    max_uses: number | null;
    uses_count: number;
    valid_until: string | null;
    created_by: string | null;
    created_at: string;
  }[];
  return rows.map((r) => ({
    code: r.code,
    kind: r.kind as "percent" | "fixed",
    value: r.value,
    maxUses: r.max_uses,
    usesCount: r.uses_count,
    validUntil: r.valid_until,
    createdBy: r.created_by,
    createdAt: r.created_at,
  }));
}

export function getPromoCode(code: string): PromoCode | null {
  const row = getDb()
    .prepare("SELECT * FROM promo_codes WHERE code = ?")
    .get(code.trim().toUpperCase()) as
    | {
        code: string;
        kind: string;
        value: number;
        max_uses: number | null;
        uses_count: number;
        valid_until: string | null;
        created_by: string | null;
        created_at: string;
      }
    | undefined;
  if (!row) return null;
  return {
    code: row.code,
    kind: row.kind as "percent" | "fixed",
    value: row.value,
    maxUses: row.max_uses,
    usesCount: row.uses_count,
    validUntil: row.valid_until,
    createdBy: row.created_by,
    createdAt: row.created_at,
  };
}

export function hasRedeemedPromo(code: string, tgId: number): boolean {
  const row = getDb()
    .prepare("SELECT 1 AS x FROM promo_redemptions WHERE code = ? AND tg_id = ?")
    .get(code.trim().toUpperCase(), tgId) as { x: number } | undefined;
  return !!row;
}

/** Validasi kode untuk satu pengguna. Mengembalikan alasan kalau tidak valid. */
export function validatePromo(
  code: string,
  tgId: number,
): { ok: true; promo: PromoCode } | { ok: false; reason: string } {
  const promo = getPromoCode(code);
  if (!promo) return { ok: false, reason: "not_found" };
  if (promo.validUntil && new Date(promo.validUntil).getTime() < Date.now()) {
    return { ok: false, reason: "expired" };
  }
  if (promo.maxUses != null && promo.usesCount >= promo.maxUses) {
    return { ok: false, reason: "exhausted" };
  }
  if (hasRedeemedPromo(promo.code, tgId)) {
    return { ok: false, reason: "already_used" };
  }
  return { ok: true, promo };
}

/** Hitung potongan untuk satu total belanja (tanpa menyimpan apa pun). */
export function calcPromoDiscount(promo: PromoCode, totalIdr: number): number {
  if (promo.kind === "percent") {
    return Math.min(totalIdr, Math.round((totalIdr * promo.value) / 100));
  }
  return Math.min(totalIdr, promo.value);
}

/**
 * Tukar promo (dipanggil saat checkout dibayar). Menambah uses_count + mencatat
 * redemption. Mengembalikan false kalau kuota habis / sudah dipakai (race).
 */
export function redeemPromo(input: {
  code: string;
  tgId: number;
  orderId: string;
  discountIdr: number;
}): boolean {
  try {
    return withTx((db) => {
      const promo = db
        .prepare("SELECT * FROM promo_codes WHERE code = ?")
        .get(input.code) as
        | { max_uses: number | null; uses_count: number; valid_until: string | null }
        | undefined;
      if (!promo) return false;
      if (promo.valid_until && new Date(promo.valid_until).getTime() < Date.now()) {
        return false;
      }
      if (promo.max_uses != null && promo.uses_count >= promo.max_uses) return false;
      const dup = db
        .prepare("SELECT 1 AS x FROM promo_redemptions WHERE code = ? AND tg_id = ?")
        .get(input.code, input.tgId) as { x: number } | undefined;
      if (dup) return false;
      db.prepare(
        `INSERT INTO promo_redemptions (code, tg_id, order_id, discount_idr, created_at)
         VALUES (?, ?, ?, ?, ?)`,
      ).run(input.code, input.tgId, input.orderId, input.discountIdr, nowIso());
      db.prepare("UPDATE promo_codes SET uses_count = uses_count + 1 WHERE code = ?").run(
        input.code,
      );
      return true;
    });
  } catch {
    return false;
  }
}

// ------------------------------------------------------------- referral ---

export function referralCode(tgId: number): string {
  return `UD-${tgId}`;
}

export function referralLink(tgId: number): string {
  return `https://t.me/unicodesdigital_bot?start=ref_${referralCode(tgId)}`;
}

/** Catat undangan (sekali per teman). Mengembalikan false kalau sudah ada / self. */
export function recordReferral(referrerTgId: number, referredTgId: number): boolean {
  if (referrerTgId === referredTgId) return false;
  try {
    const result = getDb()
      .prepare(
        `INSERT OR IGNORE INTO referrals (referrer_tg_id, referred_tg_id, rewarded, created_at)
         VALUES (?, ?, 0, ?)`,
      )
      .run(referrerTgId, referredTgId, nowIso());
    return result.changes > 0;
  } catch {
    return false;
  }
}

export function getReferralStats(tgId: number): ReferralStats {
  const db = getDb();
  const invited = (
    db
      .prepare("SELECT COUNT(*) AS n FROM referrals WHERE referrer_tg_id = ?")
      .get(tgId) as { n: number }
  ).n;
  const completed = (
    db
      .prepare(
        `SELECT COUNT(*) AS n FROM referrals r
         WHERE r.referrer_tg_id = ?
           AND EXISTS (SELECT 1 FROM store_orders o
                        WHERE o.buyer_ref = 'tg:' || r.referred_tg_id
                          AND o.status IN ('paid', 'processing', 'delivered')
                          AND o.retail_total_idr > 0)`,
      )
      .get(tgId) as { n: number }
  ).n;
  const rewarded = (
    db
      .prepare(
        "SELECT COUNT(*) AS n FROM referrals WHERE referrer_tg_id = ? AND rewarded = 1",
      )
      .get(tgId) as { n: number }
  ).n;
  const creditRow = db
    .prepare(
      `SELECT COALESCE(SUM(amount_idr), 0) AS total FROM wallet_txns
        WHERE tg_id = ? AND kind = 'referral_reward'`,
    )
    .get(tgId) as { total: number };
  return {
    invited,
    completed,
    rewarded,
    creditEarned: creditRow.total,
  };
}

export function topReferrers(limit = 5): { tgId: number; invited: number; rewarded: number }[] {
  const rows = getDb()
    .prepare(
      `SELECT referrer_tg_id AS tg_id, COUNT(*) AS invited,
              COALESCE(SUM(rewarded), 0) AS rewarded
         FROM referrals GROUP BY referrer_tg_id
        ORDER BY rewarded DESC, invited DESC LIMIT ?`,
    )
    .all(limit) as { tg_id: number; invited: number; rewarded: number }[];
  return rows.map((r) => ({ tgId: r.tg_id, invited: r.invited, rewarded: r.rewarded }));
}

/**
 * Bayar reward referral setelah teman menyelesaikan order berbayar PERTAMA.
 * Idempotent (rewarded=0 dicek + di-update atomik) + hanya untuk order pertama
 * teman itu. Dipanggil setelah markPaid (webhook Midtrans & bayar saldo).
 * Mengembalikan true kalau reward dibayarkan.
 */
export function maybeRewardReferrer(referredTgId: number): boolean {
  // Referrer bisa saja belum pernah memakai bot (tidak punya baris users /
  // wallets) — buatkan dulu supaya kredit reward ada tempatnya.
  const existing = getDb()
    .prepare("SELECT referrer_tg_id FROM referrals WHERE referred_tg_id = ? AND rewarded = 0")
    .get(referredTgId) as { referrer_tg_id: number } | undefined;
  if (existing) ensureUser({ tgId: existing.referrer_tg_id });
  return withTx((db) => {
    const ref = db
      .prepare(
        "SELECT * FROM referrals WHERE referred_tg_id = ? AND rewarded = 0",
      )
      .get(referredTgId) as
      | { referrer_tg_id: number; referred_tg_id: number }
      | undefined;
    if (!ref) return false;
    // Hanya order berbayar pertama yang memicu reward (total > 0 — order
    // gratis dari promo 100% tidak dihitung).
    const paidCount = (
      db
        .prepare(
          `SELECT COUNT(*) AS n FROM store_orders
            WHERE buyer_ref = ? AND status IN ('paid', 'processing', 'delivered')
              AND retail_total_idr > 0`,
        )
        .get(`tg:${referredTgId}`) as { n: number }
    ).n;
    if (paidCount !== 1) return false;
    const claimed = db
      .prepare(
        "UPDATE referrals SET rewarded = 1, rewarded_at = ? WHERE referred_tg_id = ? AND rewarded = 0",
      )
      .run(nowIso(), referredTgId);
    if (claimed.changes === 0) return false;
    applyWalletChange(db, {
      tgId: ref.referrer_tg_id,
      amountIdr: REFERRAL_REWARD_IDR,
      kind: "referral_reward",
      ref: `REF-${referredTgId}`,
      note: `Bonus referral — teman ${referredTgId} order pertama`,
    });
    return true;
  });
}

/** Refund order yang dibayar dari dompet kembali ke dompet pembeli. */
export function refundWalletForOrder(input: {
  tgId: number;
  amountIdr: number;
  orderId: string;
}): number {
  return withTx((db) =>
    applyWalletChange(db, {
      tgId: input.tgId,
      amountIdr: input.amountIdr,
      kind: "refund",
      ref: input.orderId,
      note: "Refund — fulfillment gagal",
    }),
  );
}

/** Ambil tg id dari buyerRef format `tg:<id>` (null kalau bukan format itu). */
export function tgIdFromBuyerRef(buyerRef: string | null): number | null {
  if (!buyerRef || !buyerRef.startsWith("tg:")) return null;
  const n = Number(buyerRef.slice(3));
  return Number.isFinite(n) && n > 0 ? n : null;
}
