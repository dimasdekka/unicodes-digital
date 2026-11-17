// Client GoBiz / GoPay — auth OTP + profil merchant + feed transaksi.
// Ditulis dari pola library Go paygateme (ATM: amati, tiru, modifikasi) —
// BUKAN salinan kode: endpoint + bentuk request/response diringkas dari
// gopay/*.go, implementasi HTTP-nya TypeScript sendiri.
//
// ⚠️ STATUS REFERENSI: provider GoPay di paygateme dinyatakan
// "in development — untested against the live GoBiz API". Semua endpoint
// di bawah ini adalah HIPOTESIS yang belum terverifikasi live. Karena itu
// modul ini dibangun defensif:
//   - setiap request punya timeout (20 detik),
//   - error dibedakan jelas (auth / captcha / api / http),
//   - 401 -> refresh sekali -> retry sekali, lalu menyerah dengan
//     AuthRequired (bukan retry tanpa henti),
//   - sesi kedaluwarsa terdeteksi SEBELUM request (refresh proaktif 5 menit),
//   - token TIDAK PERNAH ditulis ke log / dikembalikan ke client.
// Kalau GoBiz menolak endpoint ini di live-check, perbaiki mapping-nya di
// sini (satu tempat), bukan di pemanggil.
//
// Sesi disimpan di tabel settings (key gopay_session) — satu baris upsert,
// jadi tulisnya atomik. OTP context (otp_token) disimpan di settings juga
// (key gopay_otp), OTP-nya sendiri TIDAK PERNAH disimpan.
import { getSetting, setSetting } from "../db";
import { gobizPhone } from "../config";
import { getGopayStaticQris } from "./qris-settings";
import type { FeedTransaction } from "./qris";

/** Base URL GoID (login/token) — dari komentar konstanta paygateme. */
export const GOID_BASE_URL = "https://api.gojekapi.com";
/** Base URL GoBiz (merchant/feed) — dari komentar konstanta paygateme. */
export const GOBIZ_BASE_URL = "https://api.gobiz.co.id";
/** Timeout tiap request HTTP (paygateme: 20 detik). */
export const GOPAY_REQUEST_TIMEOUT_MS = 20_000;
/** Refresh proaktif 5 menit sebelum access token kedaluwarsa. */
export const GOPAY_REFRESH_BEFORE_EXPIRY_MS = 5 * 60_000;
/** OTP context berlaku 10 menit. */
export const GOPAY_OTP_TTL_MS = 10 * 60_000;
/** Jendela feed default: 1 jam ke belakang (cukup untuk QR 5 menit +
 *  transaksi yang terindeks telat; matcher tetap memvalidasi waktunya). */
export const GOPAY_FEED_LOOKBACK_MS = 60 * 60_000;
/** Ukuran halaman feed maksimum (server menolak >100 dengan 422). */
export const GOPAY_FEED_PAGE_SIZE = 100;
/** Batas halaman per tick (paygateme: MaxTransactionPagesPerTick = 10). */
export const GOPAY_FEED_MAX_PAGES = 10;
/** Fallback kedaluwarsa access token kalau response + JWT tidak memberi tahu. */
export const GOPAY_TOKEN_FALLBACK_TTL_MS = 30 * 60_000;

// ---------------------------------------------------------------------------
// Error
// ---------------------------------------------------------------------------

export type GopayErrorCode =
  | "auth_required" // sesi habis / refresh ditolak -> login OTP ulang
  | "auth_failed" // OTP salah / token response tidak berisi access token
  | "captcha_required" // server meminta captcha (belum ada jalur GoPay-nya)
  | "api_error" // GoBiz/GoID menjawab error bisnis
  | "http_error" // HTTP gagal (jaringan / status tak terduga)
  | "config_invalid"; // input tidak valid (nomor HP / OTP kosong)

/** Error GoBiz dengan kode yang bisa dicek pemanggil (tanpa bocor token). */
export class GopayError extends Error {
  readonly code: GopayErrorCode;
  readonly status?: number;
  constructor(code: GopayErrorCode, message: string, status?: number) {
    super(message);
    this.name = "GopayError";
    this.code = code;
    this.status = status;
  }
}

export function isAuthRequired(err: unknown): boolean {
  return err instanceof GopayError && err.code === "auth_required";
}

// ---------------------------------------------------------------------------
// Sesi
// ---------------------------------------------------------------------------

const SESSION_KEY = "gopay_session";
const OTP_KEY = "gopay_otp";

export interface GopaySession {
  accessToken: string;
  refreshToken: string;
  tokenType: string;
  /** Epoch ms kapan access token kedaluwarsa. */
  expiresAt: number;
  /** Nomor HP (ternormalisasi, tanpa 62/0 depan) yang dipakai login. */
  phone: string;
  /** merchant_id GoBiz (diisi setelah profil pertama kali dibaca). */
  merchantId?: string;
}

interface OtpContext {
  otpToken: string;
  phone: string;
  createdAt: number;
}

/** Normalisasi nomor HP Indonesia: hanya digit, buang awalan 62 lalu 0. */
export function normalizePhone(raw: string): string {
  let digits = (raw ?? "").replace(/\D/g, "");
  if (digits.startsWith("62")) digits = digits.slice(2);
  if (digits.startsWith("0")) digits = digits.slice(1);
  return digits;
}

function validPhone(digits: string): boolean {
  return /^8\d{8,12}$/.test(digits);
}

/** Ambil klaim exp (detik) dari JWT access token, null kalau bukan JWT. */
function jwtExpiryMs(accessToken: string): number | null {
  try {
    const parts = accessToken.split(".");
    if (parts.length !== 3) return null;
    const payload = JSON.parse(
      Buffer.from(parts[1].replace(/-/g, "+").replace(/_/g, "/"), "base64").toString("utf8"),
    ) as { exp?: unknown };
    if (typeof payload.exp !== "number") return null;
    return payload.exp * 1000;
  } catch {
    return null;
  }
}

const STATIC_HEADERS: Record<string, string> = {
  Accept: "application/json, text/plain, */*",
  "Authentication-Type": "go-id",
  "X-PhoneMake": "Web",
  "X-PhoneModel": "Node.js Client",
  "x-DeviceOS": "Web",
  "X-User-Locale": "id",
  "Gojek-Country-Code": "ID",
  "Gojek-Timezone": "Asia/Jakarta",
  "X-Platform": "Web",
  "X-User-Type": "merchant",
  "x-appId": "go-biz-web-dashboard",
};

const GOPAY_CLIENT_ID = "go-biz-web-new";

// ---------------------------------------------------------------------------
// Client
// ---------------------------------------------------------------------------

export interface GopayClientOptions {
  fetchImpl?: typeof fetch;
  now?: () => number;
  goidBase?: string;
  gobizBase?: string;
}

export interface GopayOutlet {
  popId: string;
  name: string;
  status: string;
  receiverId: string | null;
  /** String QRIS statis outlet (aspi_qr_string). */
  qrString: string | null;
}

export interface GopayMerchantProfile {
  id: string;
  merchantName: string | null;
  outletName: string | null;
  phone: string | null;
  email: string | null;
  outlets: GopayOutlet[];
}

export class GopayClient {
  private readonly fetchImpl: typeof fetch;
  private readonly now: () => number;
  private readonly goidBase: string;
  private readonly gobizBase: string;

  constructor(opts: GopayClientOptions = {}) {
    this.fetchImpl = opts.fetchImpl ?? globalThis.fetch;
    this.now = opts.now ?? Date.now;
    this.goidBase = (opts.goidBase ?? GOID_BASE_URL).replace(/\/+$/, "");
    this.gobizBase = (opts.gobizBase ?? GOBIZ_BASE_URL).replace(/\/+$/, "");
  }

  // ------------------------------------------------------------ sesi ---

  getSession(): GopaySession | null {
    const raw = getSetting(SESSION_KEY);
    if (!raw) return null;
    try {
      const s = JSON.parse(raw) as Partial<GopaySession>;
      if (
        typeof s.accessToken !== "string" ||
        !s.accessToken ||
        typeof s.refreshToken !== "string" ||
        typeof s.expiresAt !== "number"
      ) {
        return null;
      }
      return {
        accessToken: s.accessToken,
        refreshToken: s.refreshToken,
        tokenType: typeof s.tokenType === "string" ? s.tokenType : "Bearer",
        expiresAt: s.expiresAt,
        phone: typeof s.phone === "string" ? s.phone : "",
        merchantId: typeof s.merchantId === "string" ? s.merchantId : undefined,
      };
    } catch {
      return null;
    }
  }

  saveSession(session: GopaySession): void {
    setSetting(SESSION_KEY, JSON.stringify(session));
  }

  clearSession(): void {
    setSetting(SESSION_KEY, "");
  }

  /** Sesi ada DAN access token masih berlaku (di luar margin refresh). */
  isAuthenticated(nowMs?: number): boolean {
    const s = this.getSession();
    if (!s) return false;
    const now = nowMs ?? this.now();
    return s.expiresAt > now + GOPAY_REFRESH_BEFORE_EXPIRY_MS;
  }

  /** Status sesi untuk panel admin (TANPA token). */
  sessionStatus(): {
    authenticated: boolean;
    phone: string | null;
    expiresAt: string | null;
    merchantId: string | null;
  } {
    const s = this.getSession();
    if (!s) {
      return { authenticated: false, phone: null, expiresAt: null, merchantId: null };
    }
    return {
      authenticated: this.isAuthenticated(),
      phone: s.phone || null,
      expiresAt: new Date(s.expiresAt).toISOString(),
      merchantId: s.merchantId ?? null,
    };
  }

  // ------------------------------------------------------------ http ---

  private async call(input: {
    base: "goid" | "gobiz";
    path: string;
    method?: string;
    body?: unknown;
    /** Pakai Authorization Bearer (false untuk endpoint auth). */
    auth?: boolean;
    /** Sudah retry setelah refresh (internal — jangan diisi pemanggil). */
    _retried?: boolean;
  }): Promise<unknown> {
    const base = input.base === "goid" ? this.goidBase : this.gobizBase;
    const url = `${base}${input.path}`;
    const headers: Record<string, string> = { ...STATIC_HEADERS };
    if (input.body !== undefined) headers["Content-Type"] = "application/json";
    if (input.auth) {
      const session = this.getSession();
      if (!session) {
        throw new GopayError(
          "auth_required",
          "Sesi GoBiz belum ada — login OTP dulu di panel admin.",
        );
      }
      headers["Authorization"] = `${session.tokenType || "Bearer"} ${session.accessToken}`;
    } else {
      // Endpoint auth sengaja mengirim Bearer kosong (pola paygateme).
      headers["Authorization"] = "Bearer";
    }
    let res: Response;
    try {
      res = await this.fetchImpl(url, {
        method: input.method ?? (input.body !== undefined ? "POST" : "GET"),
        headers,
        body: input.body !== undefined ? JSON.stringify(input.body) : undefined,
        signal: AbortSignal.timeout(GOPAY_REQUEST_TIMEOUT_MS),
      });
    } catch (err) {
      throw new GopayError(
        "http_error",
        `GoBiz tidak bisa dihubungi: ${err instanceof Error ? err.message : String(err)}`,
      );
    }
    const body = (await res.json().catch(() => ({}))) as Record<string, unknown>;
    if (res.status === 401 && input.auth && !input._retried) {
      // Token kedaluwarsa di tengah jalan: refresh sekali, ulang sekali.
      try {
        await this.refreshSession();
      } catch (refreshErr) {
        if (refreshErr instanceof GopayError && refreshErr.code === "auth_required") {
          throw refreshErr;
        }
        throw new GopayError(
          "auth_required",
          "Sesi GoBiz kedaluwarsa dan refresh gagal — login OTP ulang di panel admin.",
        );
      }
      return this.call({ ...input, _retried: true });
    }
    if (res.status === 401) {
      throw new GopayError(
        "auth_required",
        "GoBiz menolak sesi ini (401) — login OTP ulang di panel admin.",
        401,
      );
    }
    if (!res.ok) {
      throw new GopayError(
        "http_error",
        `GoBiz menjawab HTTP ${res.status}: ${goIdErrorMessage(body) ?? "tanpa pesan"}`,
        res.status,
      );
    }
    // Sinyal captcha (defensif — paygateme tidak mendokumentasikan jalur
    // captcha GoPay; kalau server memintanya, jangan diam-diam gagal).
    const haystack = JSON.stringify(body).toLowerCase();
    if (haystack.includes("captcha")) {
      throw new GopayError(
        "captcha_required",
        "GoBiz meminta verifikasi captcha — selesaikan di aplikasi GoBiz, lalu login ulang di sini.",
      );
    }
    return body;
  }

  // ------------------------------------------------------------ auth ---

  /** Minta OTP ke nomor HP merchant. Tanpa argumen, pakai nomor default
   *  dari env GOBIZ_PHONE (diisi Dim di .env.local). otp_token disimpan
   *  server-side (settings) — TIDAK dikembalikan ke pemanggil / client. */
  async requestOtp(rawPhone?: string): Promise<{ phone: string; sentAt: string }> {
    const phone = normalizePhone(rawPhone ?? gobizPhone() ?? "");
    if (!validPhone(phone)) {
      throw new GopayError(
        "config_invalid",
        rawPhone
          ? "Nomor HP tidak valid — pakai format 08xx (contoh: 081234567890)."
          : "Nomor HP GoBiz belum diisi — isi GOBIZ_PHONE di .env.local atau ketik nomornya di panel admin.",
      );
    }
    const body = (await this.call({
      base: "goid",
      path: "/goid/login/request",
      body: { client_id: GOPAY_CLIENT_ID, phone_number: phone, country_code: "62" },
      auth: false,
    })) as Record<string, unknown>;
    const envelope = goIdEnvelope(body);
    if (!envelope.success) {
      throw new GopayError("api_error", `Gagal minta OTP: ${envelope.message}`);
    }
    const data = (body.data ?? {}) as Record<string, unknown>;
    const otpToken =
      typeof data.otp_token === "string"
        ? data.otp_token
        : typeof data.token === "string"
          ? data.token
          : "";
    if (!otpToken) {
      throw new GopayError(
        "api_error",
        "GoBiz tidak mengembalikan otp_token — coba lagi sebentar.",
      );
    }
    const ctx: OtpContext = { otpToken, phone, createdAt: this.now() };
    setSetting(OTP_KEY, JSON.stringify(ctx));
    return { phone, sentAt: new Date(ctx.createdAt).toISOString() };
  }

  /** Verifikasi OTP (diketik Dim di panel) -> simpan sesi. */
  async verifyOtp(otp: string): Promise<GopaySession> {
    const clean = (otp ?? "").trim();
    if (!/^\d{4,8}$/.test(clean)) {
      throw new GopayError("config_invalid", "Kode OTP harus 4–8 digit angka.");
    }
    const raw = getSetting(OTP_KEY);
    if (!raw) {
      throw new GopayError(
        "auth_required",
        "Belum ada permintaan OTP — tekan 'Minta OTP' dulu.",
      );
    }
    let ctx: OtpContext;
    try {
      ctx = JSON.parse(raw) as OtpContext;
    } catch {
      throw new GopayError(
        "auth_required",
        "Permintaan OTP rusak — tekan 'Minta OTP' lagi.",
      );
    }
    if (!ctx.otpToken || this.now() - ctx.createdAt > GOPAY_OTP_TTL_MS) {
      throw new GopayError(
        "auth_required",
        "OTP sudah kedaluwarsa (10 menit) — minta OTP baru.",
      );
    }
    const body = (await this.call({
      base: "goid",
      path: "/goid/token",
      body: {
        client_id: GOPAY_CLIENT_ID,
        grant_type: "otp",
        data: { otp: clean, otp_token: ctx.otpToken },
      },
      auth: false,
    })) as Record<string, unknown>;
    const session = sessionFromTokenResponse(body, ctx.phone, this.now());
    this.saveSession(session);
    setSetting(OTP_KEY, "");
    return session;
  }

  /** Tukar refresh token dengan access token baru. Refresh ditolak
   *  (400/401) = sesi mati -> AuthRequired (login OTP ulang). */
  async refreshSession(): Promise<GopaySession> {
    const current = this.getSession();
    if (!current) {
      throw new GopayError(
        "auth_required",
        "Tidak ada sesi GoBiz untuk di-refresh — login OTP dulu.",
      );
    }
    let body: unknown;
    try {
      body = await this.call({
        base: "goid",
        path: "/goid/token",
        body: {
          client_id: GOPAY_CLIENT_ID,
          grant_type: "refresh_token",
          data: { refresh_token: current.refreshToken },
        },
        auth: false,
      });
    } catch (err) {
      if (err instanceof GopayError && (err.status === 400 || err.status === 401)) {
        this.clearSession();
        throw new GopayError(
          "auth_required",
          "Refresh token ditolak GoBiz — login OTP ulang di panel admin.",
        );
      }
      throw err;
    }
    const next = sessionFromTokenResponse(
      body as Record<string, unknown>,
      current.phone,
      this.now(),
    );
    // Response refresh kadang tidak menyertakan refresh_token baru —
    // pertahankan yang lama (pola paygateme).
    if (!next.refreshToken) next.refreshToken = current.refreshToken;
    next.merchantId = current.merchantId;
    this.saveSession(next);
    return next;
  }

  /** Pastikan sesi segar: refresh proaktif kalau masuk margin 5 menit. */
  async ensureFreshSession(): Promise<GopaySession> {
    const s = this.getSession();
    if (!s) {
      throw new GopayError(
        "auth_required",
        "Sesi GoBiz belum ada — login OTP dulu di panel admin.",
      );
    }
    if (s.expiresAt <= this.now() + GOPAY_REFRESH_BEFORE_EXPIRY_MS) {
      return this.refreshSession();
    }
    return s;
  }

  // ---------------------------------------------------------- merchant ---

  /** Profil merchant + cocokkan QRIS statis outlet dengan yang tersimpan
   *  di settings (peringatan kalau beda — QR yang dibuat bot harus sama
   *  dengan outlet yang dicek feed-nya). */
  async getMerchantProfile(): Promise<{
    profile: GopayMerchantProfile;
    staticQrisMatches: boolean;
    warnings: string[];
  }> {
    await this.ensureFreshSession();
    const me = (await this.call({
      base: "gobiz",
      path: "/v1/users/me",
      auth: true,
    })) as Record<string, unknown>;
    const user = (me.user ?? {}) as Record<string, unknown>;
    const merchantId =
      typeof user.merchant_id === "string" ? user.merchant_id : "";
    if (!merchantId) {
      throw new GopayError(
        "api_error",
        "GoBiz tidak mengembalikan merchant_id untuk akun ini.",
      );
    }
    const raw = (await this.call({
      base: "gobiz",
      path: `/v1/merchants/${encodeURIComponent(merchantId)}`,
      auth: true,
    })) as Record<string, unknown>;
    const pops = Array.isArray(raw.pops) ? raw.pops : [];
    const profile: GopayMerchantProfile = {
      id: String(raw.id ?? merchantId),
      merchantName: typeof raw.merchant_name === "string" ? raw.merchant_name : null,
      outletName: typeof raw.outlet_name === "string" ? raw.outlet_name : null,
      phone: typeof raw.phone === "string" ? raw.phone : null,
      email: typeof raw.email === "string" ? raw.email : null,
      outlets: pops.map((p) => {
        const pop = p as Record<string, unknown>;
        const gopay = (pop.gopay ?? {}) as Record<string, unknown>;
        return {
          popId: String(pop.pop_id ?? ""),
          name: typeof pop.name === "string" ? pop.name : "",
          status: typeof pop.status === "string" ? pop.status : "",
          receiverId:
            typeof gopay.gopay_receiver_id === "string"
              ? gopay.gopay_receiver_id
              : null,
          qrString:
            typeof gopay.aspi_qr_string === "string"
              ? gopay.aspi_qr_string
              : null,
        };
      }),
    };
    // Simpan merchant_id ke sesi supaya feed tidak perlu memanggil /me lagi.
    const session = this.getSession();
    if (session) {
      session.merchantId = profile.id;
      this.saveSession(session);
    }
    const stored = getGopayStaticQris();
    const warnings: string[] = [];
    let matches = false;
    if (!stored) {
      warnings.push("QRIS statis belum dipasang di panel admin.");
    } else {
      matches = profile.outlets.some((o) => o.qrString === stored);
      if (!matches) {
        warnings.push(
          "QRIS statis di panel berbeda dari outlet GoBiz — perbarui string QRIS dari aplikasi GoBiz supaya QR yang dibuat bot cocok dengan feed.",
        );
      }
    }
    if (profile.outlets.length === 0) {
      warnings.push("GoBiz tidak mengembalikan outlet untuk merchant ini.");
    }
    return { profile, staticQrisMatches: matches, warnings };
  }

  // ------------------------------------------------------------- feed ---

  /** Transaksi terbaru (jendela lookback, status settlement/capture,
   *  payment qris/gopay). Dinormalisasi ke FeedTransaction (qris.ts):
   *  nominal = gross/100 (satuan minor -> rupiah, harus bulat),
   *  waktu = transaction_time. Transaksi yang nominalnya tidak bulat /
   *  tidak ada id-nya dilewati (tidak akan match). */
  async listFeedTransactions(opts: {
    lookbackMs?: number;
    statuses?: string[];
    paymentTypes?: string[];
  } = {}): Promise<{ transactions: FeedTransaction[]; truncated: boolean }> {
    const session = await this.ensureFreshSession();
    let merchantId = session.merchantId;
    if (!merchantId) {
      const { profile } = await this.getMerchantProfile();
      merchantId = profile.id;
    }
    const lookback = opts.lookbackMs ?? GOPAY_FEED_LOOKBACK_MS;
    const end = new Date(this.now());
    const start = new Date(this.now() - lookback);
    const statuses = opts.statuses ?? ["settlement", "capture"];
    const paymentTypes = opts.paymentTypes ?? ["qris", "gopay"];
    const transactions: FeedTransaction[] = [];
    let truncated = false;
    let from = 0;
    let total = Number.POSITIVE_INFINITY;
    let pages = 0;
    while (from < total && pages < GOPAY_FEED_MAX_PAGES) {
      const q = new URLSearchParams({
        from: String(from),
        size: String(GOPAY_FEED_PAGE_SIZE),
        start_time: start.toISOString(),
        end_time: end.toISOString(),
        merchant_ids: merchantId,
        statuses: statuses.join(","),
        payment_types: paymentTypes.join(","),
      });
      const body = (await this.call({
        base: "gobiz",
        path: `/merchant-analytics/v2/merchants/transactions?${q.toString()}`,
        auth: true,
      })) as Record<string, unknown>;
      const list = Array.isArray(body.transactions) ? body.transactions : [];
      total =
        typeof body.total === "number" ? body.total : from + list.length;
      for (const rawTx of list) {
        const tx = rawTx as Record<string, unknown>;
        const id = String(tx.id ?? "");
        if (!id) continue;
        const rawAmount =
          typeof tx.real_gross_amount === "number"
            ? tx.real_gross_amount
            : typeof tx.gross_amount === "number"
              ? tx.gross_amount
              : NaN;
        const amountIdr = rawAmount / 100;
        if (!Number.isFinite(amountIdr) || !Number.isInteger(amountIdr) || amountIdr <= 0) {
          continue; // bukan rupiah bulat — tidak akan match, lewati
        }
        const timeRaw = tx.transaction_time;
        const time =
          typeof timeRaw === "string" && timeRaw
            ? new Date(timeRaw)
            : null;
        transactions.push({
          id,
          amountIdr,
          time: time && !Number.isNaN(time.getTime()) ? time : null,
          status: typeof tx.transaction_status === "string"
            ? tx.transaction_status.toLowerCase()
            : "",
        });
      }
      from += list.length;
      pages += 1;
      if (list.length === 0) break;
    }
    if (from < total) truncated = true;
    return { transactions, truncated };
  }
}

/** Client default (fetch global + settings SQLite). Dipakai poller + route admin. */
export const gopayClient = new GopayClient();

// ---------------------------------------------------------------------------
// Helper response
// ---------------------------------------------------------------------------

/** Pesan error dari envelope GoID {success, errors[0]} (atau null). */
function goIdErrorMessage(body: Record<string, unknown>): string | null {
  const errors = body.errors;
  if (Array.isArray(errors) && errors.length > 0) {
    const e = errors[0] as Record<string, unknown>;
    const msg =
      typeof e.message === "string"
        ? e.message
        : typeof e.message_title === "string"
          ? e.message_title
          : typeof e.code === "string"
            ? e.code
            : null;
    if (msg) return msg;
  }
  if (typeof body.message === "string") return body.message;
  return null;
}

function goIdEnvelope(body: Record<string, unknown>): {
  success: boolean;
  message: string;
} {
  if (body.success === false) {
    return {
      success: false,
      message: goIdErrorMessage(body) ?? "GoID menolak permintaan",
    };
  }
  return { success: true, message: "" };
}

/** Bangun sesi dari response POST /goid/token (OTP maupun refresh). */
function sessionFromTokenResponse(
  body: Record<string, unknown>,
  phone: string,
  nowMs: number,
): GopaySession {
  const data = (body.data ?? body) as Record<string, unknown>;
  const accessToken =
    typeof data.access_token === "string" ? data.access_token : "";
  if (!accessToken) {
    throw new GopayError(
      "auth_failed",
      "GoID tidak mengembalikan access_token — OTP mungkin salah, coba lagi.",
    );
  }
  const refreshToken =
    typeof data.refresh_token === "string" ? data.refresh_token : "";
  const expiresIn =
    typeof data.expires_in === "number" ? data.expires_in : null;
  let expiresAt: number;
  if (expiresIn != null && Number.isFinite(expiresIn) && expiresIn > 0) {
    expiresAt = nowMs + expiresIn * 1000;
  } else {
    expiresAt =
      jwtExpiryMs(accessToken) ?? nowMs + GOPAY_TOKEN_FALLBACK_TTL_MS;
  }
  return {
    accessToken,
    refreshToken,
    tokenType:
      typeof data.token_type === "string" ? data.token_type : "Bearer",
    expiresAt,
    phone,
  };
}
