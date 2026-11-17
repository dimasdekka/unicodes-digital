// Validasi Telegram Web App initData (server-side).
// Algoritma resmi Telegram (https://core.telegram.org/bots/webapps#validating-data-received-via-the-mini-app):
//   1. Ambil semua field initData kecuali `hash`, urutkan alfabetis,
//      gabung jadi data_check_string "key=<value>" dipisah "\n".
//   2. secret_key = HMAC-SHA256(bot_token) dengan key "WebAppData".
//   3. hash = HMAC-SHA256(data_check_string) dengan key secret_key (hex).
//   4. Bandingkan dengan field `hash` (timing-safe) + auth_date maks 1 hari.
//
// Dipakai POST /api/orders saat checkout dari Mini App — initData palsu
// (mis. buyerRef tg orang lain) ditolak 401 di sana.
import { createHmac, timingSafeEqual } from "node:crypto";

export interface TelegramWebAppUser {
  id: number;
  first_name?: string;
  last_name?: string;
  username?: string;
  language_code?: string;
  is_premium?: boolean;
}

export type InitDataValidation =
  | { ok: true; user: TelegramWebAppUser; authDate: number }
  | { ok: false; reason: "bad_init_data" | "missing_hash" | "missing_auth_date" | "expired" | "bad_hash" | "bad_user" };

const MAX_AGE_SECONDS = 24 * 60 * 60; // auth_date tidak lebih dari 1 hari

export function validateInitData(
  initData: string,
  botToken: string,
): InitDataValidation {
  if (!initData || typeof initData !== "string") {
    return { ok: false, reason: "bad_init_data" };
  }
  let params: URLSearchParams;
  try {
    params = new URLSearchParams(initData);
  } catch {
    return { ok: false, reason: "bad_init_data" };
  }
  const hash = params.get("hash");
  if (!hash) return { ok: false, reason: "missing_hash" };
  const authDateRaw = params.get("auth_date");
  const authDate = authDateRaw ? Number(authDateRaw) : NaN;
  if (!Number.isFinite(authDate)) {
    return { ok: false, reason: "missing_auth_date" };
  }
  if (Date.now() / 1000 - authDate > MAX_AGE_SECONDS) {
    return { ok: false, reason: "expired" };
  }
  params.delete("hash");
  const pairs: string[] = [];
  params.forEach((value, key) => pairs.push(`${key}=${value}`));
  pairs.sort();
  const dataCheckString = pairs.join("\n");
  const secretKey = createHmac("sha256", "WebAppData").update(botToken).digest();
  const computed = createHmac("sha256", secretKey)
    .update(dataCheckString)
    .digest("hex");
  let match = false;
  try {
    const a = Buffer.from(computed, "hex");
    const b = Buffer.from(hash, "hex");
    match = a.length === b.length && timingSafeEqual(a, b);
  } catch {
    match = false;
  }
  if (!match) return { ok: false, reason: "bad_hash" };
  let user: TelegramWebAppUser;
  try {
    user = JSON.parse(params.get("user") ?? "") as TelegramWebAppUser;
  } catch {
    return { ok: false, reason: "bad_user" };
  }
  if (!user || typeof user.id !== "number") {
    return { ok: false, reason: "bad_user" };
  }
  return { ok: true, user, authDate };
}
