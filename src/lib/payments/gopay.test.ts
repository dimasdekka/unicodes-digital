// Tes client GoBiz (gopay.ts). Jalankan: npx tsx src/lib/payments/gopay.test.ts
//
// - DATABASE_URL diarahkan ke file /tmp supaya dev.db live TIDAK kotor.
// - TIDAK ada request jaringan: semua respons GoID/GoBiz di-mock.
// - Respons mock mengikuti bentuk yang diringkas dari paygateme
//   (gopay/*.go) — endpoint-nya sendiri belum terverifikasi live, jadi
//   tes ini mengunci PERILAKU client (bukan kebenaran endpoint).
process.env.DATABASE_URL = `file:/tmp/unicodes-gopay-test-${Date.now()}.db`;

import { getSetting } from "../db";
import { setGopayStaticQris } from "./qris-settings";
import {
  GopayClient,
  GopayError,
  normalizePhone,
  GOPAY_OTP_TTL_MS,
} from "./gopay";

// --- Mock fetch ---
interface MockCall {
  url: string;
  method: string;
  body: unknown;
}
type Handler = (call: MockCall) => { status: number; body: unknown } | null;

class MockFetch {
  calls: MockCall[] = [];
  constructor(private handlers: Handler[]) {}
  async fetch(url: unknown, init?: { method?: string; body?: unknown }): Promise<Response> {
    const urlStr = String(url);
    let body: unknown = undefined;
    try {
      body = init?.body ? JSON.parse(String(init.body)) : undefined;
    } catch {
      body = undefined;
    }
    const call: MockCall = { url: urlStr, method: init?.method ?? "GET", body };
    this.calls.push(call);
    for (const h of this.handlers) {
      const res = h(call);
      if (res) {
        return {
          ok: res.status >= 200 && res.status < 300,
          status: res.status,
          json: async () => res.body,
        } as unknown as Response;
      }
    }
    throw new Error(`MOCK: tidak ada handler untuk ${call.method} ${urlStr}`);
  }
}

const jsonOk = (body: unknown) => ({ status: 200, body });
const TOKEN_OK = {
  access_token: "AT-TEST",
  refresh_token: "RT-TEST",
  token_type: "Bearer",
  expires_in: 3600,
};

// --- Runner kecil ---
let passed = 0;
let failed = 0;
const failures: string[] = [];

function check(name: string, ok: boolean, detail?: string): void {
  if (ok) {
    passed++;
    console.log(`PASS  ${name}`);
  } else {
    failed++;
    failures.push(name);
    console.log(`FAIL  ${name}${detail ? ` — ${detail}` : ""}`);
  }
}

async function throwsCode(
  fn: () => Promise<unknown>,
  code: string,
): Promise<boolean> {
  try {
    await fn();
    return false;
  } catch (err) {
    return err instanceof GopayError && err.code === code;
  }
}

async function main(): Promise<void> {
  let nowMs = 1_790_000_000_000;
  const now = () => nowMs;

  // ===== 1. Normalisasi nomor HP =====
  check("phone: 0812... -> 812...", normalizePhone("0812-3456-7890") === "81234567890");
  check("phone: +62 812... -> 812...", normalizePhone("+62 812 3456 7890") === "81234567890");
  check("phone: 62812... -> 812...", normalizePhone("6281234567890") === "81234567890");

  // ===== 2. requestOtp =====
  const otpFetch = new MockFetch([
    (c) =>
      c.url.includes("/goid/login/request")
        ? jsonOk({ success: true, data: { otp_token: "OTP-CTX-1" } })
        : null,
  ]);
  const client = new GopayClient({ fetchImpl: otpFetch.fetch.bind(otpFetch), now });
  const req = await client.requestOtp("081234567890");
  check("otp: request mengembalikan phone ternormalisasi", req.phone === "81234567890");
  const storedCtx = JSON.parse(getSetting("gopay_otp") ?? "{}") as {
    otpToken?: string;
    phone?: string;
  };
  check(
    "otp: otp_token tersimpan server-side (tidak dikembalikan ke pemanggil)",
    storedCtx.otpToken === "OTP-CTX-1" && storedCtx.phone === "81234567890",
  );
  check(
    "otp: nomor tidak valid ditolak config_invalid",
    await throwsCode(() => client.requestOtp("123"), "config_invalid"),
  );

  // Nomor default dari env GOBIZ_PHONE (tanpa argumen).
  process.env.GOBIZ_PHONE = "0819-8765-4321";
  const reqDefault = await client.requestOtp();
  check(
    "otp: tanpa argumen pakai GOBIZ_PHONE",
    reqDefault.phone === "81987654321",
    `phone=${reqDefault.phone}`,
  );
  delete process.env.GOBIZ_PHONE;
  check(
    "otp: tanpa argumen + tanpa GOBIZ_PHONE -> config_invalid",
    await throwsCode(() => client.requestOtp(), "config_invalid"),
  );

  const otpFailFetch = new MockFetch([
    (c) =>
      c.url.includes("/goid/login/request")
        ? jsonOk({
            success: false,
            errors: [{ code: "E1", message: "Nomor belum terdaftar" }],
          })
        : null,
  ]);
  const clientFail = new GopayClient({
    fetchImpl: otpFailFetch.fetch.bind(otpFailFetch),
    now,
  });
  check(
    "otp: envelope gagal -> api_error + pesan GoID",
    await throwsCode(() => clientFail.requestOtp("081234567890"), "api_error"),
  );

  // ===== 3. verifyOtp =====
  const verifyFetch = new MockFetch([
    (c) => (c.url.includes("/goid/token") ? jsonOk(TOKEN_OK) : null),
  ]);
  const clientVerify = new GopayClient({
    fetchImpl: verifyFetch.fetch.bind(verifyFetch),
    now,
  });
  const session = await clientVerify.verifyOtp("123456");
  check("verify: sesi tersimpan + authenticated", clientVerify.isAuthenticated());
  check("verify: expiresAt = now + 3600 detik", session.expiresAt === nowMs + 3600_000);
  check(
    "verify: sessionStatus TIDAK membocorkan token",
    !JSON.stringify(clientVerify.sessionStatus()).includes("AT-TEST"),
  );
  check(
    "verify: OTP format salah ditolak",
    await throwsCode(() => clientVerify.verifyOtp("ab"), "config_invalid"),
  );

  // OTP kedaluwarsa (konteks dibuat ulang, waktu dimajukan).
  await client.requestOtp("081234567890");
  nowMs += GOPAY_OTP_TTL_MS + 1000;
  check(
    "verify: OTP kedaluwarsa -> auth_required",
    await throwsCode(() => client.verifyOtp("123456"), "auth_required"),
  );
  nowMs = 1_790_000_000_000;

  // Token response tanpa access_token -> auth_failed.
  const noTokenFetch = new MockFetch([
    (c) => (c.url.includes("/goid/token") ? jsonOk({ refresh_token: "x" }) : null),
  ]);
  const clientNoToken = new GopayClient({
    fetchImpl: noTokenFetch.fetch.bind(noTokenFetch),
    now,
  });
  await client.requestOtp("081234567890");
  check(
    "verify: tanpa access_token -> auth_failed",
    await throwsCode(() => clientNoToken.verifyOtp("123456"), "auth_failed"),
  );

  // ===== 4. Refresh =====
  // Sesi kedaluwarsa (expiresAt lewat) -> refresh mengembalikan token baru;
  // response tanpa refresh_token baru -> yang lama dipertahankan.
  const refreshFetch = new MockFetch([
    (c) =>
      c.url.includes("/goid/token")
        ? jsonOk({ access_token: "AT-NEW", token_type: "Bearer", expires_in: 1800 })
        : null,
  ]);
  const clientRefresh = new GopayClient({
    fetchImpl: refreshFetch.fetch.bind(refreshFetch),
    now,
  });
  await clientVerify.verifyOtp("123456"); // sesi AT-TEST (mock verifyFetch)
  // Paksa sesi kedaluwarsa dengan memundurkan jam 2 jam.
  nowMs += 2 * 3600_000;
  check("refresh: sesi lama dianggap tidak authenticated", !clientRefresh.isAuthenticated());
  const refreshed = await clientRefresh.refreshSession();
  check("refresh: access token baru dipakai", refreshed.accessToken === "AT-NEW");
  check("refresh: refresh token lama dipertahankan", refreshed.refreshToken === "RT-TEST");
  nowMs = 1_790_000_000_000;

  // Refresh ditolak 401 -> auth_required + sesi dibersihkan.
  const refreshFailFetch = new MockFetch([
    (c) =>
      c.url.includes("/goid/token")
        ? { status: 401, body: { message: "invalid refresh token" } }
        : null,
  ]);
  const clientRefreshFail = new GopayClient({
    fetchImpl: refreshFailFetch.fetch.bind(refreshFailFetch),
    now,
  });
  check(
    "refresh: ditolak 401 -> auth_required",
    await throwsCode(() => clientRefreshFail.refreshSession(), "auth_required"),
  );
  check("refresh: sesi dibersihkan setelah ditolak", clientRefreshFail.getSession() === null);

  // ===== 5. 401 -> refresh sekali -> retry sekali (feed) =====
  let feedCalls = 0;
  const retryFetch = new MockFetch([
    (c) => {
      if (c.url.includes("/merchant-analytics/")) {
        feedCalls++;
        if (feedCalls === 1) return { status: 401, body: { message: "expired" } };
        return jsonOk({ from: 0, size: 100, total: 0, transactions: [] });
      }
      if (c.url.includes("/goid/login/request")) {
        return jsonOk({ success: true, data: { otp_token: "OTP-RETRY" } });
      }
      if (c.url.includes("/v1/users/me")) {
        return jsonOk({ user: { id: "U-9", merchant_id: "M-9" } });
      }
      if (c.url.includes("/v1/merchants/M-9")) {
        return jsonOk({ id: "M-9", merchant_name: "Toko Retry", pops: [] });
      }
      if (c.url.includes("/goid/token")) return jsonOk({ ...TOKEN_OK, access_token: "AT-R2" });
      return null;
    },
  ]);
  const clientRetry = new GopayClient({
    fetchImpl: retryFetch.fetch.bind(retryFetch),
    now,
  });
  // Sesi dulu (OTP + verify) supaya feed punya token untuk di-refresh.
  await clientRetry.requestOtp("081234567890");
  await clientRetry.verifyOtp("123456");
  const feedRes = await clientRetry.listFeedTransactions();
  check(
    "401: feed retry setelah refresh berhasil",
    feedRes.transactions.length === 0 && feedCalls === 2,
    `feedCalls=${feedCalls}`,
  );
  check(
    "401: token hasil refresh dipakai",
    clientRetry.getSession()?.accessToken === "AT-R2",
  );

  // ===== 6. Normalisasi feed =====
  const feedFetch = new MockFetch([
    (c) =>
      c.url.includes("/merchant-analytics/")
        ? jsonOk({
            from: 0,
            size: 100,
            total: 3,
            transactions: [
              {
                id: "TX-1",
                transaction_status: "SETTLEMENT",
                payment_type: "qris",
                gross_amount: 150100,
                currency: "IDR",
                transaction_time: "2026-11-17T14:00:00Z",
              },
              {
                id: "TX-2",
                transaction_status: "capture",
                payment_type: "gopay",
                gross_amount: 999999,
                real_gross_amount: 250000,
                transaction_time: "2026-11-17T14:01:00Z",
              },
              {
                id: "TX-3",
                transaction_status: "settlement",
                gross_amount: 150150, // 1501.5 rupiah -> tidak bulat, lewati
                transaction_time: "2026-11-17T14:02:00Z",
              },
            ],
          })
        : null,
  ]);
  const clientFeed = new GopayClient({
    fetchImpl: feedFetch.fetch.bind(feedFetch),
    now,
  });
  const { transactions } = await clientFeed.listFeedTransactions();
  check("feed: 2 transaksi valid (yang tidak bulat dilewati)", transactions.length === 2);
  check(
    "feed: gross_amount sen -> rupiah (150100 -> 1501)",
    transactions.find((t) => t.id === "TX-1")?.amountIdr === 1501,
  );
  check(
    "feed: real_gross_amount didahulukan (250000 -> 2500)",
    transactions.find((t) => t.id === "TX-2")?.amountIdr === 2500,
  );
  check(
    "feed: status dinormalisasi huruf kecil",
    transactions.find((t) => t.id === "TX-1")?.status === "settlement",
  );

  // ===== 7. Profil merchant + cocok QRIS statis =====
  const STATIC_QRIS = "00020101021126610014COM.GO-JEK.WWW0118936009143535109406";
  setGopayStaticQris(STATIC_QRIS);
  const profileFetch = new MockFetch([
    (c) =>
      c.url.includes("/v1/users/me")
        ? jsonOk({ user: { id: "U-1", merchant_id: "M-123" } })
        : null,
    (c) =>
      c.url.includes("/v1/merchants/M-123")
        ? jsonOk({
            id: "M-123",
            merchant_name: "Unicodes Digital",
            outlet_name: "Outlet Serang",
            phone: "081234567890",
            email: "x@y.z",
            pops: [
              {
                pop_id: "P-1",
                name: "Outlet Serang",
                status: "active",
                gopay: {
                  gopay_receiver_id: "G535109406",
                  gopay_qr_string: "GOPAY-QR",
                  aspi_qr_string: STATIC_QRIS,
                },
              },
            ],
          })
        : null,
  ]);
  const clientProfile = new GopayClient({
    fetchImpl: profileFetch.fetch.bind(profileFetch),
    now,
  });
  const { profile, staticQrisMatches, warnings } =
    await clientProfile.getMerchantProfile();
  check("profil: merchant + outlet terbaca", profile.merchantName === "Unicodes Digital");
  check("profil: qrString diambil dari aspi_qr_string", profile.outlets[0]?.qrString === STATIC_QRIS);
  check("profil: QRIS statis cocok -> matches true", staticQrisMatches === true);
  check("profil: tidak ada warning saat cocok", warnings.length === 0);
  check(
    "profil: merchantId tersimpan ke sesi",
    clientProfile.getSession()?.merchantId === "M-123",
  );

  // QRIS statis beda -> matches false + warning.
  setGopayStaticQris("00020101021126610014LAIN");
  const { staticQrisMatches: m2, warnings: w2 } =
    await clientProfile.getMerchantProfile();
  check("profil: QRIS beda -> matches false", m2 === false);
  check("profil: QRIS beda -> ada warning", w2.length > 0);

  // ===== 8. Captcha =====
  const captchaFetch = new MockFetch([
    (c) =>
      c.url.includes("/merchant-analytics/")
        ? jsonOk({ message: "captcha verification required" })
        : null,
  ]);
  const clientCaptcha = new GopayClient({
    fetchImpl: captchaFetch.fetch.bind(captchaFetch),
    now,
  });
  check(
    "captcha: respons menyebut captcha -> captcha_required",
    await throwsCode(() => clientCaptcha.listFeedTransactions(), "captcha_required"),
  );

  // ===== Ringkasan =====
  console.log(`\n==== ${passed}/${passed + failed} lolos ====`);
  if (failed > 0) {
    console.log("Gagal:", failures.join(", "));
    process.exitCode = 1;
  }
}

void main();
