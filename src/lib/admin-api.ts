// Helper auth untuk API route admin: baca cookie dari header Cookie mentah
// (bukan next/headers) supaya handler bisa diuji langsung di luar runtime Next.
// Tanpa cookie admin yang valid -> pemanggil harus menjawab 401.
import { ADMIN_COOKIE, isAdminCookieValid } from "@/lib/admin-auth";

export function isAdminRequest(req: Request): boolean {
  const header = req.headers.get("cookie") ?? "";
  const cookies = Object.fromEntries(
    header.split(";").map((part) => {
      const i = part.indexOf("=");
      if (i === -1) return [part.trim(), ""];
      return [part.slice(0, i).trim(), part.slice(i + 1).trim()];
    }),
  );
  return isAdminCookieValid(cookies[ADMIN_COOKIE]);
}
