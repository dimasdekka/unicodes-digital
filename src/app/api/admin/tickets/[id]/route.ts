// Detail satu tiket + thread pesannya (JSON).
import { NextResponse } from "next/server";
import { isAdminRequest } from "@/lib/admin-api";
import { getTicket, getTicketMessages } from "@/lib/support-tickets";

function unauthorized() {
  return NextResponse.json(
    { ok: false, error: "Tidak ada akses admin" },
    { status: 401 },
  );
}

export async function GET(
  req: Request,
  { params }: { params: Promise<{ id: string }> },
) {
  if (!isAdminRequest(req)) return unauthorized();
  const { id } = await params;
  const ticket = getTicket(id);
  if (!ticket) {
    return NextResponse.json(
      { ok: false, error: "Tiket tidak ditemukan" },
      { status: 404 },
    );
  }
  return NextResponse.json({
    ok: true,
    ticket,
    messages: getTicketMessages(id),
  });
}
