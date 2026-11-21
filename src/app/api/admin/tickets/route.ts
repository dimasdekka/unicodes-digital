// Daftar tiket support (JSON). Auth: cookie admin -> 401 tanpa cookie.
import { NextResponse } from "next/server";
import { isAdminRequest } from "@/lib/admin-api";
import { countOpenTickets, listTickets } from "@/lib/support-tickets";

function unauthorized() {
  return NextResponse.json(
    { ok: false, error: "Tidak ada akses admin" },
    { status: 401 },
  );
}

export async function GET(req: Request) {
  if (!isAdminRequest(req)) return unauthorized();
  const limit = Math.min(
    Math.max(Number(new URL(req.url).searchParams.get("limit") ?? 50) || 50, 1),
    200,
  );
  return NextResponse.json({
    ok: true,
    openCount: countOpenTickets(),
    tickets: listTickets(limit),
  });
}
