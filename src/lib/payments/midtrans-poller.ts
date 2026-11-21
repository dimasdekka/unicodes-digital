// Poller status Midtrans — SATU-SATUNYA tempat pembayaran Midtrans
// dinyatakan lunas LEWAT POLLING (jalur kedua: webhook, route terpisah).
// Cara kerja: ambil pembayaran Midtrans pending -> GET /v2/{order_id}/status
// -> lunas: jalankan jalur yang SUDAH ADA (top-up -> markTopupPaid; order
// -> setOrderPaidVia("midtrans") -> markPaid -> fulfillOrder + notifyBuyer).
// Gagal/ditolak/kedaluwarsa: order -> failed (top-up dibiarkan pending
// sampai kedaluwarsa — tidak ada saldo yang masuk).
// Dijalankan berkala dari proses bot (src/bot/run.ts, interval 30 detik).
// tick() TIDAK PERNAH throw — semua kegagalan dikembalikan sebagai hasil.
import { midtransConfigured } from "../config";
import {
  expireMidtransPayments,
  getTransactionStatus,
  listExpiredPendingMidtransPayments,
  listPendingMidtransPayments,
  mapMidtransStatus,
  markMidtransPaymentFailed,
  markMidtransPaymentPaid,
} from "./midtrans";
import {
  cancelOrderForExpiredPayment,
  fulfillOrder,
  getStoreOrder,
  markOrderFailed,
  markPaid,
  setOrderPaidVia,
} from "../orders";
import {
  getTopup,
  markTopupPaid,
  maybeRewardReferrer,
  tgIdFromBuyerRef,
} from "../wallet";
import { notifyBuyer } from "../notify";

export interface MidtransPollResult {
  ok: boolean;
  /** Jumlah pembayaran pending yang dicek. */
  checked: number;
  /** Jumlah pembayaran yang lunas di tick ini. */
  matched: number;
  /** Jumlah pembayaran yang kedaluwarsa di tick ini. */
  expired: number;
  /** Pesan error (kalau ok === false). */
  error?: string;
  /** Kode error: "not_configured" | "status_error". */
  code?: "not_configured" | "status_error";
}

function logErr(...args: unknown[]): void {
  console.error("[midtrans-poller]", ...args);
}

/** Proses satu hasil status Midtrans untuk satu referensi (dipakai poller
 *  DAN webhook — satu-satunya tempat transisi lunas/gagal Midtrans). */
export async function processMidtransResult(
  orderId: string,
  txStatus: string,
): Promise<"paid" | "failed" | "pending" | "ignored"> {
  const mapped = mapMidtransStatus(txStatus);
  if (mapped === "pending") return "pending";
  if (mapped === "paid") {
    if (!markMidtransPaymentPaid(orderId)) return "ignored"; // sudah diproses
    if (orderId.startsWith("TOPUP-")) {
      const topup = getTopup(orderId);
      if (!topup) {
        logErr(`top-up ${orderId} tidak ditemukan (sudah ditandai lunas).`);
        return "ignored";
      }
      const { credited } = markTopupPaid(orderId);
      if (credited) {
        console.log(`[midtrans-poller] top-up ${orderId} lunas — saldo masuk.`);
      }
      return "paid";
    }
    if (orderId.startsWith("UD-")) {
      const order = await getStoreOrder(orderId);
      if (!order) {
        logErr(`order ${orderId} tidak ditemukan (sudah ditandai lunas).`);
        return "ignored";
      }
      if (order.status !== "awaiting_payment") return "ignored"; // sudah dibayar/diproses
      await setOrderPaidVia(order.id, "midtrans");
      await markPaid(order.id);
      try {
        const tgId = tgIdFromBuyerRef(order.buyerRef);
        if (tgId != null) maybeRewardReferrer(tgId);
      } catch (err) {
        logErr(`reward referral ${order.id} gagal:`, err);
      }
      // Fulfillment jalan di background (bisa ~10 menit) — jangan tahan tick.
      void fulfillOrder(order.id)
        .then((done) => notifyBuyer(done))
        .catch((err: unknown) => {
          logErr(`fulfill ${order.id} gagal:`, err);
        });
      console.log(`[midtrans-poller] order ${orderId} lunas — fulfill dimulai.`);
      return "paid";
    }
    return "ignored";
  }
  // failed (deny/cancel/expire/failure/dll)
  if (!markMidtransPaymentFailed(orderId)) return "ignored"; // sudah diproses
  if (orderId.startsWith("UD-")) {
    const order = await getStoreOrder(orderId);
    if (order && order.status === "awaiting_payment") {
      await markOrderFailed(
        order.id,
        `Pembayaran Midtrans ${txStatus} — silakan buat order baru.`,
      );
    }
  }
  return "failed";
}

/** Satu tick polling. Aman dipanggil berkala — idempotent. */
export async function tickMidtransPayments(): Promise<MidtransPollResult> {
  if (!midtransConfigured()) {
    // Belum ada key = tidak ada yang bisa dicek; diam (bukan error).
    return { ok: true, checked: 0, matched: 0, expired: 0 };
  }
  const now = new Date();
  const pending = listPendingMidtransPayments(now);
  const expiredList = listExpiredPendingMidtransPayments(now);
  let matched = 0;
  for (const payment of pending) {
    try {
      const status = await getTransactionStatus(payment.order_id);
      const res = await processMidtransResult(
        payment.order_id,
        status.transaction_status,
      );
      if (res === "paid") matched += 1;
    } catch (err) {
      // 404 = transaksi belum ada di Midtrans — bukan error, lewati diam-diam.
      const msg = err instanceof Error ? err.message : String(err);
      if (!/HTTP 404/.test(msg)) {
        logErr(`status ${payment.order_id} gagal:`, msg);
      }
    }
  }

  // Sapu pembayaran kedaluwarsa + batalkan order yang link-nya mati.
  expireMidtransPayments(now);
  let expired = 0;
  for (const payment of expiredList) {
    expired += 1;
    if (!payment.order_id.startsWith("UD-")) continue;
    try {
      const order = await getStoreOrder(payment.order_id);
      if (order && order.status === "awaiting_payment") {
        await cancelOrderForExpiredPayment(
          order.id,
          "Pembayaran Midtrans kedaluwarsa (30 menit) sebelum lunas — silakan buat order baru.",
        );
      }
    } catch (err) {
      logErr(`batalkan order kedaluwarsa ${payment.order_id} gagal:`, err);
    }
  }

  return { ok: true, checked: pending.length, matched, expired };
}
