// Panel admin — QRIS GoPay: daftar pembayaran QRIS terbaru.
// GET -> { payments: [{ reference, amount, unique_amount, status, created_at }] }
import { NextResponse } from "next/server";
import { isAdminRequest } from "@/lib/admin-api";
import { listRecentQrisPayments } from "@/lib/payments/qris-settings";

export async function GET(req: Request) {
  if (!isAdminRequest(req)) {
    return NextResponse.json(
      { ok: false, error: "Tidak ada akses admin" },
      { status: 401 },
    );
  }
  const rows = listRecentQrisPayments(20);
  return NextResponse.json({
    payments: rows.map((r) => ({
      reference: r.reference,
      amount: r.amount_idr,
      unique_amount: r.unique_amount_idr,
      status: r.status,
      created_at: r.created_at,
    })),
  });
}
