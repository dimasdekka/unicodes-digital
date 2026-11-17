// Midtrans Snap — payment gateway untuk web & bot Telegram.
// Flow: buat transaksi Snap -> pembeli bayar (QRIS / e-wallet / VA / dll)
//   -> Midtrans kirim webhook (notification) -> verifikasi signature ->
//   status settlement/capture -> markPaid -> fulfillOrder (otomatis).
//
// Daftar: https://dashboard.midtrans.com (sandbox dulu untuk testing).
// Server key & client key dari dashboard -> .env.local.
// JANGAN pernah taruh server key di kode client/browser.
import { createHash } from "node:crypto";
import { markPaid, fulfillOrder, getStoreOrder } from "../orders";

function config(): {
  serverKey: string;
  clientKey: string;
  snapBase: string;
  isProduction: boolean;
} {
  const serverKey = process.env.MIDTRANS_SERVER_KEY;
  const clientKey = process.env.MIDTRANS_CLIENT_KEY;
  if (!serverKey || !clientKey) {
    throw new Error(
      "MIDTRANS_SERVER_KEY / MIDTRANS_CLIENT_KEY belum diisi di .env.local",
    );
  }
  const isProduction = process.env.MIDTRANS_IS_PRODUCTION === "true";
  return {
    serverKey,
    clientKey,
    snapBase: isProduction
      ? "https://app.midtrans.com/snap/v1"
      : "https://app.sandbox.midtrans.com/snap/v1",
    isProduction,
  };
}

export interface SnapTransaction {
  token: string;
  redirectUrl: string;
}

/** Midtrans sering mengembalikan error bisnis sebagai HTTP 200 dengan
 *  status_code 4xx/5xx DI DALAM body, bukan di HTTP status — mis.
 *  GET /v2/{id}/status untuk transaksi yang belum dikenal menjawab HTTP 200
 *  dengan status_code "404" ("Transaction doesn't exist"). Mengecek res.ok
 *  saja tidak cukup: tanpa cek ini, pemanggil salah mengira API berhasil
 *  (mis. transaction_status terbaca "" lalu order tidak pernah lunas,
 *  atau refund gagal diam-diam dianggap sukses). */
function isMidtransBodyError(body: Record<string, unknown>): boolean {
  const code = typeof body.status_code === "string" ? body.status_code : "";
  return /^[45]/.test(code);
}

/** Buat transaksi Snap untuk satu order toko. Mengembalikan token + URL bayar.
 *  order_id Midtrans = ID order internal kita (UD-...) supaya webhook nanti
 *  langsung ketahuan order mana. */
export async function createSnapTransaction(orderId: string): Promise<SnapTransaction> {
  const order = await getStoreOrder(orderId);
  if (!order) throw new Error(`Order tidak ditemukan: ${orderId}`);
  return createSnapTransactionRaw({
    orderId: order.id,
    grossAmount: order.retailTotalIdr,
    itemId: order.catalogProductId,
    itemName: order.catalogProductId,
    itemPrice: order.retailTotalIdr / order.quantity,
    itemQuantity: order.quantity,
  });
}

/** Buat transaksi Snap untuk isi saldo dompet (order_id = TOPUP-...).
 *  Tidak butuh baris store_orders — webhook nanti merutekan TOPUP- ke dompet. */
export async function createSnapTransactionForTopup(
  topupId: string,
  amountIdr: number,
): Promise<SnapTransaction> {
  return createSnapTransactionRaw({
    orderId: topupId,
    grossAmount: amountIdr,
    itemId: "wallet-topup",
    itemName: "Isi Saldo Unicodes Digital",
    itemPrice: amountIdr,
    itemQuantity: 1,
  });
}

/** Pembuat Snap generik (order produk maupun top-up). */
async function createSnapTransactionRaw(input: {
  orderId: string;
  grossAmount: number;
  itemId: string;
  itemName: string;
  itemPrice: number;
  itemQuantity: number;
}): Promise<SnapTransaction> {
  const { serverKey, snapBase } = config();
  const res = await fetch(`${snapBase}/transactions`, {
    method: "POST",
    headers: {
      "Content-Type": "application/json",
      Authorization: `Basic ${Buffer.from(`${serverKey}:`).toString("base64")}`,
    },
    body: JSON.stringify({
      transaction_details: {
        order_id: input.orderId,
        gross_amount: input.grossAmount,
      },
      item_details: [
        {
          id: input.itemId,
          price: input.itemPrice,
          quantity: input.itemQuantity,
          name: input.itemName,
        },
      ],
      // TODO: isi customer_details dari data pembeli (nama/email/HP) kalau ada.
    }),
  });
  const body = (await res.json().catch(() => ({}))) as Record<string, unknown>;
  if (!res.ok || typeof body.token !== "string") {
    throw new Error(
      `Midtrans Snap gagal: ${JSON.stringify(body).slice(0, 300)}`,
    );
  }
  return {
    token: body.token as string,
    redirectUrl: body.redirect_url as string,
  };
}

/** Verifikasi signature webhook Midtrans:
 *  signature_key = SHA512(order_id + status_code + gross_amount + server_key) */
export function verifyMidtransSignature(input: {
  orderId: string;
  statusCode: string;
  grossAmount: string;
  signatureKey: string;
}): boolean {
  const { serverKey } = config();
  const expected = createHash("sha512")
    .update(
      `${input.orderId}${input.statusCode}${input.grossAmount}${serverKey}`,
    )
    .digest("hex");
  return expected === input.signatureKey.toLowerCase();
}

export interface MidtransStatusCheck {
  transactionStatus: MidtransStatus;
  fraudStatus?: string;
  statusCode: string;
  grossAmount: string;
}

/**
 * Re-konfirmasi status transaksi langsung ke API Midtrans (GET /v2/{order_id}/status).
 * Aturan dari docs Sondev v1.2 (bagian Payments): jangan kredit hanya dari body
 * webhook — webhook memberitahu KAPAN bertanya, API status yang menjawab APAKAH
 * benar-benar lunas. Dipakai handleMidtransNotification sebelum markPaid.
 */
export async function getMidtransStatus(
  orderId: string,
): Promise<MidtransStatusCheck> {
  const { serverKey, isProduction } = config();
  const base = isProduction
    ? "https://api.midtrans.com"
    : "https://api.sandbox.midtrans.com";
  const res = await fetch(`${base}/v2/${encodeURIComponent(orderId)}/status`, {
    headers: {
      Authorization: `Basic ${Buffer.from(`${serverKey}:`).toString("base64")}`,
    },
    signal: AbortSignal.timeout(10000),
  });
  const body = (await res.json().catch(() => ({}))) as Record<string, unknown>;
  if (!res.ok || isMidtransBodyError(body)) {
    throw new Error(
      `Midtrans status API gagal: ${String(body.status_message ?? res.status)}`,
    );
  }
  return {
    transactionStatus: String(body.transaction_status ?? "") as MidtransStatus,
    fraudStatus:
      typeof body.fraud_status === "string" ? body.fraud_status : undefined,
    statusCode: String(body.status_code ?? ""),
    grossAmount: String(body.gross_amount ?? ""),
  };
}

export type MidtransStatus =
  | "capture"
  | "settlement"
  | "pending"
  | "deny"
  | "cancel"
  | "expire"
  | "refund";

/** Refund transaksi Midtrans (dipakai otomatis kalau fulfillment gagal setelah
 *  pembeli bayar). refund_key unik per order supaya retry tidak double-refund.
 *  Hanya untuk transaksi yang sudah settlement/capture — order kita hanya
 *  masuk fulfill setelah lunas, jadi syarat ini terpenuhi. */
export async function refundMidtrans(
  orderId: string,
  amount: number,
): Promise<void> {
  const { serverKey, isProduction } = config();
  const base = isProduction
    ? "https://api.midtrans.com"
    : "https://api.sandbox.midtrans.com";
  const res = await fetch(
    `${base}/v2/${encodeURIComponent(orderId)}/refund`,
    {
      method: "POST",
      headers: {
        "Content-Type": "application/json",
        Authorization: `Basic ${Buffer.from(`${serverKey}:`).toString(
          "base64",
        )}`,
      },
      body: JSON.stringify({
        refund_key: `refund-${orderId}`,
        amount,
        reason: "Fulfillment supplier gagal",
      }),
      signal: AbortSignal.timeout(15000),
    },
  );
  const body = (await res.json().catch(() => ({}))) as Record<
    string,
    unknown
  >;
  if (!res.ok || isMidtransBodyError(body)) {
    throw new Error(
      `Midtrans refund gagal: ${String(body.status_message ?? res.status)}`,
    );
  }
}

/** Proses satu notifikasi webhook Midtrans. Dipanggil route webhook setelah
 *  signature terverifikasi. Settlement/capture = lunas -> markPaid -> fulfill.
 *  fulfillOrder dijalankan TANPA await (polling bisa ~10 menit) supaya webhook
 *  dibalas cepat; status order bisa dipantau via /api/orders/[id].
 *  Sebelum markPaid, status dikonfirmasi ulang ke API Midtrans (aturan docs:
 *  webhook = kapan bertanya, API status = apakah benar lunas). Kalau API
 *  konfirmasi tidak bisa dihubungi, pakai nilai webhook (sudah terverifikasi
 *  signature) dan catat peringatan — jangan sampai pembayaran legit macet.
 *  TOPUP-... = isi saldo dompet (bukan order produk) — dikredit ke dompet,
 *  BUKAN ke store_orders. */
export async function handleMidtransNotification(input: {
  orderId: string;
  transactionStatus: MidtransStatus;
  fraudStatus?: string;
}): Promise<{ handled: boolean; reason: string }> {
  // --- Isi saldo dompet (TOPUP-...) ---
  if (input.orderId.startsWith("TOPUP-")) {
    const { getTopup, markTopupPaid } = await import("../wallet");
    const topup = await getTopup(input.orderId);
    if (!topup) return { handled: false, reason: "top-up tidak ditemukan" };
    if (topup.status === "paid") {
      return { handled: true, reason: "top-up sudah dikredit sebelumnya" };
    }
    let transactionStatus = input.transactionStatus;
    try {
      const confirmed = await getMidtransStatus(input.orderId);
      transactionStatus = confirmed.transactionStatus;
    } catch (err) {
      console.warn(
        `[midtrans] konfirmasi status top-up ${input.orderId} gagal, pakai nilai webhook:`,
        err instanceof Error ? err.message : String(err),
      );
    }
    const paid =
      transactionStatus === "settlement" ||
      (transactionStatus === "capture" && input.fraudStatus !== "deny");
    if (!paid) {
      return { handled: true, reason: `top-up status ${transactionStatus} — menunggu` };
    }
    const { credited } = await markTopupPaid(input.orderId);
    return {
      handled: true,
      reason: credited
        ? `top-up ${input.orderId} dikredit ke dompet`
        : "top-up sudah dikredit sebelumnya",
    };
  }

  const order = await getStoreOrder(input.orderId);
  if (!order) return { handled: false, reason: "order tidak ditemukan" };

  let transactionStatus = input.transactionStatus;
  let fraudStatus = input.fraudStatus;
  try {
    const confirmed = await getMidtransStatus(input.orderId);
    transactionStatus = confirmed.transactionStatus;
    fraudStatus = confirmed.fraudStatus;
  } catch (err) {
    console.warn(
      `[midtrans] konfirmasi status ${input.orderId} gagal, pakai nilai webhook:`,
      err instanceof Error ? err.message : String(err),
    );
  }

  const paid =
    transactionStatus === "settlement" ||
    (transactionStatus === "capture" && fraudStatus !== "deny");
  if (paid) {
    const paidOrder = await markPaid(order.id);
    if (paidOrder.status === "paid") {
      // Reward referral: order berbayar pertama teman yang diundang.
      try {
        const { maybeRewardReferrer, tgIdFromBuyerRef } = await import("../wallet");
        const tgId = tgIdFromBuyerRef(paidOrder.buyerRef);
        if (tgId != null) maybeRewardReferrer(tgId);
      } catch (err) {
        console.error(`[midtrans] reward referral ${order.id} gagal:`, err);
      }
      // Jalan di background — jangan tahan webhook. Setelah selesai,
      // kabari pembeli di Telegram (kalau order dari bot).
      void fulfillOrder(order.id)
        .then(async (done) => {
          const { notifyBuyer } = await import("../notify");
          await notifyBuyer(done);
        })
        .catch((err: unknown) => {
          console.error(`[midtrans] fulfill ${order.id} gagal:`, err);
        });
    }
    return { handled: true, reason: "paid -> fulfill dimulai" };
  }
  if (["deny", "cancel", "expire"].includes(transactionStatus)) {
    return { handled: true, reason: `transaksi ${transactionStatus} — order dibiarkan awaiting_payment (bisa bayar ulang / refund manual)` };
  }
  return { handled: true, reason: `status ${transactionStatus} — menunggu` };
}
