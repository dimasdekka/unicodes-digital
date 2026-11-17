import { NextResponse } from "next/server";
import {
  ADMIN_COOKIE,
  adminPasswordHash,
  hashPassword,
} from "@/lib/admin-auth";

/** Login admin: form POST {password} -> cookie httpOnly -> redirect /admin.
 *  Password dibandingkan sebagai sha256 (tanpa menyimpan plaintext di cookie). */
export async function POST(req: Request) {
  const expected = adminPasswordHash();
  if (!expected) {
    return NextResponse.json(
      { ok: false, error: "ADMIN_PASSWORD belum diisi di env" },
      { status: 500 },
    );
  }
  const form = await req.formData();
  const password = String(form.get("password") ?? "");
  if (hashPassword(password) !== expected) {
    return NextResponse.redirect(new URL("/admin?error=1", req.url));
  }
  const res = NextResponse.redirect(new URL("/admin", req.url));
  res.cookies.set(ADMIN_COOKIE, expected, {
    httpOnly: true,
    path: "/",
    maxAge: 7 * 24 * 3600,
    sameSite: "lax",
  });
  return res;
}
