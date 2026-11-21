// Pembayaran via Midtrans Snap (Dim: "yaudah midtrans aja", 2026-11-21).
// Alasan: webhook resmi + deteksi pasti jalan; hook QRIS GoBiz diblokir
// Gojek (GoPay-1000) dan rapuh. Biaya QRIS sama (0,7%, 0% di bawah Rp100rb).
// HANYA QRIS yang diaktifkan (enabled_payments: ["qris"]) — VA/kartu punya
// fee flat (Rp4.000 / 2,9% + Rp2.000) yang lebih mahal dari produk Rp1.500.
// Deteksi DUA jalur (sama-sama idempotent, via processMidtransResult di
// midtrans-poller.ts):
//   1. Polling status API tiap 30 detik (proses bot) — jalan tanpa domain publik.
//   2. Webhook POST /api/webhooks/midtrans (butuh domain publik) —
//      verifikasi signature SHA512(order_id+status_code+gross_amount+server_key).
// Secret TIDAK pernah di-log — pemanggil dilarang mencetak hasilnya.
import { createHash, timingSafeEqual } from "node:crypto";
import { getDb } from "../db";
import {
  midtransIsProduction,
  midtransServerKey,
} from "../config";

/** Masa berlaku link bayar Midtrans (menit). */
export const MIDTRANS_EXPIRY_MINUTES = 30;
/** Satu-satunya metode yang diaktifkan di Snap (alasan: lihat header). */
export const MIDTRANS_QRIS_ONLY = ["qris"] as const;

function snapBase(): string {
  return midtransIsProduction()
    ? "https://app.midtrans.com"
    : "https://app.sandbox.midtrans.com";
}

function apiBase(): string {
  return midtransIsProduction()
    ? "https://api.midtrans.com"
    : "https://api.sandbox.midtrans.com";
}

function authHeader(): string {
  return `Basic ${Buffer.from(`${midtransServerKey()}:`).toString("base64")}`;
}

export interface SnapTransaction {
  token: string;
  redirectUrl: string;
}

/** Buat transaksi Snap — kembalikan token + URL halaman bayar Midtrans.
 *  Pembeli membuka redirectUrl (QRIS tampil di halaman Midtrans). */
export async function createSnapTransaction(args: {
  orderId: string;
  amountIdr: number;
  itemName: string;
}): Promise<SnapTransaction> {
  const res = await fetch(`${snapBase()}/snap/v1/transactions`, {
    method: "POST",
    headers: {
      Accept: "application/json",
      "Content-Type": "application/json",
      Authorization: authHeader(),
    },
    body: JSON.stringify({
      transaction_details: {
        order_id: args.orderId,
        gross_amount: Math.round(args.amountIdr),
      },
      item_details: [
        {
          id: args.orderId,
          price: Math.round(args.amountIdr),
          quantity: 1,
          name: args.itemName.slice(0, 50),
        },
      ],
      enabled_payments: [...MIDTRANS_QRIS_ONLY],
      expiry: { unit: "minute", duration: MIDTRANS_EXPIRY_MINUTES },
    }),
    signal: AbortSignal.timeout(25000),
  });
  const data = (await res.json().catch(() => ({}))) as Record<string, unknown>;
  if (!res.ok || typeof data.token !== "string") {
    const messages = data.error_messages;
    const msg =
      Array.isArray(messages) && messages.length > 0
        ? messages.map(String).join("; ")
        : `Midtrans Snap gagal (HTTP ${res.status})`;
    throw new Error(msg);
  }
  return {
    token: data.token,
    redirectUrl: String(data.redirect_url ?? ""),
  };
}

export type MidtransTxStatus = "paid" | "pending" | "failed";

export interface MidtransStatusBody {
  transaction_status: string;
  status_code: string;
  gross_amount: string;
  payment_type?: string;
  order_id: string;
  [key: string]: unknown;
}

/** settlement/capture = lunas; pending/authorize = tunggu; sisanya = gagal. */
export function mapMidtransStatus(transactionStatus: string): MidtransTxStatus {
  switch (transactionStatus) {
    case "settlement":
    case "capture":
      return "paid";
    case "pending":
    case "authorize":
      return "pending";
    default:
      return "failed";
  }
}

/** GET /v2/{order_id}/status — status transaksi di Midtrans. */
export async function getTransactionStatus(
  orderId: string,
): Promise<MidtransStatusBody> {
  const res = await fetch(
    `${apiBase()}/v2/${encodeURIComponent(orderId)}/status`,
    {
      headers: { Accept: "application/json", Authorization: authHeader() },
      signal: AbortSignal.timeout(25000),
    },
  );
  const data = (await res.json().catch(() => ({}))) as Record<string, unknown>;
  if (!res.ok) {
    const msg =
      typeof data.status_message === "string"
        ? data.status_message
        : `HTTP ${res.status}`;
    throw new Error(`Midtrans status gagal (HTTP ${res.status}): ${msg}`);
  }
  return data as unknown as MidtransStatusBody;
}

/** Verifikasi signature webhook: SHA512(order_id + status_code +
 *  gross_amount + server_key) hex == signature_key (constant-time). */
export function verifyNotificationSignature(body: {
  order_id: string;
  status_code: string;
  gross_amount: string;
  signature_key: string;
}): boolean {
  const input = `${body.order_id}${body.status_code}${body.gross_amount}${midtransServerKey()}`;
  const want = createHash("sha512").update(input).digest("hex");
  const got = body.signature_key;
  if (want.length !== got.length) return false;
  return timingSafeEqual(Buffer.from(want), Buffer.from(got));
}

// ------------------------------------------------------- tabel midtrans ---

export type MidtransPaymentStatus = "pending" | "paid" | "failed" | "expired";

export interface MidtransPaymentRow {
  order_id: string;
  amount_idr: number;
  snap_token: string;
  redirect_url: string;
  status: MidtransPaymentStatus;
  created_at: string;
  expires_at: string;
}

export function createMidtransPayment(args: {
  orderId: string;
  amountIdr: number;
  snapToken: string;
  redirectUrl: string;
  now?: Date;
}): MidtransPaymentRow {
  const now = args.now ?? new Date();
  const expiresAt = new Date(
    now.getTime() + MIDTRANS_EXPIRY_MINUTES * 60_000,
  ).toISOString();
  getDb()
    .prepare(
      "INSERT INTO midtrans_payments (order_id, amount_idr, snap_token, redirect_url, status, created_at, expires_at) VALUES (?, ?, ?, ?, 'pending', ?, ?)",
    )
    .run(
      args.orderId,
      Math.round(args.amountIdr),
      args.snapToken,
      args.redirectUrl,
      now.toISOString(),
      expiresAt,
    );
  const row = getMidtransPayment(args.orderId);
  if (!row) {
    throw new Error("Gagal membaca kembali pembayaran Midtrans yang baru dibuat");
  }
  return row;
}

export function getMidtransPayment(
  orderId: string,
): MidtransPaymentRow | undefined {
  return getDb()
    .prepare("SELECT * FROM midtrans_payments WHERE order_id = ?")
    .get(orderId) as MidtransPaymentRow | undefined;
}

/** Pembayaran pending yang belum kedaluwarsa. */
export function listPendingMidtransPayments(
  now: Date = new Date(),
): MidtransPaymentRow[] {
  return getDb()
    .prepare(
      "SELECT * FROM midtrans_payments WHERE status = 'pending' AND expires_at > ? ORDER BY created_at ASC",
    )
    .all(now.toISOString()) as unknown as MidtransPaymentRow[];
}

/** Daftar pembayaran pending yang sudah lewat expires_at (belum disapu). */
export function listExpiredPendingMidtransPayments(
  now: Date = new Date(),
): MidtransPaymentRow[] {
  return getDb()
    .prepare(
      "SELECT * FROM midtrans_payments WHERE status = 'pending' AND expires_at <= ? ORDER BY created_at ASC",
    )
    .all(now.toISOString()) as unknown as MidtransPaymentRow[];
}

/** Tandai pending jadi 'paid'. False kalau id tidak ada / sudah bukan pending. */
export function markMidtransPaymentPaid(orderId: string): boolean {
  const result = getDb()
    .prepare(
      "UPDATE midtrans_payments SET status = 'paid' WHERE order_id = ? AND status = 'pending'",
    )
    .run(orderId);
  return result.changes > 0;
}

/** Tandai pending jadi 'failed'. False kalau id tidak ada / sudah bukan pending. */
export function markMidtransPaymentFailed(orderId: string): boolean {
  const result = getDb()
    .prepare(
      "UPDATE midtrans_payments SET status = 'failed' WHERE order_id = ? AND status = 'pending'",
    )
    .run(orderId);
  return result.changes > 0;
}

/** Tandai semua pending yang lewat expires_at jadi 'expired'. Kembalikan jumlahnya. */
export function expireMidtransPayments(now: Date = new Date()): number {
  const result = getDb()
    .prepare(
      "UPDATE midtrans_payments SET status = 'expired' WHERE status = 'pending' AND expires_at <= ?",
    )
    .run(now.toISOString());
  return Number(result.changes);
}
