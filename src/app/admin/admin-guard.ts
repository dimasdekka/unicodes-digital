// Guard halaman admin (Server Component): baca cookie lewat next/headers.
import { cookies } from "next/headers";

import { ADMIN_COOKIE, isAdminCookieValid } from "@/lib/admin-auth";

export async function isAdminPageAuthed(): Promise<boolean> {
  const jar = await cookies();
  return isAdminCookieValid(jar.get(ADMIN_COOKIE)?.value);
}
