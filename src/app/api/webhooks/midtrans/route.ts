import { NextResponse } from "next/server";
import {
  handleMidtransNotification,
  verifyMidtransSignature,
  type MidtransStatus,
} from "@/lib/payments/midtrans";

/**
 * POST /api/webhooks/midtrans — notifikasi pembayaran dari Midtrans.
 * Wajib didaftarkan di dashboard Midtrans (Settings -> Configuration ->
 * Payment Notification URL) dengan URL publik ini.
 * Verifikasi signature dulu, baru proses. Settlement/capture = lunas ->
 * markPaid -> fulfillOrder otomatis (dijalankan di background).
 */
export async function POST(req: Request) {
  try {
    const body = (await req.json().catch(() => ({}))) as Record<
      string,
      unknown
    >;
    const orderId = String(body.order_id ?? "");
    const statusCode = String(body.status_code ?? "");
    const grossAmount = String(body.gross_amount ?? "");
    const signatureKey = String(body.signature_key ?? "");
    const transactionStatus = String(
      body.transaction_status ?? "",
    ) as MidtransStatus;
    const fraudStatus =
      typeof body.fraud_status === "string" ? body.fraud_status : undefined;

    if (!orderId || !signatureKey) {
      return NextResponse.json({ ok: false, error: "payload tidak lengkap" }, { status: 400 });
    }
    const valid = verifyMidtransSignature({
      orderId,
      statusCode,
      grossAmount,
      signatureKey,
    });
    if (!valid) {
      return NextResponse.json({ ok: false, error: "Invalid signature" }, { status: 401 });
    }
    const result = await handleMidtransNotification({
      orderId,
      transactionStatus,
      fraudStatus,
    });
    return NextResponse.json({ ok: true, ...result });
  } catch (err) {
    console.error("[midtrans webhook]", err);
    return NextResponse.json(
      { ok: false, error: err instanceof Error ? err.message : String(err) },
      { status: 500 },
    );
  }
}
