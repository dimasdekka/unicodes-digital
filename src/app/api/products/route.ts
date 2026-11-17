import { NextResponse } from "next/server";
import { listActiveProducts } from "@/lib/catalog";

/** GET /api/products — katalog gabungan (semua supplier) untuk storefront. */
export async function GET() {
  return NextResponse.json({ ok: true, products: listActiveProducts() });
}
