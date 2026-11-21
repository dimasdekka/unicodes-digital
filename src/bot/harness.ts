// Harness test bot Telegram Unicodes Digital v2 (+ Web App).
// TANPA pesan Telegram sungguhan: bot.api di-stub (semua send/edit/forward
// direkam), fetch global di-stub (GoPay GoID/GoBiz + Sondev + status API),
// database = file SQLite SEMENTARA di /tmp (dev.db asli TIDAK disentuh).
// Jalankan: npx tsx src/bot/harness.ts
// Keluar 0 = semua lolos; 1 = ada yang gagal (nama + alasan dicetak).
import { createHash, createHmac } from "node:crypto";
import { unlinkSync } from "node:fs";
import { createElement } from "react";
import { renderToString } from "react-dom/server";

// ---- env (SEBELUM import modul apa pun yang membacanya) ----
const TEST_DB = `/tmp/unicodes-harness-${Date.now()}.db`;
process.env.DATABASE_URL = `file:${TEST_DB}`;
process.env.TELEGRAM_BOT_TOKEN = "123456:TESTTOKEN";
process.env.TELEGRAM_ADMIN_IDS = "807719771";
process.env.ADMIN_PASSWORD = "test-admin-pw";
process.env.SONDEV_API_KEY = "test-sondev-key";
process.env.SONDEV_API_SECRET = "test-sondev-secret";
process.env.LAHASTORE_API_KEY = "test-laha-key";

// QRIS statis sintetis (speks EMVCo, BUKAN merchant asli) — dibuat di main()
// dari serializeQrisPayload, lalu disimpan ke settings + dipakai mock GoBiz.
let TEST_STATIC_QRIS = "";

// ---- fetch stub: GoPay + Sondev palsu, sisanya DILARANG ----
const SONDEV_ORDER = {
  status: "success",
  request_id: "test-req",
  data: {
    order: {
      ref_id: "TEST-ORDER",
      status: "DELIVERED",
      delivery_status: "DELIVERED",
      delivered_data: { items: ["MUSE-TOKEN-TEST-1B"], code: "TESTCODE1B" },
    },
  },
};
const SONDEV_PRODUCTS = {
  status: "success",
  data: {
    products: [
      {
        code: "muse_token",
        price_idr: 1000,
        available_stock: "UNLIMITED",
        min_qty: 1,
        max_qty: 30,
      },
    ],
  },
};
// --- Stub kontrol skenario (diubah dari main() untuk verifikasi) ---
const stubControl = {
  sondevBalanceIdr: 100000, // saldo IDR yang dikembalikan GET /profile
  sondevFail: false, // true = semua endpoint Sondev melempar (simulasi API down)
  sondevCreateCalls: 0, // jumlah POST /orders/create yang diterima
  // Transaksi feed GoBiz yang dikembalikan endpoint transactions
  // ({ id, amountIdr (rupiah), timeISO?, status? }).
  feedTransactions: [] as {
    id: string;
    amountIdr: number;
    timeISO?: string;
    status?: string;
  }[],
};

function fakeResponse(body: unknown): Response {
  return {
    ok: true,
    status: 200,
    json: async () => body,
    headers: { get: () => null },
  } as unknown as Response;
}
// Rekaman semua kiriman bot (via Bot API sungguhan grammY -> fetch stub).
interface Call {
  method: string;
  chatId?: number;
  text?: string;
  keyboard?: unknown;
  data?: unknown;
  photo?: string;
}
const calls: Call[] = [];
let midCounter = 1000;
// Parse body request: JSON biasa, FormData, atau stream multipart mentah
// (grammY mengirim foto Buffer sebagai multipart — body-nya Readable berisi
// byte multipart, bukan FormData). Foto dicatat sebagai "[binary]" karena
// isinya tidak perlu dicek di sini; caption/chat_id/reply_markup diekstrak.
async function parseBody(init?: { body?: unknown }): Promise<Record<string, unknown>> {
  const raw = init?.body;
  if (raw == null) return {};
  if (typeof raw === "string") {
    try {
      return JSON.parse(raw) as Record<string, unknown>;
    } catch {
      return {};
    }
  }
  const get = (raw as { get?: unknown }).get;
  if (typeof get === "function") {
    const out: Record<string, unknown> = {};
    for (const [k, v] of (raw as FormData).entries()) {
      if (k === "photo") {
        out.photo = "[binary]";
        continue;
      }
      if (k === "reply_markup" && typeof v === "string") {
        try {
          out.reply_markup = JSON.parse(v);
        } catch {
          out.reply_markup = v;
        }
        continue;
      }
      out[k] = v;
    }
    return out;
  }
  // Stream multipart (Readable): kumpulkan byte, pisah per boundary.
  if (typeof (raw as AsyncIterable<unknown>)[Symbol.asyncIterator] === "function") {
    const chunks: Buffer[] = [];
    for await (const chunk of raw as AsyncIterable<Buffer | string>) {
      chunks.push(Buffer.isBuffer(chunk) ? chunk : Buffer.from(String(chunk)));
    }
    const buf = Buffer.concat(chunks);
    const firstNl = buf.indexOf("\r\n");
    if (firstNl === -1) return {};
    const boundary = buf.subarray(0, firstNl).toString("ascii");
    // latin1: 1 char = 1 byte, jadi slice string -> Buffer latin1 = byte asli.
    const text = buf.toString("latin1");
    const out: Record<string, unknown> = {};
    for (const part of text.split(boundary)) {
      const sep = part.indexOf("\r\n\r\n");
      if (sep === -1) continue;
      const headers = part.slice(0, sep);
      const nameMatch = /name="([^"]+)"/.exec(headers);
      if (!nameMatch) continue;
      const name = nameMatch[1];
      let value = part.slice(sep + 4);
      if (value.endsWith("\r\n")) value = value.slice(0, -2);
      if (value === "--" || value === "--\r\n") continue;
      if (/filename="/.test(headers)) {
        out[name] = "[binary]";
        continue;
      }
      const decoded = Buffer.from(value, "latin1").toString("utf8");
      if (name === "reply_markup") {
        try {
          out[name] = JSON.parse(decoded);
        } catch {
          out[name] = decoded;
        }
      } else {
        out[name] = decoded;
      }
    }
    return out;
  }
  return {};
}
(globalThis as unknown as { fetch: typeof fetch }).fetch = (async (
  input: unknown,
  init?: { body?: unknown },
) => {
  const url = String(input);
  // ---- Bot API Telegram (dipanggil grammY via fixedFetch) ----
  const tgMatch = /api\.telegram\.org\/bot[^/]+\/([A-Za-z]+)$/.exec(url);
  if (tgMatch) {
    const method = tgMatch[1];
    const body = await parseBody(init);
    const chatId =
      typeof body.chat_id === "number"
        ? body.chat_id
        : typeof body.chat_id === "string"
          ? Number(body.chat_id)
          : undefined;
    const text = typeof body.text === "string" ? body.text : undefined;
    if (method === "sendMessage") {
      calls.push({ method, chatId, text, keyboard: body.reply_markup });
      return fakeResponse({
        ok: true,
        result: {
          message_id: ++midCounter,
          chat: { id: chatId, type: "private" },
          text,
          date: Math.floor(Date.now() / 1000),
        },
      });
    }
    if (method === "sendPhoto") {
      // file_id "BAD-FILE-ID" mensimulasikan foto yang dihapus / tidak valid.
      if (body.photo === "BAD-FILE-ID") {
        throw new Error("HARNESS: sendPhoto file_id tidak valid");
      }
      calls.push({
        method,
        chatId,
        photo: typeof body.photo === "string" ? body.photo : undefined,
        text: typeof body.caption === "string" ? body.caption : undefined,
        keyboard: body.reply_markup,
      });
      return fakeResponse({
        ok: true,
        result: {
          message_id: ++midCounter,
          chat: { id: chatId, type: "private" },
          caption: body.caption,
          date: Math.floor(Date.now() / 1000),
        },
      });
    }
    if (method === "setMyCommands") {
      calls.push({ method, data: body.commands });
      return fakeResponse({ ok: true, result: true });
    }
    if (method === "editMessageText") {
      calls.push({
        method,
        chatId,
        text,
        keyboard: body.reply_markup,
        data: body.message_id,
      });
      return fakeResponse({
        ok: true,
        result: {
          message_id: body.message_id,
          chat: { id: chatId, type: "private" },
          text,
          date: Math.floor(Date.now() / 1000),
        },
      });
    }
    if (method === "answerCallbackQuery") {
      calls.push({ method });
      return fakeResponse({ ok: true, result: true });
    }
    if (method === "forwardMessage") {
      calls.push({
        method,
        chatId,
        data: { fromChatId: body.from_chat_id, messageId: body.message_id },
      });
      return fakeResponse({
        ok: true,
        result: {
          message_id: ++midCounter,
          chat: { id: chatId, type: "private" },
          text: "forwarded",
          date: Math.floor(Date.now() / 1000),
        },
      });
    }
    throw new Error(`HARNESS: metode Telegram tak terduga: ${method}`);
  }
  // ---- GoPay GoID: minta OTP + token (mock mengikuti bentuk paygateme) ----
  if (url.includes("api.gojekapi.com/goid/login/request")) {
    return fakeResponse({ success: true, data: { otp_token: "OTP-HARNESS" } });
  }
  if (url.includes("api.gojekapi.com/goid/token")) {
    return fakeResponse({
      access_token: "AT-HARNESS",
      refresh_token: "RT-HARNESS",
      token_type: "Bearer",
      expires_in: 3600,
    });
  }
  // ---- GoPay GoBiz: profil merchant + feed transaksi ----
  if (url.includes("api.gobiz.co.id/v1/users/me")) {
    return fakeResponse({ user: { id: "U-HARNESS", merchant_id: "M-HARNESS" } });
  }
  if (url.includes("api.gobiz.co.id/v1/merchants/")) {
    return fakeResponse({
      id: "M-HARNESS",
      merchant_name: "Unicodes Digital",
      outlet_name: "Outlet Serang",
      phone: "081234567890",
      email: "t@t.t",
      pops: [
        {
          pop_id: "P-1",
          name: "Outlet Serang",
          status: "active",
          gopay: {
            gopay_receiver_id: "G535109406",
            gopay_qr_string: "GOPAY-QR",
            aspi_qr_string: TEST_STATIC_QRIS,
          },
        },
      ],
    });
  }
  if (url.includes("api.gobiz.co.id/merchant-analytics/v2/merchants/transactions")) {
    const list = stubControl.feedTransactions.map((t) => ({
      id: t.id,
      order_id: null,
      merchant_id: "M-HARNESS",
      transaction_status: t.status ?? "settlement",
      payment_type: "qris",
      gross_amount: t.amountIdr * 100,
      real_gross_amount: t.amountIdr * 100,
      currency: "IDR",
      transaction_time: t.timeISO ?? new Date().toISOString(),
      settlement_time: null,
      transaction_source: "qris",
    }));
    return fakeResponse({ from: 0, size: 100, total: list.length, transactions: list });
  }
  if (url.includes("api.sondevs.tech/api/v1/orders/create")) {
    stubControl.sondevCreateCalls += 1;
    if (stubControl.sondevFail) {
      throw new Error("HARNESS: Sondev orders down");
    }
    // Order dengan ref_id UD-FAIL-* gagal di supplier (simulasi fulfillment
    // gagal) — dipakai S27 untuk jalur refund otomatis.
    let reqBody: Record<string, unknown> = {};
    try {
      reqBody = JSON.parse(String(init?.body ?? "{}")) as Record<
        string,
        unknown
      >;
    } catch {
      reqBody = {};
    }
    const refId = String(reqBody.ref_id ?? "");
    if (refId.startsWith("UD-FAIL")) {
      return fakeResponse({
        status: "success",
        request_id: "test-req-fail",
        data: {
          order: {
            ref_id: refId,
            status: "FAILED",
            delivery_status: "FAILED",
            delivered_data: { items: [] },
          },
        },
      });
    }
    return fakeResponse(SONDEV_ORDER);
  }
  if (/api\.sondevs\.tech\/api\/v1\/orders\/[^/]+$/.test(url)) {
    return fakeResponse(SONDEV_ORDER);
  }
  if (url.includes("api.sondevs.tech/api/v1/products")) {
    if (stubControl.sondevFail) {
      throw new Error("HARNESS: Sondev products down");
    }
    return fakeResponse(SONDEV_PRODUCTS);
  }
  if (url.includes("api.sondevs.tech/api/v1/profile")) {
    if (stubControl.sondevFail) {
      throw new Error("HARNESS: Sondev profile down");
    }
    return fakeResponse({
      status: "success",
      data: { balance_idr: stubControl.sondevBalanceIdr, balance_usdt: 0 },
    });
  }
  throw new Error(`HARNESS: fetch tak terduga: ${url}`);
}) as typeof fetch;

// ---- hasil test ----
const results: { name: string; ok: boolean; detail?: string }[] = [];
function check(name: string, cond: boolean, detail?: string): void {
  results.push({ name, ok: cond, detail: cond ? undefined : detail });
  console.log(`${cond ? "PASS" : "FAIL"}  ${name}${cond ? "" : ` — ${detail ?? ""}`}`);
}

async function main(): Promise<void> {
  const { createBot } = await import("./bot");
  const { getDb } = await import("../lib/db");
  const { getWallet, creditWallet, getReferralStats, hasRedeemedPromo, listPromoCodes } =
    await import("../lib/wallet");
  const { getStoreOrder } = await import("../lib/orders");
  const { validateInitData } = await import("../lib/telegram-webapp");
  const { getWebAppUrl, setWebAppMenuButton } = await import("./menu-button");
  const { serializeQrisPayload } = await import("../lib/payments/qris");
  const { setGopayStaticQris } = await import("../lib/payments/qris-settings");
  const { gopayClient } = await import("../lib/payments/gopay");
  const { tickQrisPayments } = await import("../lib/payments/qris-poller");

  // QRIS statis sintetis (speks EMVCo — CRC dihitung serializeQrisPayload).
  TEST_STATIC_QRIS = serializeQrisPayload([
    { tag: "00", value: "01" },
    { tag: "01", value: "11" },
    {
      tag: "26",
      value: "0014ID.CO.QRIS.WWW011693600123456789010215ID1234567890123",
    },
    { tag: "52", value: "4814" },
    { tag: "53", value: "360" },
    { tag: "58", value: "ID" },
    { tag: "59", value: "TOKO TEST QRIS" },
    { tag: "60", value: "JAKARTA" },
  ]);
  setGopayStaticQris(TEST_STATIC_QRIS);
  // Sesi GoBiz mock (mock fetch mengembalikan token yang sama) supaya
  // tickQrisPayments bisa jalan tanpa login OTP sungguhan.
  gopayClient.saveSession({
    accessToken: "AT-HARNESS",
    refreshToken: "RT-HARNESS",
    tokenType: "Bearer",
    expiresAt: Date.now() + 3600_000,
    phone: "81234567890",
    merchantId: "M-HARNESS",
  });

  const bot = createBot();
  (bot as unknown as { botInfo: unknown }).botInfo = {
    id: 1,
    is_bot: true,
    first_name: "TestBot",
    username: "unicodesdigital_bot",
    can_join_groups: true,
    can_read_all_group_messages: false,
    supports_inline_queries: false,
  };

  // bot.api ASLI dipakai — kirimannya direkam oleh fetch stub di atas
  // (lebih setia: menguji jalur grammY + fixedFetch yang sebenarnya).
  // botInfo wajib diisi manual karena handleUpdate butuh this.me.

  // setMyCommands didaftarkan fire-and-forget di createBot — tunggu sebentar.
  for (let i = 0; i < 50 && !calls.some((c) => c.method === "setMyCommands"); i++) {
    await new Promise((r) => setTimeout(r, 20));
  }
  const cmdCall = calls.find((c) => c.method === "setMyCommands");
  const cmdList = (cmdCall?.data as { command: string }[] | undefined) ?? [];
  check(
    "setMyCommands dipanggil saat startup (start/products/order/cancel, tanpa /menu)",
    cmdList.length === 4 &&
      cmdList.some((c) => c.command === "start") &&
      cmdList.some((c) => c.command === "products") &&
      cmdList.some((c) => c.command === "order") &&
      cmdList.some((c) => c.command === "cancel") &&
      !cmdList.some((c) => c.command === "menu"),
    JSON.stringify(cmdList),
  );

  // ---- pembangun update palsu ----
  const admin = { id: 807719771, first_name: "Dimas", username: "Dimas0227", is_bot: false };
  const buyer = { id: 111, first_name: "Budi", username: "budi", is_bot: false };
  const friend = { id: 333, first_name: "Sari", username: "sari", is_bot: false };
  let updateId = 1;
  let msgId = 1;
  const textUpdate = (user: typeof buyer, text: string, extra?: Record<string, unknown>) => {
    // grammY mendeteksi command dari entity bot_command — tanpa ini,
    // bot.command(...) tidak match (pesan Telegram asli selalu membawanya).
    const entities = text.startsWith("/")
      ? [{ offset: 0, length: text.split(/\s/)[0].length, type: "bot_command" }]
      : undefined;
    return {
      update_id: updateId++,
      message: {
        message_id: msgId++,
        from: user,
        chat: { id: user.id, type: "private", first_name: user.first_name },
        date: Math.floor(Date.now() / 1000),
        text,
        ...(entities ? { entities } : {}),
        ...extra,
      },
    };
  };
  const cbUpdate = (user: typeof buyer, data: string) => ({
    update_id: updateId++,
    callback_query: {
      id: `cq${updateId}`,
      from: user,
      message: {
        message_id: 100,
        chat: { id: user.id, type: "private" },
        date: Math.floor(Date.now() / 1000),
        text: "menu",
      },
      data,
    },
  });
  const lastText = (method?: string): string => {
    for (let i = calls.length - 1; i >= 0; i--) {
      if ((!method || calls[i].method === method) && calls[i].text) return calls[i].text as string;
    }
    return "";
  };
  const anyText = (needle: string, method?: string): boolean =>
    calls.some(
      (c) => (!method || c.method === method) && (c.text ?? "").includes(needle),
    );
  // Cari teks di kiriman SEJAK indeks tertentu — dipakai saat satu handler
  // mengirim 2 pesan berurutan (mis. promoInvalid lalu ringkasan order),
  // supaya assertion tidak menangkap pesan kedua.
  const textSince = (
    fromIndex: number,
    needle: string,
    method?: string,
  ): boolean =>
    calls
      .slice(fromIndex)
      .some(
        (c) =>
          (!method || c.method === method) &&
          (c.text ?? "").includes(needle),
      );
  const kbText = (call?: Call): string => JSON.stringify(call?.keyboard ?? "");
  const lastCall = (method: string): Call | undefined => {
    for (let i = calls.length - 1; i >= 0; i--) {
      if (calls[i].method === method) return calls[i];
    }
    return undefined;
  };
  async function send(update: unknown): Promise<string | null> {
    try {
      await bot.handleUpdate(update as never);
      return null;
    } catch (err) {
      return err instanceof Error ? err.message : String(err);
    }
  }

  // ================= S1: /start kaya =================
  let err = await send(textUpdate(buyer, "/start"));
  check("/start tidak throw", err === null, err ?? undefined);
  check(
    "/start menyapa nama + daftar produk + cara belanja, tanpa referral, tanpa statistik untuk user baru",
    lastText("sendMessage").includes("Welcome to Unicodes Digital, Budi") &&
      lastText("sendMessage").includes("Di sini kamu bisa beli") &&
      lastText("sendMessage").includes("Muse AI Token") &&
      lastText("sendMessage").includes("Cara belanja") &&
      !lastText("sendMessage").includes("UD-111") &&
      !/referral/i.test(lastText("sendMessage")) &&
      !lastText("sendMessage").includes("Total belanja") &&
      !lastText("sendMessage").includes("0 order"),
    lastText("sendMessage").slice(0, 300),
  );
  check(
    "/start keyboard fokus Sondev: Katalog + Saldo + Pesanan, tanpa Referral/Promo/Reseller/Bahasa",
    kbText(lastCall("sendMessage") as Call).includes("Katalog") &&
      kbText(lastCall("sendMessage") as Call).includes("Saldo") &&
      kbText(lastCall("sendMessage") as Call).includes("Pesanan") &&
      !kbText(lastCall("sendMessage") as Call).includes("Referral") &&
      !kbText(lastCall("sendMessage") as Call).includes("Promo") &&
      !kbText(lastCall("sendMessage") as Call).includes("Reseller") &&
      !kbText(lastCall("sendMessage") as Call).includes("Bahasa"),
  );

  // ================= S2: referral via /start ref_ =================
  err = await send(textUpdate(friend, "/start ref_UD-222"));
  check("start ref tidak throw", err === null, err ?? undefined);
  check("referral tercatat untuk 222", getReferralStats(222).invited === 1);

  // ================= S3: menu + katalog + detail =================
  err = await send(cbUpdate(buyer, "cat"));
  check("tombol Katalog tidak throw", err === null, err ?? undefined);
  check(
    "katalog fokus Sondev: langsung produk Muse, tanpa AI Lain / Gemini",
    kbText(lastCall("editMessageText")).includes("Muse AI Token") &&
      !kbText(lastCall("editMessageText")).includes("AI Lain") &&
      !kbText(lastCall("editMessageText")).includes("Gemini"),
    kbText(lastCall("editMessageText")).slice(0, 200),
  );
  err = await send(cbUpdate(buyer, "cat:all"));
  check("pilih kategori Semua tidak throw", err === null, err ?? undefined);
  check(
    "katalog Semua (Sondev saja) menampilkan Muse (tombol)",
    kbText(lastCall("editMessageText")).includes("Muse AI Token") &&
      !kbText(lastCall("editMessageText")).includes("Gemini"),
    kbText(lastCall("editMessageText")).slice(0, 150),
  );
  err = await send(cbUpdate(buyer, "prod:muse-token-1b"));
  check("detail produk tidak throw", err === null, err ?? undefined);
  check(
    "detail produk ada harga + paket jumlah",
    lastText("editMessageText").includes("Rp1.500") &&
      kbText(lastCall("editMessageText") as Call).includes("5B"),
    lastText("editMessageText").slice(0, 100),
  );
  err = await send(cbUpdate(buyer, "prod:gemini-xiaomi-3bln"));
  check(
    "produk Gemini comingSoon",
    lastText("editMessageText").includes("Segera hadir"),
    lastText("editMessageText").slice(0, 100),
  );

  // ================= S4: checkout qty -> target -> promo skip -> pay =================
  err = await send(cbUpdate(buyer, "qty:muse-token-1b:5"));
  check("pilih qty tidak throw", err === null, err ?? undefined);
  check("qty 5 meminta target", lastText("editMessageText").includes("Kode invite"));
  err = await send(textUpdate(buyer, "https://muse.ai/invite/123"));
  check("kirim target tidak throw", err === null, err ?? undefined);
  check("setelah target: layar promo", lastText("sendMessage").includes("kode promo"));
  err = await send(cbUpdate(buyer, "promo_skip"));
  check("lewati promo tidak throw", err === null, err ?? undefined);
  check(
    "layar bayar total 5x1500",
    lastText("editMessageText").includes("Rp7.500") &&
      kbText(lastCall("editMessageText") as Call).includes("pay_wallet"),
    lastText("editMessageText").slice(0, 200),
  );

  // ================= S5: dompet + top-up =================
  err = await send(cbUpdate(buyer, "wallet"));
  check("layar dompet tidak throw", err === null, err ?? undefined);
  check("dompet awal Rp0", lastText("editMessageText").includes("Rp0"));
  err = await send(cbUpdate(buyer, "topup"));
  check(
    "layar top-up ada nominal (tombol)",
    kbText(lastCall("editMessageText")).includes("25.000"),
    kbText(lastCall("editMessageText")).slice(0, 150),
  );
  err = await send(cbUpdate(buyer, "topup:25000"));
  check("buat top-up tidak throw", err === null, err ?? undefined);
  const topupPhoto = lastCall("sendPhoto");
  check(
    "top-up kirim foto QR (kartu standee) + caption nominal unik",
    (topupPhoto?.text ?? "").includes("Rp25.001") &&
      (topupPhoto?.text ?? "").includes("Isi saldo"),
    JSON.stringify({ caption: topupPhoto?.text?.slice(0, 150) }),
  );
  const topupRow = getDb()
    .prepare("SELECT id FROM wallet_topups WHERE tg_id = ? ORDER BY created_at DESC LIMIT 1")
    .get(111) as { id: string } | undefined;
  check("baris top-up tersimpan", !!topupRow?.id);
  const topupQrisRow = topupRow
    ? (getDb()
        .prepare("SELECT unique_amount_idr FROM qris_payments WHERE reference = ?")
        .get(topupRow.id) as { unique_amount_idr: number } | undefined)
    : undefined;
  check(
    "pembayaran QRIS top-up tersimpan (nominal unik 25001)",
    topupQrisRow?.unique_amount_idr === 25001,
    `unique=${topupQrisRow?.unique_amount_idr}`,
  );

  // ================= S6: feed GoBiz -> top-up lunas (idempotent) =================
  stubControl.feedTransactions = [{ id: "TX-TOPUP-25001", amountIdr: 25001 }];
  const tickTopup = await tickQrisPayments();
  check("tick poller top-up ok", tickTopup.ok === true, tickTopup.error);
  check("tick poller top-up match 1", tickTopup.matched === 1, JSON.stringify(tickTopup));
  check("saldo jadi Rp25.000", getWallet(111).balanceIdr === 25000);
  const tickTopup2 = await tickQrisPayments();
  check(
    "tick duplikat idempotent (tetap 25.000, match 0)",
    getWallet(111).balanceIdr === 25000 && tickTopup2.matched === 0,
    JSON.stringify(tickTopup2),
  );

  // ================= S7: bayar saldo -> delivered + notifikasi =================
  await send(cbUpdate(buyer, "cat"));
  await send(cbUpdate(buyer, "prod:muse-token-1b"));
  await send(cbUpdate(buyer, "qty:muse-token-1b:1"));
  await send(textUpdate(buyer, "https://muse.ai/invite/123"));
  await send(cbUpdate(buyer, "promo_skip"));
  const callsBeforePay = calls.length;
  err = await send(cbUpdate(buyer, "pay_wallet"));
  check("bayar saldo tidak throw", err === null, err ?? undefined);
  const payCalls = calls.slice(callsBeforePay);
  check(
    "bayar saldo konfirmasi",
    payCalls.some((c) => (c.text ?? "").includes("Dibayar dari saldo")),
    JSON.stringify(payCalls.map((c) => c.text?.slice(0, 60))),
  );
  check("saldo terpotong 1500", getWallet(111).balanceIdr === 23500);
  const orderRow = getDb()
    .prepare("SELECT id, status FROM store_orders WHERE buyer_ref = 'tg:111' ORDER BY created_at DESC LIMIT 1")
    .get() as { id: string; status: string } | undefined;
  check("order delivered (stub Sondev)", orderRow?.status === "delivered", orderRow?.status);
  check(
    "notifikasi selesai dikirim ke pembeli",
    calls.some(
      (c) => c.method === "sendMessage" && c.chatId === 111 && (c.text ?? "").includes("selesai"),
    ),
  );
  // Welcome menampilkan statistik setelah user punya order.
  err = await send(textUpdate(buyer, "/start"));
  check(
    "welcome menampilkan statistik setelah ada order",
    err === null &&
      lastText().includes("📦 1 order") &&
      lastText().includes("Total belanja: Rp1.500"),
    lastText().slice(0, 250),
  );

  // ================= S8: reward referral =================
  creditWallet({ tgId: 333, amountIdr: 50000, kind: "topup", ref: "TEST", note: "test" });
  await send(cbUpdate(friend, "cat"));
  await send(cbUpdate(friend, "prod:muse-token-1b"));
  await send(cbUpdate(friend, "qty:muse-token-1b:1"));
  await send(textUpdate(friend, "https://muse.ai/invite/999"));
  await send(cbUpdate(friend, "promo_skip"));
  err = await send(cbUpdate(friend, "pay_wallet"));
  check("order pertama teman tidak throw", err === null, err ?? undefined);
  check("referrer 222 dapat Rp1.500", getWallet(222).balanceIdr === 1500);
  check("referral tercatat rewarded", getReferralStats(222).rewarded === 1);
  // Order kedua teman -> reward TIDAK dobel.
  await send(cbUpdate(friend, "cat"));
  await send(cbUpdate(friend, "prod:muse-token-1b"));
  await send(cbUpdate(friend, "qty:muse-token-1b:1"));
  await send(textUpdate(friend, "https://muse.ai/invite/999"));
  await send(cbUpdate(friend, "promo_skip"));
  await send(cbUpdate(friend, "pay_wallet"));
  check("reward tidak dobel di order kedua", getWallet(222).balanceIdr === 1500);

  // ================= S9: promo =================
  err = await send(textUpdate(admin, "/promo_add TEST10 percent 10 5 30"));
  check("promo_add admin tidak throw", err === null, err ?? undefined);
  check("promo TEST10 dibuat", lastText("sendMessage").includes("TEST10"));
  err = await send(textUpdate(admin, "/promo_list"));
  check("promo_list menampilkan TEST10", lastText("sendMessage").includes("TEST10"));
  err = await send(textUpdate(buyer, "/promo_add NAKAL percent 10"));
  check("promo_add non-admin ditolak", lastText("sendMessage").includes("khusus admin"));
  // Checkout qty 10 + promo TEST10 (10% dari 15000 = 1500).
  await send(cbUpdate(buyer, "cat"));
  await send(cbUpdate(buyer, "prod:muse-token-1b"));
  await send(cbUpdate(buyer, "qty:muse-token-1b:10"));
  await send(textUpdate(buyer, "https://muse.ai/invite/123"));
  await send(cbUpdate(buyer, "promo_enter"));
  err = await send(textUpdate(buyer, "TEST10"));
  check("pakai promo tidak throw", err === null, err ?? undefined);
  check(
    "promo dipakai: diskon 1500, total 13500",
    lastText("sendMessage").includes("Rp1.500") &&
      lastText("sendMessage").includes("Rp13.500"),
    lastText("sendMessage").slice(0, 300),
  );
  err = await send(cbUpdate(buyer, "pay_wallet"));
  check("bayar dengan promo tidak throw", err === null, err ?? undefined);
  check("saldo 23500-13500=10000", getWallet(111).balanceIdr === 10000);
  check("redemption promo tercatat", hasRedeemedPromo("TEST10", 111));
  check("uses_count promo = 1", (listPromoCodes().find((p) => p.code === "TEST10")?.usesCount ?? -1) === 1);
  // Pakai lagi oleh user yang sama -> ditolak.
  await send(cbUpdate(buyer, "cat"));
  await send(cbUpdate(buyer, "prod:muse-token-1b"));
  await send(cbUpdate(buyer, "qty:muse-token-1b:1"));
  await send(textUpdate(buyer, "https://muse.ai/invite/123"));
  await send(cbUpdate(buyer, "promo_enter"));
  err = await send(textUpdate(buyer, "TEST10"));
  check(
    "promo yang sama tidak bisa dipakai 2x",
    anyText("sudah pernah kamu pakai", "sendMessage"),
    lastText("sendMessage").slice(0, 120),
  );

  // ================= S10: support forward + balasan admin =================
  err = await send(cbUpdate(buyer, "support"));
  check("layar support ada admin", lastText("editMessageText").includes("@Dimas0227"));
  err = await send(cbUpdate(buyer, "support_msg"));
  err = await send(textUpdate(buyer, "Order saya belum masuk, tolong dicek"));
  check("pesan support tidak throw", err === null, err ?? undefined);
  check(
    "pesan support diteruskan ke admin",
    calls.some((c) => c.method === "forwardMessage" && c.chatId === 807719771),
  );
  check("konfirmasi terkirim ke pembeli", lastText("sendMessage").includes("diteruskan ke admin"));
  const adminReply = textUpdate(admin, "Sudah dicek, token sudah masuk ya", {
    reply_to_message: {
      message_id: 555,
      from: { id: 1, first_name: "TestBot", is_bot: true },
      forward_from: { id: 111, first_name: "Budi", is_bot: false },
      text: "Order saya belum masuk, tolong dicek",
    },
  });
  err = await send(adminReply);
  check("balasan admin tidak throw", err === null, err ?? undefined);
  check(
    "balasan admin diteruskan ke pembeli",
    calls.some(
      (c) => c.method === "sendMessage" && c.chatId === 111 && (c.text ?? "").includes("Balasan admin"),
    ),
  );

  // ================= S11: pesanan saya + detail =================
  err = await send(cbUpdate(buyer, "orders"));
  check("daftar pesanan tidak throw", err === null, err ?? undefined);
  check(
    "daftar pesanan ada order (tombol)",
    kbText(lastCall("editMessageText")).includes("UD-"),
    kbText(lastCall("editMessageText")).slice(0, 150),
  );
  const myOrderId = orderRow?.id ?? "";
  err = await send(cbUpdate(buyer, `ord:${myOrderId}`));
  check(
    "detail order ada status selesai",
    lastText("editMessageText").includes("Selesai"),
    lastText("editMessageText").slice(0, 150),
  );

  // ================= S12: bahasa =================
  err = await send(cbUpdate(buyer, "lang"));
  check("layar bahasa tidak throw", err === null, err ?? undefined);
  err = await send(cbUpdate(buyer, "lang:en"));
  check("ganti ke EN tidak throw", err === null, err ?? undefined);
  check(
    "menu berbahasa Inggris (tombol)",
    kbText(lastCall("editMessageText")).includes("Product Catalog"),
    kbText(lastCall("editMessageText")).slice(0, 150),
  );
  err = await send(cbUpdate(buyer, "lang:id"));
  check(
    "kembali ke ID (tombol)",
    kbText(lastCall("editMessageText")).includes("Katalog Produk"),
    kbText(lastCall("editMessageText")).slice(0, 150),
  );

  // ================= S13: kebijakan + reseller =================
  err = await send(cbUpdate(buyer, "policy"));
  check("kebijakan ada refund + garansi", lastText("editMessageText").includes("Refund") && lastText("editMessageText").includes("Garansi"));
  err = await send(cbUpdate(buyer, "reseller"));
  check("reseller info (tanpa API palsu)", lastText("editMessageText").includes("segera hadir"));

  // ================= S14: legacy /products /order /fulfill =================
  err = await send(textUpdate(buyer, "/products"));
  check(
    "/products fokus Sondev: langsung daftar Muse, tanpa AI Lain / Gemini",
    err === null &&
      kbText(lastCall("sendMessage")).includes("Muse AI Token") &&
      !kbText(lastCall("sendMessage")).includes("AI Lain") &&
      !kbText(lastCall("sendMessage")).includes("Gemini"),
    err ?? kbText(lastCall("sendMessage")).slice(0, 120),
  );
  err = await send(textUpdate(buyer, `/order ${myOrderId}`));
  check("/order tetap jalan", err === null && lastText("sendMessage").includes(myOrderId), err ?? undefined);
  err = await send(textUpdate(buyer, "/order"));
  check("/order tanpa ID tidak throw", err === null, err ?? undefined);
  check(
    "/order tanpa ID menampilkan daftar pesanan saya",
    kbText(lastCall("sendMessage")).includes(myOrderId),
    kbText(lastCall("sendMessage")).slice(0, 200),
  );
  const { createStoreOrder } = await import("../lib/orders");
  const legacyOrder = await createStoreOrder({
    catalogProductId: "muse-token-1b",
    quantity: 1,
    buyerRef: "tg:111",
    target: "https://muse.ai/invite/123",
  });
  err = await send(textUpdate(admin, `/fulfill ${legacyOrder.id}`));
  check("/fulfill admin tidak throw", err === null, err ?? undefined);
  const legacyDone = await getStoreOrder(legacyOrder.id);
  check("/fulfill menyelesaikan order", legacyDone?.status === "delivered", legacyDone?.status);

  // ================= S15: validasi initData Web App =================
  const TEST_TOKEN = "123456:TESTTOKEN";
  const buildInitData = (
    token: string,
    fields: Record<string, string>,
    authDate?: number,
  ): string => {
    const all: Record<string, string> = {
      ...fields,
      auth_date: String(authDate ?? Math.floor(Date.now() / 1000)),
    };
    const pairs = Object.entries(all)
      .map(([k, v]) => `${k}=${v}`)
      .sort();
    const secret = createHmac("sha256", "WebAppData").update(token).digest();
    const hash = createHmac("sha256", secret).update(pairs.join("\n")).digest("hex");
    return new URLSearchParams({ ...all, hash }).toString();
  };
  const userJson = JSON.stringify({ id: 111, first_name: "Budi", username: "budi" });
  const good = buildInitData(TEST_TOKEN, {
    query_id: "q1",
    user: userJson,
  });
  const valid = validateInitData(good, TEST_TOKEN);
  check("initData benar -> ok + user 111", valid.ok === true && valid.ok && valid.user.id === 111);
  const tampered = good.replace("Budi", "Bado");
  const tamperedRes = validateInitData(tampered, TEST_TOKEN);
  check("initData diubah -> bad_hash", !tamperedRes.ok && tamperedRes.reason === "bad_hash");
  const expired = buildInitData(
    TEST_TOKEN,
    { query_id: "q1", user: userJson },
    Math.floor(Date.now() / 1000) - 90000,
  );
  const expiredRes = validateInitData(expired, TEST_TOKEN);
  check("initData kedaluwarsa -> expired", !expiredRes.ok && expiredRes.reason === "expired");
  const noHash = new URLSearchParams({
    query_id: "q1",
    user: userJson,
    auth_date: String(Math.floor(Date.now() / 1000)),
  }).toString();
  const noHashRes = validateInitData(noHash, TEST_TOKEN);
  check("initData tanpa hash -> missing_hash", !noHashRes.ok && noHashRes.reason === "missing_hash");
  const wrongToken = validateInitData(good, "999:WRONG");
  check("initData token salah -> bad_hash", !wrongToken.ok && wrongToken.reason === "bad_hash");

  // ================= S16: render halaman /tg tanpa env Telegram =================
  try {
    const { default: TgPage } = await import("../app/tg/page");
    const html = renderToString(createElement(TgPage));
    check(
      "halaman /tg render tanpa crash (tanpa Telegram)",
      html.includes("Unicodes Digital") && html.includes("Memuat katalog"),
      html.slice(0, 200),
    );
  } catch (e) {
    check("halaman /tg render tanpa crash (tanpa Telegram)", false, e instanceof Error ? e.message : String(e));
  }

  // ================= S17: menu button Web App (belum deploy) =================
  check("getWebAppUrl() undefined saat localhost", getWebAppUrl() === undefined);
  let menuButtonThrew = false;
  try {
    await setWebAppMenuButton();
  } catch {
    menuButtonThrew = true;
  }
  check("setWebAppMenuButton menolak sebelum ada domain publik", menuButtonThrew);

  // ================= S18: banner di /start & /menu =================
  process.env.BOT_BANNER_FILE_ID = "TEST-BANNER-FILE-ID";
  err = await send(textUpdate(buyer, "/start"));
  check("/start banner tidak throw", err === null, err ?? undefined);
  const startPhoto = lastCall("sendPhoto");
  check(
    "/start mengirim banner (file_id benar + caption welcome + keyboard menu)",
    startPhoto?.photo === "TEST-BANNER-FILE-ID" &&
      (startPhoto?.text ?? "").includes("Welcome to Unicodes Digital, Budi") &&
      kbText(startPhoto).includes("Katalog"),
    JSON.stringify({
      photo: startPhoto?.photo,
      caption: startPhoto?.text?.slice(0, 80),
    }),
  );
  err = await send(textUpdate(buyer, "/menu"));
  check(
    "/menu mengirim banner juga",
    lastCall("sendPhoto")?.photo === "TEST-BANNER-FILE-ID",
    JSON.stringify(lastCall("sendPhoto")),
  );
  // file_id tidak valid -> fallback teks, bot tidak diam.
  process.env.BOT_BANNER_FILE_ID = "BAD-FILE-ID";
  const photosBeforeBad = calls.filter((c) => c.method === "sendPhoto").length;
  err = await send(textUpdate(buyer, "/start"));
  check("/start file_id rusak tidak throw", err === null, err ?? undefined);
  check(
    "/start file_id rusak fallback ke teks welcome (tanpa foto baru)",
    lastText("sendMessage").includes("Welcome to Unicodes Digital, Budi") &&
      calls.filter((c) => c.method === "sendPhoto").length === photosBeforeBad,
    lastText("sendMessage").slice(0, 120),
  );
  // env kosong -> teks biasa.
  delete process.env.BOT_BANNER_FILE_ID;
  err = await send(textUpdate(buyer, "/start"));
  check(
    "/start tanpa banner env fallback teks",
    err === null &&
      lastText("sendMessage").includes("Welcome to Unicodes Digital, Budi"),
    err ?? lastText("sendMessage").slice(0, 120),
  );

  // ================= S19: kategori + stok =================
  err = await send(cbUpdate(buyer, "cat:Muse"));
  check(
    "kategori Muse hanya menampilkan Muse",
    kbText(lastCall("editMessageText")).includes("Muse AI Token") &&
      !kbText(lastCall("editMessageText")).includes("Gemini"),
    kbText(lastCall("editMessageText")).slice(0, 200),
  );
  err = await send(cbUpdate(buyer, "cat:AI Lain"));
  check(
    "kategori AI Lain disembunyikan di bot (daftar kosong, tanpa Gemini)",
    err === null &&
      !kbText(lastCall("editMessageText")).includes("Gemini") &&
      !kbText(lastCall("editMessageText")).includes("Muse AI Token"),
    kbText(lastCall("editMessageText")).slice(0, 200),
  );
  err = await send(cbUpdate(buyer, "cat:all"));
  check(
    "kategori Semua (Sondev saja): Muse saja, tanpa Gemini / Segera Hadir",
    kbText(lastCall("editMessageText")).includes("Muse AI Token") &&
      !kbText(lastCall("editMessageText")).includes("Gemini") &&
      !kbText(lastCall("editMessageText")).includes("Segera Hadir"),
    kbText(lastCall("editMessageText")).slice(0, 200),
  );
  // Produk coming_soon tidak bisa dibeli.
  err = await send(cbUpdate(buyer, "prod:gemini-xiaomi-3bln"));
  check(
    "detail Gemini coming_soon tanpa tombol beli",
    lastText("editMessageText").includes("Segera hadir") &&
      !kbText(lastCall("editMessageText")).includes("qty:"),
    kbText(lastCall("editMessageText")).slice(0, 200),
  );
  // Produk out_of_stock: ubah sementara stok Muse (seed tidak punya produk
  // out_of_stock — jangan tambah produk palsu ke katalog asli).
  const { CATALOG } = await import("../lib/catalog");
  const muse = CATALOG.find((p) => p.id === "muse-token-1b");
  const ordersBefore = (
    getDb().prepare("SELECT COUNT(*) AS n FROM store_orders").get() as { n: number }
  ).n;
  if (muse) muse.stockState = "out_of_stock";
  try {
    err = await send(cbUpdate(buyer, "cat:all"));
    check(
      "katalog menampilkan badge Stok Habis",
      kbText(lastCall("editMessageText")).includes("Stok Habis"),
      kbText(lastCall("editMessageText")).slice(0, 200),
    );
    err = await send(cbUpdate(buyer, "prod:muse-token-1b"));
    check(
      "detail out_of_stock ada pesan Stok Habis tanpa tombol beli",
      lastText("editMessageText").includes("Stok habis") &&
        !kbText(lastCall("editMessageText")).includes("qty:"),
      lastText("editMessageText").slice(0, 150),
    );
    err = await send(cbUpdate(buyer, "qty:muse-token-1b:1"));
    check(
      "checkout out_of_stock ditolak (pesan stok, tanpa order baru)",
      lastText("editMessageText").includes("Stok habis") &&
        (
          getDb().prepare("SELECT COUNT(*) AS n FROM store_orders").get() as {
            n: number;
          }
        ).n === ordersBefore,
      lastText("editMessageText").slice(0, 150),
    );
  } finally {
    if (muse) muse.stockState = "available";
  }

  // ================= S20: qty custom, target kosong, saldo kurang, Snap =================
  // Qty custom: 0 / 31 / "abc" / -5 ditolak; 7 diterima.
  await send(cbUpdate(buyer, "cat:all"));
  await send(cbUpdate(buyer, "prod:muse-token-1b"));
  await send(cbUpdate(buyer, "qtycustom:muse-token-1b"));
  for (const bad of ["0", "31", "abc", "-5"]) {
    err = await send(textUpdate(buyer, bad));
    check(
      `qty custom "${bad}" ditolak`,
      err === null && lastText("sendMessage").includes("Jumlah harus angka"),
      lastText("sendMessage").slice(0, 100),
    );
  }
  err = await send(textUpdate(buyer, "7"));
  check(
    "qty custom 7 diterima (minta target)",
    err === null && lastText("sendMessage").includes("Kode invite"),
    lastText("sendMessage").slice(0, 120),
  );
  // Target kosong (cuma spasi) ditolak — minta ulang, tidak lanjut ke promo.
  err = await send(textUpdate(buyer, "   "));
  check(
    "target kosong ditolak (minta ulang)",
    err === null && lastText("sendMessage").includes("Kode invite"),
    lastText("sendMessage").slice(0, 120),
  );
  err = await send(textUpdate(buyer, "https://muse.ai/invite/123"));
  await send(cbUpdate(buyer, "promo_skip"));
  // Paksa saldo rendah supaya bayar saldo pasti kurang (total 7 x 1500).
  getDb()
    .prepare("UPDATE wallets SET balance_idr = ? WHERE tg_id = ?")
    .run(1000, buyer.id);
  err = await send(cbUpdate(buyer, "pay_wallet"));
  check(
    "bayar saldo kurang: pesan jelas + tawaran isi saldo",
    err === null &&
      lastText("editMessageText").includes("tidak cukup") &&
      lastText("editMessageText").includes("Isi saldo"),
    lastText("editMessageText").slice(0, 200),
  );
  // Bayar via QRIS -> foto QR (kartu standee) dengan nominal unik, tanpa charge beneran.
  err = await send(cbUpdate(buyer, "pay_qris"));
  const qrisPhoto = lastCall("sendPhoto");
  check(
    "bayar QRIS mengirim foto QR + nominal unik 10501",
    err === null &&
      (qrisPhoto?.text ?? "").includes("Rp10.501") &&
      (qrisPhoto?.text ?? "").includes("Order"),
    JSON.stringify({ caption: qrisPhoto?.text?.slice(0, 200) }),
  );
  const awaitingOrder = getDb()
    .prepare(
      "SELECT id FROM store_orders WHERE buyer_ref = 'tg:111' AND status = 'awaiting_payment' ORDER BY created_at DESC LIMIT 1",
    )
    .get() as { id: string } | undefined;
  const qrisOrderRow = awaitingOrder
    ? (getDb()
        .prepare("SELECT unique_amount_idr FROM qris_payments WHERE reference = ?")
        .get(awaitingOrder.id) as { unique_amount_idr: number } | undefined)
    : undefined;
  check(
    "pembayaran QRIS order tersimpan (nominal unik 10501)",
    qrisOrderRow?.unique_amount_idr === 10501,
    `unique=${qrisOrderRow?.unique_amount_idr}`,
  );

  // ================= S21: top-up custom =================
  err = await send(cbUpdate(buyer, "topup"));
  err = await send(cbUpdate(buyer, "topup_custom"));
  err = await send(textUpdate(buyer, "5000"));
  check(
    "top-up custom di bawah 10000 ditolak",
    err === null && lastText("sendMessage").includes("minimal 10000"),
    lastText("sendMessage").slice(0, 120),
  );
  err = await send(textUpdate(buyer, "abc"));
  check(
    "top-up custom bukan angka ditolak",
    err === null && lastText("sendMessage").includes("minimal 10000"),
    lastText("sendMessage").slice(0, 120),
  );
  err = await send(textUpdate(buyer, "20000"));
  check(
    "top-up custom 20000 diterima (foto QR, nominal unik 20001)",
    // startTopup dipanggil dari handler pesan teks -> sendQrisPhoto mengirim
    // sendPhoto baru (bukan editMessageText seperti dari callback).
    err === null &&
      (lastCall("sendPhoto")?.text ?? "").includes("Rp20.001"),
    JSON.stringify({ caption: lastCall("sendPhoto")?.text?.slice(0, 150) }),
  );

  // ================= S22: promo tidak ditemukan / kedaluwarsa / habis =================
  const { createPromoCode, redeemPromo } = await import("../lib/wallet");
  createPromoCode({
    code: "EXPIRED1",
    kind: "percent",
    value: 10,
    validDays: -1,
    createdBy: "harness",
  });
  createPromoCode({
    code: "HABIS1",
    kind: "fixed",
    value: 1000,
    maxUses: 1,
    createdBy: "harness",
  });
  redeemPromo({ code: "HABIS1", tgId: 333, orderId: "UD-HABIS-1", discountIdr: 1000 });
  async function checkoutToPromo(): Promise<void> {
    await send(cbUpdate(buyer, "cat:all"));
    await send(cbUpdate(buyer, "prod:muse-token-1b"));
    await send(cbUpdate(buyer, "qty:muse-token-1b:1"));
    await send(textUpdate(buyer, "https://muse.ai/invite/123"));
    await send(cbUpdate(buyer, "promo_enter"));
  }
  await checkoutToPromo();
  let mark = calls.length;
  err = await send(textUpdate(buyer, "TIDAKADA"));
  check(
    "promo tidak ditemukan ditolak",
    // Handler promo mengirim 2 pesan (promoInvalid lalu ringkasan order) —
    // cari di kiriman sejak update ini, bukan pesan terakhir saja.
    err === null && textSince(mark, "tidak ditemukan", "sendMessage"),
    lastText("sendMessage").slice(0, 120),
  );
  await checkoutToPromo();
  mark = calls.length;
  err = await send(textUpdate(buyer, "EXPIRED1"));
  check(
    "promo kedaluwarsa ditolak",
    err === null && textSince(mark, "kedaluwarsa", "sendMessage"),
    lastText("sendMessage").slice(0, 120),
  );
  await checkoutToPromo();
  mark = calls.length;
  err = await send(textUpdate(buyer, "HABIS1"));
  check(
    "promo habis kuota ditolak",
    err === null && textSince(mark, "habis", "sendMessage"),
    lastText("sendMessage").slice(0, 120),
  );

  // ================= S23: order tidak ada, blokir admin =================
  err = await send(textUpdate(buyer, "/order UD-TIDAKADA"));
  check(
    "/order id tidak ada -> pesan tidak ditemukan",
    err === null && lastText("sendMessage").includes("tidak ditemukan"),
    lastText("sendMessage").slice(0, 120),
  );
  err = await send(textUpdate(buyer, "/fulfill UD-XXXX"));
  check(
    "non-admin /fulfill diblokir",
    err === null && lastText("sendMessage").includes("khusus admin"),
    lastText("sendMessage").slice(0, 120),
  );
  err = await send(textUpdate(buyer, "/promo_list"));
  check(
    "non-admin /promo_list diblokir",
    err === null && lastText("sendMessage").includes("khusus admin"),
    lastText("sendMessage").slice(0, 120),
  );
  err = await send(textUpdate(admin, "/promo_add"));
  check(
    "/promo_add tanpa argumen menampilkan cara pakai",
    err === null && lastText("sendMessage").includes("Pakai: /promo_add"),
    lastText("sendMessage").slice(0, 120),
  );
  err = await send(textUpdate(admin, "/promo_add JELEK salah 10"));
  check(
    "/promo_add kind salah menampilkan cara pakai",
    err === null && lastText("sendMessage").includes("Pakai: /promo_add"),
    lastText("sendMessage").slice(0, 120),
  );

  // ================= S24: referral diri sendiri + bahasa EN =================
  const invitedBefore = getReferralStats(111).invited;
  err = await send(textUpdate(buyer, "/start ref_UD-111"));
  check(
    "referral diri sendiri diblokir",
    err === null && getReferralStats(111).invited === invitedBefore,
    `invited ${invitedBefore} -> ${getReferralStats(111).invited}`,
  );
  await send(cbUpdate(buyer, "lang"));
  await send(cbUpdate(buyer, "lang:en"));
  err = await send(textUpdate(buyer, "/start"));
  check(
    "menu EN: welcome + tombol Inggris (tanpa referral)",
    err === null &&
      lastText("sendMessage").includes("Welcome to Unicodes Digital") &&
      !lastText("sendMessage").includes("Referral code") &&
      kbText(lastCall("sendMessage")).includes("Product Catalog") &&
      kbText(lastCall("sendMessage")).includes("My Balance"),
    lastText("sendMessage").slice(0, 200),
  );
  await send(cbUpdate(buyer, "lang:id"));

  // ================= S25: route admin GoBiz (sesi + merchant + poll) =================
  // Cookie admin = sha256(ADMIN_PASSWORD) — ADMIN_PASSWORD di-set di env atas.
  const adminCookie = `unicodes_admin=${createHash("sha256").update("test-admin-pw").digest("hex")}`;
  const gopayReq = (path: string, method = "GET", body?: unknown): Request =>
    new Request(`http://x${path}`, {
      method,
      headers: { "Content-Type": "application/json", cookie: adminCookie },
      body: body === undefined ? undefined : JSON.stringify(body),
    });
  const { GET: sessionGet } = await import(
    "../app/api/admin/payments/gopay/session/route"
  );
  const { GET: merchantGet } = await import(
    "../app/api/admin/payments/gopay/merchant/route"
  );
  const { POST: pollPost } = await import(
    "../app/api/admin/payments/gopay/poll/route"
  );
  const noAuthRes = await sessionGet(
    new Request("http://x/api/admin/payments/gopay/session"),
  );
  check("admin GoPay tanpa cookie -> 401", noAuthRes.status === 401);
  const sres = await sessionGet(
    gopayReq("/api/admin/payments/gopay/session"),
  );
  const sbody = (await sres.json()) as { authenticated?: boolean; phone?: string };
  check(
    "admin GoPay session: login + HP 812...",
    sres.status === 200 && sbody.authenticated === true && sbody.phone === "81234567890",
    JSON.stringify(sbody),
  );
  const mres = await merchantGet(
    gopayReq("/api/admin/payments/gopay/merchant"),
  );
  const mbody = (await mres.json()) as {
    staticQrisMatches?: boolean;
    profile?: { merchantName?: string };
  };
  check(
    "admin GoPay merchant: cocok dengan QRIS statis",
    mres.status === 200 &&
      mbody.staticQrisMatches === true &&
      mbody.profile?.merchantName === "Unicodes Digital",
    JSON.stringify(mbody).slice(0, 200),
  );
  stubControl.feedTransactions = [];
  const pres = await pollPost(
    gopayReq("/api/admin/payments/gopay/poll", "POST"),
  );
  const pbody = (await pres.json()) as { ok?: boolean; matched?: number };
  check(
    "admin GoPay poll manual -> ok, tidak ada match",
    pres.status === 200 && pbody.ok === true && pbody.matched === 0,
    JSON.stringify(pbody),
  );

  // ================= S26: feed duplikat -> order ter-fulfill sekali =================
  // (createStoreOrder sudah diimpor di S14 — pakai ulang di sini.)
  const { createQrisPayment } = await import("../lib/payments/qris");
  const dupOrder = await createStoreOrder({
    catalogProductId: "muse-token-1b",
    quantity: 1,
    buyerRef: "tg:111",
    target: "https://muse.ai/invite/123",
  });
  const { payment: dupQris } = createQrisPayment({
    reference: dupOrder.id,
    amountIdr: 1500,
    staticPayload: TEST_STATIC_QRIS,
  });
  stubControl.feedTransactions = [
    { id: "TX-DUP-1", amountIdr: dupQris.unique_amount_idr },
  ];
  const tickDup1 = await tickQrisPayments();
  check("tick pertama melunasi order", tickDup1.matched === 1, JSON.stringify(tickDup1));
  // Tunggu fulfill background (mock Sondev cepat) selesai.
  for (let i = 0; i < 50; i++) {
    const o = await getStoreOrder(dupOrder.id);
    if (o?.status === "delivered") break;
    await new Promise((r) => setTimeout(r, 100));
  }
  check(
    "order feed terkirim delivered",
    (await getStoreOrder(dupOrder.id))?.status === "delivered",
    (await getStoreOrder(dupOrder.id))?.status ?? "?",
  );
  const createsAfterFirst = stubControl.sondevCreateCalls;
  // Feed yang SAMA dikirim lagi (duplikat) -> tick tidak memproses ulang.
  const tickDup2 = await tickQrisPayments();
  check("tick duplikat tidak melunasi lagi", tickDup2.matched === 0, JSON.stringify(tickDup2));
  await new Promise((r) => setTimeout(r, 300));
  check(
    "feed duplikat tidak membuat order Sondev kedua",
    stubControl.sondevCreateCalls === createsAfterFirst,
    `creates ${createsAfterFirst} -> ${stubControl.sondevCreateCalls}`,
  );

  // ================= S27: fulfillment gagal -> refund =================
  const { fulfillOrder, markPaid, setOrderPaidVia } = await import(
    "../lib/orders"
  );
  const { debitWallet: debitWalletLib } = await import("../lib/wallet");
  // (a) Bayar saldo -> refund kembali ke dompet.
  creditWallet({ tgId: 111, amountIdr: 50000, kind: "topup", ref: "TEST-S27", note: "test" });
  const balA = getWallet(111).balanceIdr;
  const failWallet = await createStoreOrder({
    id: "UD-FAIL-1",
    catalogProductId: "muse-token-1b",
    quantity: 1,
    buyerRef: "tg:111",
    target: "https://muse.ai/invite/123",
  });
  const debited = debitWalletLib({ tgId: 111, amountIdr: 1500, kind: "purchase", ref: failWallet.id, note: "test" });
  check("setup S27a: debit saldo berhasil", debited !== null);
  await setOrderPaidVia(failWallet.id, "wallet");
  await markPaid(failWallet.id);
  const doneA = await fulfillOrder(failWallet.id);
  check(
    "fulfill gagal (saldo): status refunded",
    doneA.status === "refunded",
    doneA.status,
  );
  check(
    "fulfill gagal (saldo): saldo kembali utuh",
    getWallet(111).balanceIdr === balA,
    `saldo ${getWallet(111).balanceIdr} vs ${balA}`,
  );
  // (b) Bayar QRIS -> TIDAK ADA refund otomatis: status failed + catatan
  // refund manual + notifikasi ke admin.
  const failQris = await createStoreOrder({
    id: "UD-FAIL-2",
    catalogProductId: "muse-token-1b",
    quantity: 1,
    buyerRef: "tg:111",
    target: "https://muse.ai/invite/123",
  });
  await setOrderPaidVia(failQris.id, "qris");
  await markPaid(failQris.id);
  const doneB = await fulfillOrder(failQris.id);
  check(
    "fulfill gagal (QRIS): status failed (bukan refunded)",
    doneB.status === "failed",
    doneB.status,
  );
  check(
    "fulfill gagal (QRIS): ada catatan refund manual",
    (doneB.error ?? "").includes("manual"),
    doneB.error ?? "",
  );
  // notifyAdmin jalan fire-and-forget — tunggu sebentar supaya fetch stub
  // sempat mencatat pesan ke admin.
  await new Promise((r) => setTimeout(r, 300));
  check(
    "fulfill gagal (QRIS): admin diberi tahu soal refund manual",
    calls.some(
      (c) =>
        c.method === "sendMessage" &&
        c.chatId === 807719771 &&
        (c.text ?? "").includes("Refund manual"),
    ),
    JSON.stringify(
      calls
        .filter((c) => c.chatId === 807719771)
        .map((c) => c.text?.slice(0, 60)),
    ),
  );

  // ================= S28: cek saldo supplier via route order =================
  const { POST: orderPost } = await import("../app/api/orders/route");
  const orderReq = (body: unknown): Request =>
    new Request("http://x/api/orders", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify(body),
    });
  const orderBody = {
    productId: "muse-token-1b",
    quantity: 1,
    buyerRef: "tg:111",
    target: "https://muse.ai/invite/123",
  };
  // Saldo supplier kurang (500 < 1000) -> ditolak 400 SEBELUM bayar.
  stubControl.sondevBalanceIdr = 500;
  let ores = await orderPost(orderReq(orderBody));
  const oresBody = (await ores.json()) as { ok: boolean; error?: string };
  check(
    "saldo supplier kurang: order ditolak 400 sebelum bayar",
    ores.status === 400 && (oresBody.error ?? "").includes("supplier"),
    `status ${ores.status}: ${oresBody.error ?? ""}`,
  );
  // API supplier error -> fail open: order + Snap tetap jalan.
  stubControl.sondevFail = true;
  stubControl.sondevBalanceIdr = 100000;
  ores = await orderPost(orderReq(orderBody));
  const oresBody2 = (await ores.json()) as {
    ok: boolean;
    payment?: { qrPngBase64: string; uniqueAmountIdr: number };
  };
  check(
    "supplier API error: fail open, order + QRIS tetap jalan",
    ores.status === 201 &&
      !!oresBody2.payment?.qrPngBase64 &&
      oresBody2.payment?.uniqueAmountIdr === 1501,
    `status ${ores.status}`,
  );
  stubControl.sondevFail = false;
  ores = await orderPost(orderReq(orderBody));
  check(
    "saldo cukup: order dibuat (201)",
    ores.status === 201,
    `status ${ores.status}`,
  );

  // ================= S29: sesi /cancel, callback asing, tombol back =================
  await send(cbUpdate(buyer, "cat:all"));
  await send(cbUpdate(buyer, "prod:muse-token-1b"));
  await send(cbUpdate(buyer, "qty:muse-token-1b:1"));
  err = await send(textUpdate(buyer, "/cancel"));
  check(
    "/cancel tidak throw + menampilkan dibatalkan",
    err === null && lastText("sendMessage").includes("Dibatalkan"),
    err ?? lastText("sendMessage").slice(0, 100),
  );
  const callsBeforeFree = calls.length;
  err = await send(textUpdate(buyer, "halo ada orang?"));
  check(
    "teks bebas setelah cancel diabaikan (tanpa balasan baru)",
    err === null && calls.length === callsBeforeFree,
    `calls ${callsBeforeFree} -> ${calls.length}`,
  );
  err = await send(cbUpdate(buyer, "tidak_ada_xyz"));
  check("callback tidak dikenal tidak crash", err === null, err ?? undefined);
  await send(cbUpdate(buyer, "wallet"));
  err = await send(cbUpdate(buyer, "menu"));
  check(
    "tombol Menu Utama kembali ke menu",
    err === null &&
      lastText("editMessageText").includes("Welcome to Unicodes Digital"),
    lastText("editMessageText").slice(0, 120),
  );

  // ================= S30: poller — order lunas, sesi habis, QR kedaluwarsa =================
  // (a) Order QRIS lunas via feed -> delivered + notifikasi pembeli.
  const pollOrder = await createStoreOrder({
    catalogProductId: "muse-token-1b",
    quantity: 1,
    buyerRef: "tg:111",
    target: "https://muse.ai/invite/123",
  });
  const { payment: pollQris } = createQrisPayment({
    reference: pollOrder.id,
    amountIdr: 1500,
    staticPayload: TEST_STATIC_QRIS,
  });
  stubControl.feedTransactions = [
    { id: "TX-POLL-1", amountIdr: pollQris.unique_amount_idr },
  ];
  const tickPoll = await tickQrisPayments();
  check("poller: order QRIS lunas via feed", tickPoll.matched === 1, JSON.stringify(tickPoll));
  for (let i = 0; i < 50; i++) {
    const o = await getStoreOrder(pollOrder.id);
    if (o?.status === "delivered") break;
    await new Promise((r) => setTimeout(r, 100));
  }
  check(
    "poller: order ter-fulfill delivered",
    (await getStoreOrder(pollOrder.id))?.status === "delivered",
    (await getStoreOrder(pollOrder.id))?.status ?? "?",
  );
  check(
    "poller: notifikasi selesai ke pembeli",
    calls.some(
      (c) => c.method === "sendMessage" && c.chatId === 111 && (c.text ?? "").includes("selesai"),
    ),
  );
  // (b) Sesi GoBiz kedaluwarsa -> error jelas + tick tidak throw.
  gopayClient.clearSession();
  const tickNoSession = await tickQrisPayments();
  check(
    "poller: sesi GoBiz habis -> error jelas (auth_required), tidak throw",
    tickNoSession.ok === false && tickNoSession.code === "auth_required",
    JSON.stringify(tickNoSession),
  );
  // Pulihkan sesi untuk skenario berikut.
  gopayClient.saveSession({
    accessToken: "AT-HARNESS",
    refreshToken: "RT-HARNESS",
    tokenType: "Bearer",
    expiresAt: Date.now() + 3600_000,
    phone: "81234567890",
    merchantId: "M-HARNESS",
  });
  // (c) QR kedaluwarsa -> order dibatalkan.
  const expOrder = await createStoreOrder({
    catalogProductId: "muse-token-1b",
    quantity: 1,
    buyerRef: "tg:111",
    target: "https://muse.ai/invite/123",
  });
  createQrisPayment({
    reference: expOrder.id,
    amountIdr: 1500,
    staticPayload: TEST_STATIC_QRIS,
  });
  // Buat QR kedaluwarsa (tanpa menunggu 5 menit): mundurkan expires_at-nya.
  getDb()
    .prepare("UPDATE qris_payments SET expires_at = ? WHERE reference = ?")
    .run(new Date(Date.now() - 1000).toISOString(), expOrder.id);
  stubControl.feedTransactions = [];
  const tickExp = await tickQrisPayments();
  check("poller: QR kedaluwarsa disapu", tickExp.expired >= 1, JSON.stringify(tickExp));
  const expDone = await getStoreOrder(expOrder.id);
  check(
    "poller: order QR kedaluwarsa dibatalkan",
    expDone?.status === "failed" && (expDone?.error ?? "").includes("kedaluwarsa"),
    `${expDone?.status ?? "?"}: ${expDone?.error ?? ""}`,
  );

  // ================= S31: GoPay OTP via route admin =================
  // (Terakhir — verify/logout mengubah sesi; tidak ada skenario setelah ini.)
  const { POST: otpPost } = await import(
    "../app/api/admin/payments/gopay/otp/route"
  );
  const { POST: verifyPost } = await import(
    "../app/api/admin/payments/gopay/verify/route"
  );
  const { POST: logoutPost } = await import(
    "../app/api/admin/payments/gopay/logout/route"
  );
  const otpRes = await otpPost(
    gopayReq("/api/admin/payments/gopay/otp", "POST", { phone: "081234567890" }),
  );
  const otpBody = (await otpRes.json()) as { ok?: boolean; phone?: string };
  check(
    "GoPay OTP: minta OTP berhasil, otp_token tidak bocor ke client",
    otpRes.status === 200 &&
      otpBody.ok === true &&
      otpBody.phone === "81234567890" &&
      !JSON.stringify(otpBody).includes("OTP-HARNESS"),
    JSON.stringify(otpBody),
  );
  const badOtpRes = await verifyPost(
    gopayReq("/api/admin/payments/gopay/verify", "POST", { otp: "ab" }),
  );
  check("GoPay OTP: format salah ditolak 400", badOtpRes.status === 400);
  const vRes = await verifyPost(
    gopayReq("/api/admin/payments/gopay/verify", "POST", { otp: "123456" }),
  );
  const vBody = (await vRes.json()) as {
    ok?: boolean;
    session?: { authenticated?: boolean };
  };
  check(
    "GoPay OTP: verifikasi berhasil, sesi login",
    vRes.status === 200 && vBody.ok === true && vBody.session?.authenticated === true,
    JSON.stringify(vBody),
  );
  await logoutPost(gopayReq("/api/admin/payments/gopay/logout", "POST"));
  const sAfter = await sessionGet(gopayReq("/api/admin/payments/gopay/session"));
  const sAfterBody = (await sAfter.json()) as { authenticated?: boolean };
  check("GoPay logout: sesi terhapus", sAfterBody.authenticated === false);

  // ---- ringkasan ----
  const failed = results.filter((r) => !r.ok);
  console.log(`\n==== ${results.length - failed.length}/${results.length} lolos ====`);
  if (failed.length) {
    for (const f of failed) console.log(`  GAGAL: ${f.name} — ${f.detail ?? ""}`);
  }
  try {
    unlinkSync(TEST_DB);
    unlinkSync(`${TEST_DB}-wal`);
    unlinkSync(`${TEST_DB}-shm`);
  } catch {
    // file mungkin tidak ada — abaikan
  }
  process.exit(failed.length ? 1 : 0);
}

main().catch((err: unknown) => {
  console.error("HARNESS CRASH:", err);
  try {
    unlinkSync(TEST_DB);
  } catch {
    // abaikan
  }
  process.exit(1);
});
