// Helper route admin GoBiz — mapping error GopayError ke respons HTTP.
// Pesan error dikembalikan apa adanya (tidak mengandung token).
import { NextResponse } from "next/server";
import { GopayError } from "@/lib/payments/gopay";

export function gopayErrorResponse(err: unknown): NextResponse {
  if (err instanceof GopayError) {
    const status = err.code === "auth_required" ? 401 : 400;
    return NextResponse.json(
      { ok: false, error: err.message, code: err.code },
      { status },
    );
  }
  return NextResponse.json(
    { ok: false, error: err instanceof Error ? err.message : String(err) },
    { status: 500 },
  );
}
