// Panel admin — GoBiz: profil merchant + cocokkan QRIS statis outlet.
// GET -> { ok, profile, staticQrisMatches, warnings }.
import { NextResponse } from "next/server";
import { isAdminRequest } from "@/lib/admin-api";
import { gopayClient } from "@/lib/payments/gopay";
import { gopayErrorResponse } from "../helpers";

export async function GET(req: Request) {
  if (!isAdminRequest(req)) {
    return NextResponse.json({ ok: false, error: "Tidak ada akses admin" }, { status: 401 });
  }
  try {
    const res = await gopayClient.getMerchantProfile();
    return NextResponse.json({ ok: true, ...res });
  } catch (err) {
    return gopayErrorResponse(err);
  }
}
