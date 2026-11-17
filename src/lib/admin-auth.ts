// Login admin sederhana (cookie): password dari env ADMIN_PASSWORD.
// Cookie berisi sha256(password) — httpOnly. Tanpa ADMIN_PASSWORD di env,
// halaman admin tidak bisa dibuka siapa pun (fail closed).
import { createHash } from "node:crypto";
import { adminPassword } from "./config";

export const ADMIN_COOKIE = "unicodes_admin";

export function hashPassword(password: string): string {
  return createHash("sha256").update(password).digest("hex");
}

/** sha256(ADMIN_PASSWORD), atau null kalau env belum diisi. */
export function adminPasswordHash(): string | null {
  const pw = adminPassword();
  if (!pw) return null;
  return hashPassword(pw);
}

export function isAdminCookieValid(value: string | undefined): boolean {
  if (!value) return false;
  const expected = adminPasswordHash();
  if (!expected) return false;
  return value === expected;
}
