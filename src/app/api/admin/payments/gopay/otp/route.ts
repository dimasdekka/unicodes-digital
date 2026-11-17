// Panel admin — GoBiz: minta OTP ke nomor HP merchant.
// POST { phone? } -> { ok, phone, sentAt }.
// Tanpa phone, pakai nomor default dari env GOBIZ_PHONE.
// otp_token disimpan server-side (settings) — TIDAK dikembalikan ke client.
// OTP diketik Dim di panel (bukan di chat).
import { NextResponse } from "next/server";
import { isAdminRequest } from "@/lib/admin-api";
import { gopayClient } from "@/lib/payments/gopay";
import { gopayErrorResponse } from "../helpers";

export async function POST(req: Request) {
  if (!isAdminRequest(req)) {
    return NextResponse.json({ ok: false, error: "Tidak ada akses admin" }, { status: 401 });
  }
  let body: unknown;
  try {
    body = await req.json();
  } catch {
    return NextResponse.json({ ok: false, error: "Body harus JSON { phone }" }, { status: 400 });
  }
  const phone = String((body as { phone?: unknown } | null)?.phone ?? "").trim();
  try {
    const res = await gopayClient.requestOtp(phone || undefined);
    return NextResponse.json({ ok: true, ...res });
  } catch (err) {
    return gopayErrorResponse(err);
  }
}
