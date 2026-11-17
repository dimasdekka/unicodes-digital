// Panel admin — GoBiz: jalankan satu tick polling feed secara manual.
// POST -> { ok, checked, matched, expired } atau { ok: false, error, code }.
// Dipakai tombol "Cek pembayaran masuk" + debugging tanpa menunggu interval.
import { NextResponse } from "next/server";
import { isAdminRequest } from "@/lib/admin-api";
import { tickQrisPayments } from "@/lib/payments/qris-poller";

export async function POST(req: Request) {
  if (!isAdminRequest(req)) {
    return NextResponse.json({ ok: false, error: "Tidak ada akses admin" }, { status: 401 });
  }
  const result = await tickQrisPayments();
  if (!result.ok) {
    return NextResponse.json(result, { status: 401 });
  }
  return NextResponse.json(result);
}
