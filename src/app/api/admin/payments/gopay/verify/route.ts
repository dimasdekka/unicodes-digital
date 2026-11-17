// Panel admin — GoBiz: verifikasi OTP -> simpan sesi.
// POST { otp } -> { ok, session: { authenticated, phone, expiresAt, merchantId } }.
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
    return NextResponse.json({ ok: false, error: "Body harus JSON { otp }" }, { status: 400 });
  }
  const otp = String((body as { otp?: unknown } | null)?.otp ?? "");
  try {
    await gopayClient.verifyOtp(otp);
    return NextResponse.json({ ok: true, session: gopayClient.sessionStatus() });
  } catch (err) {
    return gopayErrorResponse(err);
  }
}
