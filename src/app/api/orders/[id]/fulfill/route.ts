import { NextResponse } from "next/server";
import { fulfillOrder, getStoreOrder, markPaid } from "@/lib/orders";

/**
 * POST /api/orders/[id]/fulfill — JALANKAN FULFILLMENT (testing/admin saja).
 * TODO PRODUKSI: lindungi route ini (admin auth) dan panggil HANYA setelah
 * pembayaran terkonfirmasi (poller QRIS -> markPaid -> fulfillOrder).
 * Di sini markPaid dipanggil otomatis supaya bisa dites end-to-end tanpa
 * payment gateway dulu.
 */
export async function POST(
  _req: Request,
  { params }: { params: Promise<{ id: string }> },
) {
  const { id } = await params;
  const order = await getStoreOrder(id);
  if (!order) {
    return NextResponse.json({ ok: false, error: "Order tidak ditemukan" }, { status: 404 });
  }
  try {
    await markPaid(id);
    const done = await fulfillOrder(id);
    return NextResponse.json({ ok: true, order: done });
  } catch (err) {
    return NextResponse.json(
      { ok: false, error: err instanceof Error ? err.message : String(err) },
      { status: 400 },
    );
  }
}
