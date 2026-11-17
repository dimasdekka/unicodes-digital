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
  if (url.includes("api.sondevs.tech/api/v1/orders/create")) {
    return fakeResponse(SONDEV_ORDER);
  }
  if (/api\.sondevs\.tech\/api\/v1\/orders\/[^/]+$/.test(url)) {
    return fakeResponse(SONDEV_ORDER);
  }
  if (url.includes("api.sondevs.tech/api/v1/products")) {
    return fakeResponse(SONDEV_PRODUCTS);
  }
  if (url.includes("api.sondevs.tech/api/v1/profile")) {
    return fakeResponse({
      status: "success",
      data: { balance_idr: 100000, balance_usdt: 0 },
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
    "katalog menampilkan Muse (tombol)",
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
    "/products tetap jalan",
    err === null && kbText(lastCall("sendMessage")).includes("Muse AI Token"),
    err ?? kbText(lastCall("sendMessage")).slice(0, 120),
  );
  err = await send(textUpdate(buyer, `/order ${myOrderId}`));
  check("/order tetap jalan", err === null && lastText("sendMessage").includes(myOrderId), err ?? undefined);
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
