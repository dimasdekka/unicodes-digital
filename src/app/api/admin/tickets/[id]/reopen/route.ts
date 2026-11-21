// Buka lagi tiket yang sudah ditutup.
import { NextResponse } from "next/server";
import { isAdminRequest } from "@/lib/admin-api";
import { getTicket, reopenTicket } from "@/lib/support-tickets";

function unauthorized() {
  return NextResponse.json(
    { ok: false, error: "Tidak ada akses admin" },
    { status: 401 },
  );
}

export async function POST(
  req: Request,
  { params }: { params: Promise<{ id: string }> },
) {
  if (!isAdminRequest(req)) return unauthorized();
  const { id } = await params;
  if (!getTicket(id)) {
    return NextResponse.json(
      { ok: false, error: "Tiket tidak ditemukan" },
      { status: 404 },
    );
  }
  reopenTicket(id);
  const ct = req.headers.get("content-type") ?? "";
  if (ct.includes("form-urlencoded") || ct.includes("multipart/form-data")) {
    return NextResponse.redirect(new URL(`/admin/tickets/${id}`, req.url));
  }
  return NextResponse.json({ ok: true, ticketId: id });
}
