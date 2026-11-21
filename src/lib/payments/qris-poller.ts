// Poller feed GoBiz — SATU-SATUNYA tempat pembayaran QRIS dinyatakan lunas.
// Cara kerja: ambil transaksi terbaru dari feed GoBiz -> cocokkan dengan
// pembayaran QRIS pending via reconcileFeedTransactions (qris.ts) -> untuk
// tiap match, jalankan jalur yang SUDAH ADA:
//   - top-up (ref TOPUP-...) -> markTopupPaid (wallet.ts, idempotent),
//   - order (ref UD-...) -> setOrderPaidVia("qris") -> markPaid ->
//     fulfillOrder (orders.ts) + notifyBuyer.
// Tidak ada logika fulfillment / saldo yang diduplikasi di sini.
// QR yang kedaluwarsa -> pembayaran jadi 'expired' + order terkait
// dibatalkan (cancelOrderForExpiredPayment) supaya tidak menggantung.
//
// Dijalankan berkala dari proses bot (src/bot/run.ts, interval default
// 30 detik) dan manual dari panel admin (POST /api/admin/payments/gopay/poll).
// tick() TIDAK PERNAH throw — semua kegagalan dikembalikan sebagai hasil
// { ok: false, error, code } supaya interval tidak mati.
import { gopayClient, isAuthRequired, type GopayClient } from "./gopay";
import {
  expireQrisPayments,
  listExpiredPendingQrisPayments,
  listPendingQrisPayments,
  markQrisPaymentPaid,
  reconcileFeedTransactions,
} from "./qris";
import {
  cancelOrderForExpiredPayment,
  fulfillOrder,
  getStoreOrder,
  markPaid,
  setOrderPaidVia,
} from "../orders";
import { getTopup, markTopupPaid, maybeRewardReferrer, tgIdFromBuyerRef } from "../wallet";
import { notifyBuyer } from "../notify";

export interface QrisPollResult {
  ok: boolean;
  /** Jumlah pembayaran pending yang dicek. */
  checked: number;
  /** Jumlah pembayaran yang lunas di tick ini. */
  matched: number;
  /** Jumlah QR yang kedaluwarsa di tick ini. */
  expired: number;
  /** Pesan error (kalau ok === false). */
  error?: string;
  /** Kode error: "auth_required" | "feed_error". */
  code?: "auth_required" | "feed_error";
}

function logErr(...args: unknown[]): void {
  console.error("[qris-poller]", ...args);
}

/** Satu tick polling. Aman dipanggil berkala — idempotent. */
export async function tickQrisPayments(
  client: GopayClient = gopayClient,
): Promise<QrisPollResult> {
  const now = new Date();
  // Tidak ada yang menunggu + tidak ada yang kedaluwarsa = jangan panggil
  // feed sama sekali (diam — bukan error; sejak Midtrans jadi jalur bayar
  // aktif, ini jalur yang paling sering terjadi).
  const pendingNow = listPendingQrisPayments(now);
  const expiredNow = listExpiredPendingQrisPayments(now);
  if (pendingNow.length === 0 && expiredNow.length === 0) {
    return { ok: true, checked: 0, matched: 0, expired: 0 };
  }
  let feed: Awaited<ReturnType<GopayClient["listFeedTransactions"]>>;
  try {
    feed = await client.listFeedTransactions();
  } catch (err) {
    if (isAuthRequired(err)) {
      return {
        ok: false,
        checked: 0,
        matched: 0,
        expired: 0,
        error:
          "Sesi GoBiz belum login / kedaluwarsa — minta OTP di panel admin (/admin).",
        code: "auth_required",
      };
    }
    logErr("feed gagal:", err instanceof Error ? err.message : String(err));
    return {
      ok: false,
      checked: 0,
      matched: 0,
      expired: 0,
      error: err instanceof Error ? err.message : "Feed GoBiz gagal dibaca.",
      code: "feed_error",
    };
  }
  if (feed.truncated) {
    logErr("feed terpotong (lebih dari 10 halaman) — sebagian transaksi mungkin terlewat tick ini.");
  }

  const pending = listPendingQrisPayments(now);
  const matches = reconcileFeedTransactions(pending, feed.transactions);
  let matched = 0;
  for (const { payment, transaction } of matches) {
    try {
      if (!markQrisPaymentPaid(payment.id)) continue; // sudah diproses tick lain
      matched += 1;
      const ref = payment.reference;
      if (ref.startsWith("TOPUP-")) {
        const topup = getTopup(ref);
        if (!topup) {
          logErr(`top-up ${ref} tidak ditemukan (transaksi feed ${transaction.id} sudah ditandai lunas).`);
          continue;
        }
        const { credited } = markTopupPaid(ref);
        if (credited) {
          console.log(`[qris-poller] top-up ${ref} lunas (feed ${transaction.id}) — saldo masuk.`);
        }
      } else if (ref.startsWith("UD-")) {
        const order = await getStoreOrder(ref);
        if (!order) {
          logErr(`order ${ref} tidak ditemukan (transaksi feed ${transaction.id} sudah ditandai lunas).`);
          continue;
        }
        if (order.status !== "awaiting_payment") continue; // sudah dibayar/diproses
        await setOrderPaidVia(order.id, "qris");
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
        console.log(`[qris-poller] order ${ref} lunas (feed ${transaction.id}) — fulfill dimulai.`);
      } else if (ref.startsWith("ADMIN-TEST-")) {
        // QR tes panel admin — tidak ada tindak lanjut.
      } else {
        logErr(`referensi pembayaran tidak dikenal: ${ref} (feed ${transaction.id}) — sudah ditandai lunas, cek manual.`);
      }
    } catch (err) {
      logErr(`proses match ${payment.reference} gagal:`, err);
    }
  }

  // Sapu QR kedaluwarsa + batalkan order yang QR-nya mati.
  const expiredList = listExpiredPendingQrisPayments(now);
  expireQrisPayments(now);
  let expired = 0;
  for (const payment of expiredList) {
    expired += 1;
    if (!payment.reference.startsWith("UD-")) continue;
    try {
      const order = await getStoreOrder(payment.reference);
      if (order && order.status === "awaiting_payment") {
        await cancelOrderForExpiredPayment(
          order.id,
          "Pembayaran QRIS kedaluwarsa (5 menit) sebelum lunas — silakan buat order baru.",
        );
      }
    } catch (err) {
      logErr(`batalkan order kedaluwarsa ${payment.reference} gagal:`, err);
    }
  }

  return { ok: true, checked: pending.length, matched, expired };
}
