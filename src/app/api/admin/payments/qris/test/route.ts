// Panel admin — QRIS GoPay: buat QR tes sungguhan.
// GET ?amount=1500 -> { qr_png_base64, qr_string, unique_amount, expires_at,
//   reference, amount_idr }.
// QR ini bisa di-scan dari e-wallet untuk melihat nama merchant + nominal
// sebelum bayar — Dim boleh membatalkannya tanpa membayar (kedaluwarsa 5 menit).
import { NextResponse } from "next/server";
import { isAdminRequest } from "@/lib/admin-api";
import {
  createQrisPayment,
  renderQrisPng,
} from "@/lib/payments/qris";
import { getGopayStaticQris } from "@/lib/payments/qris-settings";

const MIN_AMOUNT = 100;
const MAX_AMOUNT = 100_000_000;

export async function GET(req: Request) {
  if (!isAdminRequest(req)) {
    return NextResponse.json(
      { ok: false, error: "Tidak ada akses admin" },
      { status: 401 },
    );
  }
  const staticQris = getGopayStaticQris();
  if (!staticQris) {
    return NextResponse.json(
      {
        ok: false,
        error:
          "QRIS GoPay belum dipasang — tempel dulu string QRIS statis dari GoBiz.",
      },
      { status: 400 },
    );
  }
  const amount = Number(new URL(req.url).searchParams.get("amount") ?? 1500);
  if (!Number.isInteger(amount) || amount < MIN_AMOUNT || amount > MAX_AMOUNT) {
    return NextResponse.json(
      {
        ok: false,
        error: `Nominal harus bilangan bulat ${MIN_AMOUNT}..${MAX_AMOUNT} rupiah`,
      },
      { status: 400 },
    );
  }
  try {
    const { payment, qrString } = createQrisPayment({
      reference: `ADMIN-TEST-${Date.now()}`,
      amountIdr: amount,
      staticPayload: staticQris,
    });
    const png = await renderQrisPng(qrString);
    return NextResponse.json({
      qr_png_base64: png.toString("base64"),
      qr_string: qrString,
      unique_amount: payment.unique_amount_idr,
      expires_at: payment.expires_at,
      reference: payment.reference,
      amount_idr: payment.amount_idr,
    });
  } catch (err) {
    return NextResponse.json(
      {
        ok: false,
        error:
          err instanceof Error ? err.message : "Gagal membuat QR tes",
      },
      { status: 400 },
    );
  }
}
