import { NextResponse } from "next/server";
import { getStoreOrder } from "@/lib/orders";

/** GET /api/orders/[id] — lacak status order (dipakai halaman tracking & bot). */
export async function GET(
  _req: Request,
  { params }: { params: Promise<{ id: string }> },
) {
  const { id } = await params;
  const order = await getStoreOrder(id);
  if (!order) {
    return NextResponse.json({ ok: false, error: "Order tidak ditemukan" }, { status: 404 });
  }
  return NextResponse.json({
    ok: true,
    order: {
      id: order.id,
      product: order.catalogProductId,
      quantity: order.quantity,
      totalIdr: order.retailTotalIdr,
      status: order.status,
      supplier: order.supplier,
      deliveredItems: order.deliveredItems,
      resultLink: order.resultLink,
      error: order.error,
      createdAt: order.createdAt,
      updatedAt: order.updatedAt,
    },
  });
}
