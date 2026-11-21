// Balas tiket: kirim ke user via Bot API Telegram + simpan ke thread.
// Terima JSON {text} (API) atau form POST (dari halaman detail tiket —
// kalau form, redirect kembali ke halaman tiket).
import { NextResponse } from "next/server";
import { isAdminRequest } from "@/lib/admin-api";
import { getTicket, replyToTicket } from "@/lib/support-tickets";

function unauthorized() {
  return NextResponse.json(
    { ok: false, error: "Tidak ada akses admin" },
    { status: 401 },
  );
}

function isFormPost(req: Request): boolean {
  const ct = req.headers.get("content-type") ?? "";
  return ct.includes("form-urlencoded") || ct.includes("multipart/form-data");
}

async function readText(req: Request): Promise<string | null> {
  const ct = req.headers.get("content-type") ?? "";
  if (ct.includes("application/json")) {
    const body = (await req.json().catch(() => ({}))) as { text?: unknown };
    return typeof body.text === "string" ? body.text : null;
  }
  const form = await req.formData();
  const v = form.get("text");
  return typeof v === "string" ? v : null;
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
  const text = await readText(req);
  if (!text || !text.trim()) {
    return NextResponse.json(
      { ok: false, error: "Balasan kosong" },
      { status: 400 },
    );
  }
  const result = await replyToTicket(id, text);
  if (isFormPost(req)) {
    const url = new URL(`/admin/tickets/${id}`, req.url);
    if (!result.ok) url.searchParams.set("error", result.error ?? "gagal");
    return NextResponse.redirect(url);
  }
  if (!result.ok) {
    // Balasan tetap tersimpan di thread (lihat replyToTicket) — yang gagal
    // hanya kirim ke Telegram. Beri tahu admin supaya bisa kirim ulang.
    return NextResponse.json(
      { ok: false, error: result.error ?? "Gagal mengirim ke Telegram" },
      { status: 502 },
    );
  }
  return NextResponse.json({ ok: true, ticketId: id });
}
