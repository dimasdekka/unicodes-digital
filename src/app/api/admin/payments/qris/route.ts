// Panel admin — QRIS GoPay: status + simpan string QRIS statis.
// GET  -> { configured, merchant_name, merchant_city, merchant_id }
// POST { static_qris } -> validasi (parse + CRC) lalu simpan ke settings.
// String lengkap TIDAK pernah dikembalikan ke client.
import { NextResponse } from "next/server";
import { isAdminRequest } from "@/lib/admin-api";
import {
  extractMerchantInfo,
  getGopayStaticQris,
  setGopayStaticQris,
  validateStaticQris,
} from "@/lib/payments/qris-settings";

function unauthorized() {
  return NextResponse.json(
    { ok: false, error: "Tidak ada akses admin" },
    { status: 401 },
  );
}

export async function GET(req: Request) {
  if (!isAdminRequest(req)) return unauthorized();
  const stored = getGopayStaticQris();
  if (!stored) {
    return NextResponse.json({
      configured: false,
      merchant_name: null,
      merchant_city: null,
      merchant_id: null,
    });
  }
  try {
    const info = extractMerchantInfo(stored);
    return NextResponse.json({
      configured: true,
      merchant_name: info.merchantName,
      merchant_city: info.merchantCity,
      merchant_id: info.merchantId,
    });
  } catch (err) {
    return NextResponse.json({
      configured: true,
      merchant_name: null,
      merchant_city: null,
      merchant_id: null,
      error:
        err instanceof Error
          ? err.message
          : "String QRIS tersimpan tidak bisa dibaca",
    });
  }
}

export async function POST(req: Request) {
  if (!isAdminRequest(req)) return unauthorized();
  let body: unknown;
  try {
    body = await req.json();
  } catch {
    return NextResponse.json(
      { ok: false, error: "Body harus JSON { static_qris }" },
      { status: 400 },
    );
  }
  const staticQris = String(
    (body as { static_qris?: unknown } | null)?.static_qris ?? "",
  ).trim();
  try {
    const info = validateStaticQris(staticQris);
    setGopayStaticQris(staticQris);
    return NextResponse.json({
      ok: true,
      configured: true,
      merchant_name: info.merchantName,
      merchant_city: info.merchantCity,
      merchant_id: info.merchantId,
    });
  } catch (err) {
    return NextResponse.json(
      {
        ok: false,
        error:
          err instanceof Error ? err.message : "String QRIS tidak valid",
      },
      { status: 400 },
    );
  }
}
