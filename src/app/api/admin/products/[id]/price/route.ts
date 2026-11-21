// Ubah harga retail produk dari web admin (override — harga dasar di
// kode tetap, override disimpan di DB dan dipakai sebagai harga efektif).
// Terima JSON {priceIdr} atau form POST {price} (dari halaman /admin).
import { NextResponse } from "next/server";
import { isAdminRequest } from "@/lib/admin-api";
import { CATALOG } from "@/lib/catalog";
import { setPriceOverride } from "@/lib/product-prices";

function unauthorized() {
  return NextResponse.json(
    { ok: false, error: "Tidak ada akses admin" },
    { status: 401 },
  );
}

async function readPrice(req: Request): Promise<number | null> {
  const ct = req.headers.get("content-type") ?? "";
  if (ct.includes("application/json")) {
    const body = (await req.json().catch(() => ({}))) as { priceIdr?: unknown };
    return typeof body.priceIdr === "number" ? body.priceIdr : null;
  }
  const form = await req.formData();
  const v = form.get("price");
  const n = typeof v === "string" ? Number(v) : NaN;
  return Number.isFinite(n) ? n : null;
}

export async function POST(
  req: Request,
  { params }: { params: Promise<{ id: string }> },
) {
  if (!isAdminRequest(req)) return unauthorized();
  const { id } = await params;
  if (!CATALOG.some((p) => p.id === id)) {
    return NextResponse.json(
      { ok: false, error: "Produk tidak ditemukan" },
      { status: 404 },
    );
  }
  const price = await readPrice(req);
  if (price === null || !Number.isFinite(price)) {
    return NextResponse.json(
      { ok: false, error: "Harga tidak valid" },
      { status: 400 },
    );
  }
  const rounded = Math.floor(price);
  if (rounded < 500 || rounded > 10_000_000) {
    return NextResponse.json(
      { ok: false, error: "Harga harus Rp500 – Rp10.000.000" },
      { status: 400 },
    );
  }
  setPriceOverride(id, rounded);
  const ct = req.headers.get("content-type") ?? "";
  if (ct.includes("form-urlencoded") || ct.includes("multipart/form-data")) {
    return NextResponse.redirect(new URL("/admin", req.url));
  }
  return NextResponse.json({ ok: true, productId: id, priceIdr: rounded });
}
