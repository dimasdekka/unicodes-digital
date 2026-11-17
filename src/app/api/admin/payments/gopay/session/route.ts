// Panel admin — GoBiz: status sesi (TANPA token).
// GET -> { authenticated, phone, expiresAt, merchantId, defaultPhone }
import { NextResponse } from "next/server";
import { isAdminRequest } from "@/lib/admin-api";
import { gobizPhone } from "@/lib/config";
import { gopayClient } from "@/lib/payments/gopay";

export async function GET(req: Request) {
  if (!isAdminRequest(req)) {
    return NextResponse.json({ ok: false, error: "Tidak ada akses admin" }, { status: 401 });
  }
  return NextResponse.json({
    ok: true,
    ...gopayClient.sessionStatus(),
    defaultPhone: gobizPhone() ?? null,
  });
}
