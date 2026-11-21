// Konfigurasi terpusat — SATU tempat membaca environment variable.
// Aturan: secret TIDAK dibaca tersebar di src/lib / src/bot / route.
// Modul yang butuh secret memanggil getter di sini; kalau belum diisi,
// getter melempar error yang jelas (bukan undefined yang meledak belakangan).
// Nilai di sini TIDAK pernah di-log — pemanggil dilarang mencetak hasilnya.

function required(name: string): string {
  const v = (process.env[name] ?? "").trim();
  if (!v) throw new Error(`${name} belum diisi di .env.local`);
  return v;
}

function optional(name: string): string | undefined {
  const v = (process.env[name] ?? "").trim();
  return v ? v : undefined;
}

/** Token bot Telegram (@BotFather). */
export function telegramBotToken(): string {
  return required("TELEGRAM_BOT_TOKEN");
}

/** Token bot news (@unicodesdigitalbot). Opsional — kosong = posting
 *  channel tetap memakai token bot toko. */
export function telegramNewsBotToken(): string {
  return (process.env.TELEGRAM_NEWS_BOT_TOKEN ?? "").trim();
}

/** ID Telegram admin (dipisah koma di env). */
export function telegramAdminIds(): number[] {
  return (process.env.TELEGRAM_ADMIN_IDS ?? "")
    .split(",")
    .map((s) => Number(s.trim()))
    .filter((n) => Number.isFinite(n) && n > 0);
}

/** ID chat channel komunitas (mis. "-1001234567890"). Kosong = fitur
 *  channel (feed order + wajib join) nonaktif — bot jalan normal. */
export function telegramChannelId(): string {
  return (process.env.TELEGRAM_CHANNEL_ID ?? "").trim();
}

/** Link publik channel komunitas (mis. "https://t.me/unicodesdigital"). */
export function telegramChannelUrl(): string {
  return (process.env.TELEGRAM_CHANNEL_URL ?? "").trim();
}

/** Server Key Midtrans (Basic auth API). Wajib untuk transaksi + cek status. */
export function midtransServerKey(): string {
  return required("MIDTRANS_SERVER_KEY");
}

/** Client Key Midtrans (untuk Snap.js — saat ini tidak dipakai, kami pakai
 *  redirect_url). */
export function midtransClientKey(): string {
  return required("MIDTRANS_CLIENT_KEY");
}

/** true = production, false/absen = sandbox. */
export function midtransIsProduction(): boolean {
  return (process.env.MIDTRANS_IS_PRODUCTION ?? "").trim() === "true";
}

/** Apakah Midtrans terkonfigurasi (server key terisi)? */
export function midtransConfigured(): boolean {
  return (process.env.MIDTRANS_SERVER_KEY ?? "").trim().length > 0;
}

/** Password admin /admin (null kalau belum diisi — halaman admin fail closed). */
export function adminPassword(): string | null {
  return optional("ADMIN_PASSWORD") ?? null;
}

/** URL publik aplikasi (domain deploy). Undefined saat dev lokal. */
export function appUrl(): string | undefined {
  const base = (process.env.NEXT_PUBLIC_APP_URL ?? "").trim().replace(/\/+$/, "");
  return base ? base : undefined;
}

/** File SQLite (default ./dev.db). */
export function databaseUrl(): string {
  return process.env.DATABASE_URL ?? "file:./dev.db";
}

/** Base URL Sondev (tidak pernah throw — default kalau env kosong). */
export function sondevBaseUrl(): string {
  return (
    process.env.SONDEV_BASE_URL ?? "https://api.sondevs.tech/api/v1"
  );
}

/** Kredensial + base URL Sondev. */
export function sondevConfig(): {
  apiKey: string;
  apiSecret: string;
  baseUrl: string;
} {
  return {
    apiKey: required("SONDEV_API_KEY"),
    apiSecret: required("SONDEV_API_SECRET"),
    baseUrl: sondevBaseUrl(),
  };
}

/** Base URL LahaStore (tidak pernah throw — default kalau env kosong). */
export function lahastoreBaseUrl(): string {
  return (
    process.env.LAHASTORE_BASE_URL ??
    "https://lahastore.up.railway.app/api/v1"
  );
}

/** Kredensial + base URL LahaStore. */
export function lahastoreConfig(): { apiKey: string; baseUrl: string } {
  return {
    apiKey: required("LAHASTORE_API_KEY"),
    baseUrl: lahastoreBaseUrl(),
  };
}

/** Nomor HP GoBiz merchant (dari env GOBIZ_PHONE, diisi Dim di .env.local).
 *  Dipakai sebagai default requestOtp — TIDAK PERNAH di-hardcode di code.
 *  Kalau kosong, OTP harus diminta dengan nomor yang diketik di panel. */
export function gobizPhone(): string | undefined {
  const v = (process.env.GOBIZ_PHONE ?? "").trim();
  return v ? v : undefined;
}

/** Interval polling feed GoBiz (ms). Default 30 detik — cukup cepat untuk
 *  QR 5 menit, cukup jarang untuk tidak membanjiri API. */
export function gopayPollIntervalMs(): number {
  const raw = Number(process.env.GOPAY_POLL_INTERVAL_MS ?? "");
  return Number.isFinite(raw) && raw >= 5000 ? Math.floor(raw) : 30000;
}
