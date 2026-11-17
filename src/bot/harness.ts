// Harness test bot Telegram Unicodes Digital v2 (+ Web App).
// TANPA pesan Telegram sungguhan: bot.api di-stub (semua send/edit/forward
// direkam), fetch global di-stub (Midtrans Snap + Sondev + status API),
// database = file SQLite SEMENTARA di /tmp (dev.db asli TIDAK disentuh).
// Jalankan: npx tsx src/bot/harness.ts
// Keluar 0 = semua lolos; 1 = ada yang gagal (nama + alasan dicetak).
import { createHmac } from "node:crypto";
import { unlinkSync } from "node:fs";
import { createElement } from "react";
import { renderToString } from "react-dom/server";

// ---- env (SEBELUM import modul apa pun yang membacanya) ----
const TEST_DB = `/tmp/unicodes-harness-${Date.now()}.db`;
process.env.DATABASE_URL = `file:${TEST_DB}`;
process.env.TELEGRAM_BOT_TOKEN = "123456:TESTTOKEN";
process.env.TELEGRAM_ADMIN_IDS = "807719771";
process.env.MIDTRANS_SERVER_KEY = "test-server-key";
process.env.MIDTRANS_CLIENT_KEY = "test-client-key";
process.env.SONDEV_API_KEY = "test-sondev-key";
process.env.SONDEV_API_SECRET = "test-sondev-secret";

// ---- fetch stub: Midtrans + Sondev palsu, sisanya DILARANG ----
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
(globalThis as unknown as { fetch: typeof fetch }).fetch = (async (
  input: unknown,
  init?: { body?: unknown },
) => {
  const url = String(input);
  // ---- Bot API Telegram (dipanggil grammY via fixedFetch) ----
  const tgMatch = /api\.telegram\.org\/bot[^/]+\/([A-Za-z]+)$/.exec(url);
  if (tgMatch) {
    const method = tgMatch[1];
    let body: Record<string, unknown> = {};
    try {
      body = JSON.parse(String(init?.body ?? "{}")) as Record<string, unknown>;
    } catch {
      body = {};
    }
    const chatId = typeof body.chat_id === "number" ? body.chat_id : undefined;
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
  if (url.includes("app.sandbox.midtrans.com/snap/v1/transactions")) {
    return fakeResponse({
      token: "SNAP-TEST-TOKEN",
      redirect_url: "https://snap.test/pay/TEST",
    });
  }
  if (url.includes("api.sandbox.midtrans.com/v2/") && url.endsWith("/status")) {
    return fakeResponse({
      transaction_status: "settlement",
      status_code: "200",
      gross_amount: "25000",
    });
  }
  // Refund Midtrans: order yang id-nya mengandung NOREFUND ditolak (simulasi
  // refund gagal) — dipakai S27 untuk jalur "refund manual".
  if (url.includes("api.sandbox.midtrans.com/v2/") && url.endsWith("/refund")) {
    if (url.includes("NOREFUND")) {
      throw new Error("HARNESS: refund ditolak");
    }
    return fakeResponse({ status_code: "200", transaction_status: "refund" });
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
  const { handleMidtransNotification } = await import("../lib/payments/midtrans");
  const { validateInitData } = await import("../lib/telegram-webapp");
  const { getWebAppUrl, setWebAppMenuButton } = await import("./menu-button");

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
    "/start menyapa nama + statistik + referral",
    lastText("sendMessage").includes("Welcome to Unicodes Digital, Budi") &&
      lastText("sendMessage").includes("UD-111"),
    lastText("sendMessage").slice(0, 120),
  );
  check(
    "/start keyboard ada Katalog + Saldo + Referral",
    kbText(lastCall("sendMessage") as Call).includes("Katalog") &&
      kbText(lastCall("sendMessage") as Call).includes("Saldo") &&
      kbText(lastCall("sendMessage") as Call).includes("Referral"),
  );

  // ================= S2: referral via /start ref_ =================
  err = await send(textUpdate(friend, "/start ref_UD-222"));
  check("start ref tidak throw", err === null, err ?? undefined);
  check("referral tercatat untuk 222", getReferralStats(222).invited === 1);

  // ================= S3: menu + katalog + detail =================
  err = await send(cbUpdate(buyer, "cat"));
  check("tombol Katalog tidak throw", err === null, err ?? undefined);
  check(
    "katalog menampilkan pilih kategori (Muse / AI Lain / Semua)",
    kbText(lastCall("editMessageText")).includes("Muse") &&
      kbText(lastCall("editMessageText")).includes("AI Lain") &&
      kbText(lastCall("editMessageText")).includes("Semua"),
    kbText(lastCall("editMessageText")).slice(0, 200),
  );
  err = await send(cbUpdate(buyer, "cat:all"));
  check("pilih kategori Semua tidak throw", err === null, err ?? undefined);
  check(
    "katalog Semua menampilkan Muse (tombol)",
    kbText(lastCall("editMessageText")).includes("Muse AI Token"),
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
  check(
    "top-up kirim link Snap",
    lastText("editMessageText").includes("https://snap.test/pay/TEST"),
    lastText("editMessageText").slice(0, 120),
  );
  const topupRow = getDb()
    .prepare("SELECT id FROM wallet_topups WHERE tg_id = ? ORDER BY created_at DESC LIMIT 1")
    .get(111) as { id: string } | undefined;
  check("baris top-up tersimpan", !!topupRow?.id);

  // ================= S6: webhook top-up -> saldo masuk (idempotent) =================
  const topupRes = await handleMidtransNotification({
    orderId: topupRow?.id ?? "",
    transactionStatus: "settlement",
  });
  check("webhook top-up handled", topupRes.handled, topupRes.reason);
  check("saldo jadi Rp25.000", getWallet(111).balanceIdr === 25000);
  await handleMidtransNotification({
    orderId: topupRow?.id ?? "",
    transactionStatus: "settlement",
  });
  check("webhook top-up idempotent (tetap 25.000)", getWallet(111).balanceIdr === 25000);

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
    "/products menampilkan pilih kategori",
    err === null && kbText(lastCall("sendMessage")).includes("AI Lain"),
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
    "kategori AI Lain hanya menampilkan Gemini",
    kbText(lastCall("editMessageText")).includes("Gemini") &&
      !kbText(lastCall("editMessageText")).includes("Muse AI Token"),
    kbText(lastCall("editMessageText")).slice(0, 200),
  );
  err = await send(cbUpdate(buyer, "cat:all"));
  check(
    "kategori Semua menampilkan badge Segera Hadir untuk Gemini",
    kbText(lastCall("editMessageText")).includes("Segera Hadir") &&
      kbText(lastCall("editMessageText")).includes("Muse AI Token"),
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
  // Bayar via Snap (mock) -> URL Snap dihasilkan tanpa charge beneran.
  err = await send(cbUpdate(buyer, "pay_snap"));
  check(
    "bayar Snap menghasilkan URL Snap mock",
    err === null &&
      lastText("editMessageText").includes("https://snap.test/pay/TEST"),
    lastText("editMessageText").slice(0, 200),
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
    "top-up custom 20000 diterima (link Snap)",
    // startTopup dipanggil dari handler pesan teks -> render() mengirim
    // sendMessage baru (bukan editMessageText seperti dari callback).
    err === null &&
      lastText("sendMessage").includes("https://snap.test/pay/TEST"),
    lastText("sendMessage").slice(0, 200),
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
    "menu EN: welcome + tombol Inggris",
    err === null &&
      lastText("sendMessage").includes("Welcome to Unicodes Digital") &&
      kbText(lastCall("sendMessage")).includes("Product Catalog") &&
      kbText(lastCall("sendMessage")).includes("My Balance"),
    lastText("sendMessage").slice(0, 120),
  );
  await send(cbUpdate(buyer, "lang:id"));

  // ================= S25: webhook Midtrans via route (signature + duplikat) =================
  const { createHash } = await import("node:crypto");
  const { POST: webhookPost } = await import(
    "../app/api/webhooks/midtrans/route"
  );
  const { createTopup } = await import("../lib/wallet");
  const signMidtrans = (
    orderId: string,
    statusCode: string,
    gross: string,
  ): string =>
    createHash("sha512")
      .update(`${orderId}${statusCode}${gross}test-server-key`)
      .digest("hex");
  const topup2 = createTopup(111, 30000);
  const goodBody = {
    order_id: topup2.id,
    status_code: "200",
    gross_amount: "30000",
    signature_key: signMidtrans(topup2.id, "200", "30000"),
    transaction_status: "settlement",
  };
  const webhookReq = (body: unknown): Request =>
    new Request("http://x/api/webhooks/midtrans", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify(body),
    });
  const balBeforeWebhook = getWallet(111).balanceIdr;
  let wres = await webhookPost(webhookReq(goodBody));
  check(
    "webhook signature benar -> 200 + saldo masuk sekali",
    wres.status === 200 && getWallet(111).balanceIdr === balBeforeWebhook + 30000,
    `status ${wres.status}, saldo ${getWallet(111).balanceIdr}`,
  );
  wres = await webhookPost(webhookReq(goodBody));
  check(
    "webhook top-up duplikat -> tetap 200, saldo tidak dobel",
    wres.status === 200 && getWallet(111).balanceIdr === balBeforeWebhook + 30000,
    `status ${wres.status}, saldo ${getWallet(111).balanceIdr}`,
  );
  const badBody = { ...goodBody, signature_key: "0".repeat(128) };
  wres = await webhookPost(webhookReq(badBody));
  check(
    "webhook signature salah -> 401, saldo tidak berubah",
    wres.status === 401 && getWallet(111).balanceIdr === balBeforeWebhook + 30000,
    `status ${wres.status}`,
  );

  // ================= S26: webhook duplikat order produk =================
  // (createStoreOrder sudah diimpor di S14 — pakai ulang di sini.)
  const dupOrder = await createStoreOrder({
    catalogProductId: "muse-token-1b",
    quantity: 1,
    buyerRef: "tg:111",
    target: "https://muse.ai/invite/123",
  });
  const r1 = await handleMidtransNotification({
    orderId: dupOrder.id,
    transactionStatus: "settlement",
  });
  check("webhook order pertama handled", r1.handled, r1.reason);
  // Tunggu fulfill background (mock Sondev cepat) selesai.
  for (let i = 0; i < 50; i++) {
    const o = await getStoreOrder(dupOrder.id);
    if (o?.status === "delivered") break;
    await new Promise((r) => setTimeout(r, 100));
  }
  check(
    "order webhook terkirim delivered",
    (await getStoreOrder(dupOrder.id))?.status === "delivered",
    (await getStoreOrder(dupOrder.id))?.status ?? "?",
  );
  const createsAfterFirst = stubControl.sondevCreateCalls;
  const r2 = await handleMidtransNotification({
    orderId: dupOrder.id,
    transactionStatus: "settlement",
  });
  check("webhook order duplikat handled", r2.handled, r2.reason);
  await new Promise((r) => setTimeout(r, 300));
  check(
    "webhook duplikat tidak membuat order Sondev kedua",
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
  // (b) Bayar Snap -> refund Midtrans (mock) -> refunded.
  const failSnap = await createStoreOrder({
    id: "UD-FAIL-2",
    catalogProductId: "muse-token-1b",
    quantity: 1,
    buyerRef: "tg:111",
    target: "https://muse.ai/invite/123",
  });
  await markPaid(failSnap.id);
  const doneB = await fulfillOrder(failSnap.id);
  check(
    "fulfill gagal (Snap): status refunded",
    doneB.status === "refunded",
    doneB.status,
  );
  // (c) Bayar Snap + refund ditolak -> tetap failed + catatan manual.
  const failNoRefund = await createStoreOrder({
    id: "UD-FAIL-NOREFUND",
    catalogProductId: "muse-token-1b",
    quantity: 1,
    buyerRef: "tg:111",
    target: "https://muse.ai/invite/123",
  });
  await markPaid(failNoRefund.id);
  const doneC = await fulfillOrder(failNoRefund.id);
  check(
    "fulfill gagal + refund ditolak: status tetap failed",
    doneC.status === "failed",
    doneC.status,
  );
  check(
    "fulfill gagal + refund ditolak: ada catatan refund manual",
    (doneC.error ?? "").includes("manual"),
    doneC.error ?? "",
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
    payment?: { redirectUrl: string };
  };
  check(
    "supplier API error: fail open, order + Snap tetap jalan",
    ores.status === 201 && !!oresBody2.payment?.redirectUrl,
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
