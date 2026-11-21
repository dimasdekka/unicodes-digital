// POST /api/webhooks/midtrans — notifikasi pembayaran Midtrans.
// Verifikasi signature SHA512; lalu proses via processMidtransResult
// (jalur yang sama dengan poller — idempotent). Selalu jawab 200 cepat
// (kecuali signature tidak valid -> 401, atau key belum dikonfigurasi -> 500).
// Webhook butuh URL publik — tanpa hosting, deteksi jalan lewat polling
// status tiap 30 detik (midtrans-poller.ts).
import { NextResponse } from "next/server";
import { midtransConfigured } from "@/lib/config";
import { verifyNotificationSignature } from "@/lib/payments/midtrans";
import { processMidtransResult } from "@/lib/payments/midtrans-poller";

export async function POST(req: Request) {
  try {
    if (!midtransConfigured()) {
      return NextResponse.json(
        { ok: false, error: "Midtrans belum dikonfigurasi." },
        { status: 500 },
      );
    }
    const body = (await req.json().catch(() => ({}))) as Record<
      string,
      unknown
    >;
    const orderId =
      typeof body.order_id === "string" ? body.order_id : "";
    const statusCode =
      typeof body.status_code === "string" ? body.status_code : "";
    const grossAmount =
      typeof body.gross_amount === "string"
        ? body.gross_amount
        : String(body.gross_amount ?? "");
    const signatureKey =
      typeof body.signature_key === "string" ? body.signature_key : "";
    const txStatus =
      typeof body.transaction_status === "string"
        ? body.transaction_status
        : "";
    if (!orderId || !statusCode || !grossAmount || !signatureKey || !txStatus) {
      return NextResponse.json(
        { ok: false, error: "Body notifikasi tidak lengkap." },
        { status: 400 },
      );
    }
    const valid = verifyNotificationSignature({
      order_id: orderId,
      status_code: statusCode,
      gross_amount: grossAmount,
      signature_key: signatureKey,
    });
    if (!valid) {
      return NextResponse.json(
        { ok: false, error: "Signature tidak valid." },
        { status: 401 },
      );
    }
    await processMidtransResult(orderId, txStatus);
    return NextResponse.json({ ok: true });
  } catch (err) {
    console.error("[webhook/midtrans]", err);
    return NextResponse.json(
      { ok: false, error: "Webhook gagal diproses." },
      { status: 500 },
    );
  }
}
