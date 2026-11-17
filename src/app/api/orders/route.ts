import { NextResponse } from "next/server";
import { createStoreOrder } from "@/lib/orders";
import { createSnapTransaction } from "@/lib/payments/midtrans";

/**
 * POST /api/orders — buat order + transaksi Midtrans Snap.
 * Body JSON: { productId, quantity?, buyerRef?, xiaomiAccounts?, sheeridUrl? }
 * Balasan: { ok, order: { id, status, totalIdr }, payment: { token, redirectUrl } }
 * Pembeli diarahkan ke redirectUrl untuk bayar (QRIS/e-wallet/VA/dll).
 * Setelah lunas, webhook Midtrans (/api/webhooks/midtrans) akan memanggil
 * markPaid -> fulfillOrder otomatis.
 */
export async function POST(req: Request) {
  try {
    const body = (await req.json().catch(() => ({}))) as Record<string, unknown>;
    const productId = String(body.productId ?? "");
    const quantity =
      typeof body.quantity === "number" && body.quantity > 0
        ? Math.floor(body.quantity)
        : 1;
    const order = await createStoreOrder({
      catalogProductId: productId,
      quantity,
      buyerRef:
        typeof body.buyerRef === "string" ? body.buyerRef : "web",
      xiaomiAccounts: Array.isArray(body.xiaomiAccounts)
        ? (body.xiaomiAccounts as unknown[]).map(String)
        : undefined,
      sheeridUrl:
        typeof body.sheeridUrl === "string" ? body.sheeridUrl : null,
      // Sondev: target WAJIB untuk produk on-demand (kode invite / email).
      target: typeof body.target === "string" ? body.target.trim() : null,
    });
    const payment = await createSnapTransaction(order.id);
    return NextResponse.json(
      {
        ok: true,
        order: { id: order.id, status: order.status, totalIdr: order.retailTotalIdr },
        payment,
      },
      { status: 201 },
    );
  } catch (err) {
    return NextResponse.json(
      { ok: false, error: err instanceof Error ? err.message : String(err) },
      { status: 400 },
    );
  }
}
