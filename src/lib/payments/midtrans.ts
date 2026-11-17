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

/** Buat transaksi Snap untuk satu order toko. Mengembalikan token + URL bayar.
 *  order_id Midtrans = ID order internal kita (UD-...) supaya webhook nanti
 *  langsung ketahuan order mana. */
export async function createSnapTransaction(orderId: string): Promise<SnapTransaction> {
  const { serverKey, snapBase } = config();
  const order = await getStoreOrder(orderId);
  if (!order) throw new Error(`Order tidak ditemukan: ${orderId}`);

  const res = await fetch(`${snapBase}/transactions`, {
    method: "POST",
    headers: {
      "Content-Type": "application/json",
      Authorization: `Basic ${Buffer.from(`${serverKey}:`).toString("base64")}`,
    },
    body: JSON.stringify({
      transaction_details: {
        order_id: order.id,
        gross_amount: order.retailTotalIdr,
      },
      item_details: [
        {
          id: order.catalogProductId,
          price: order.retailTotalIdr / order.quantity,
          quantity: order.quantity,
          name: order.catalogProductId,
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
  if (!res.ok) {
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

/** Proses satu notifikasi webhook Midtrans. Dipanggil route webhook setelah
 *  signature terverifikasi. Settlement/capture = lunas -> markPaid -> fulfill.
 *  fulfillOrder dijalankan TANPA await (polling bisa ~10 menit) supaya webhook
 *  dibalas cepat; status order bisa dipantau via /api/orders/[id].
 *  Sebelum markPaid, status dikonfirmasi ulang ke API Midtrans (aturan docs:
 *  webhook = kapan bertanya, API status = apakah benar lunas). Kalau API
 *  konfirmasi tidak bisa dihubungi, pakai nilai webhook (sudah terverifikasi
 *  signature) dan catat peringatan — jangan sampai pembayaran legit macet. */
export async function handleMidtransNotification(input: {
  orderId: string;
  transactionStatus: MidtransStatus;
  fraudStatus?: string;
}): Promise<{ handled: boolean; reason: string }> {
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
      // Jalan di background — jangan tahan webhook.
      void fulfillOrder(order.id).catch((err: unknown) => {
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
