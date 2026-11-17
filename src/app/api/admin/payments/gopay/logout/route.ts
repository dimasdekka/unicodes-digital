// Panel admin — GoBiz: keluar (hapus sesi tersimpan).
// POST -> { ok: true }. Poller berhenti melunasi sampai login OTP lagi.
import { NextResponse } from "next/server";
import { isAdminRequest } from "@/lib/admin-api";
import { gopayClient } from "@/lib/payments/gopay";

export async function POST(req: Request) {
  if (!isAdminRequest(req)) {
    return NextResponse.json({ ok: false, error: "Tidak ada akses admin" }, { status: 401 });
  }
  gopayClient.clearSession();
  return NextResponse.json({ ok: true });
}
