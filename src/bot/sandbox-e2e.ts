// Tes end-to-end SANDBOX Midtrans (tanpa uang sungguhan).
//
// - Memuat .env.test.local (key SANDBOX) — HANYA untuk proses ini.
//   .env.local (produksi) dan bot/web live TIDAK disentuh.
// - Transaksi Snap ASLI ke sandbox API (order tes Rp1.500).
// - Webhook route POST dengan signature SHA512 ASLI dari sandbox server key.
// - Sondev tetap MOCK (fetch di-stub hanya untuk api.sondevs.tech);
//   Midtrans dan sisanya lewat fetch asli.
// - Tidak ada token Telegram di env ini -> notifyBuyer jadi no-op.
//
// Jalankan: npx tsx src/bot/sandbox-e2e.ts
import dotenv from "dotenv";
dotenv.config({ path: ".env.test.local", override: true });

const TEST_DB = `/tmp/unicodes-sandbox-e2e-${Date.now()}.db`;
process.env.DATABASE_URL = `file:${TEST_DB}`;
// Pastikan tidak ada token Telegram / key produksi yang bocor ke proses ini.
delete process.env.TELEGRAM_BOT_TOKEN;
// Sondev tetap MOCK: kredensial dummy hanya untuk HMAC signing ke stub.
process.env.SONDEV_API_KEY = "test-key";
process.env.SONDEV_API_SECRET = "test-secret";

// ---- fetch: Sondev mock, sisanya asli ----
const realFetch = globalThis.fetch;
let sondevCreateCalls = 0;
function sondevJson(body: unknown): Response {
  return new Response(JSON.stringify(body), {
    status: 200,
    headers: { "Content-Type": "application/json" },
  });
}
(globalThis as unknown as { fetch: typeof fetch }).fetch = (async (
  input: unknown,
  init?: { body?: unknown },
) => {
  const url = String(input);
  if (url.includes("api.sondevs.tech")) {
    if (url.includes("/orders/create")) {
      sondevCreateCalls += 1;
      return sondevJson({
        status: "success",
        request_id: "sbx-req",
        data: {
          order: {
            ref_id: "SBX",
            status: "DELIVERED",
            delivery_status: "DELIVERED",
            delivered_data: { items: ["SBX-TOKEN-TEST"] },
          },
        },
      });
    }
    if (url.includes("/products")) {
      return sondevJson({
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
      });
    }
    if (url.includes("/profile")) {
      return sondevJson({
        status: "success",
        data: { balance_idr: 100000, balance_usdt: 0 },
      });
    }
    return sondevJson({ status: "success", data: {} });
  }
  return realFetch(input as never, init as never);
}) as typeof fetch;

// ---- hasil test ----
const results: { name: string; ok: boolean; detail?: string }[] = [];
function check(name: string, cond: boolean, detail?: string): void {
  results.push({ name, ok: cond, detail: cond ? undefined : detail });
  console.log(
    `${cond ? "PASS" : "FAIL"}  ${name}${cond ? "" : ` — ${detail ?? ""}`}`,
  );
}

async function main(): Promise<void> {
  const { createHash } = await import("node:crypto");
  const { unlinkSync } = await import("node:fs");
  const { createStoreOrder, getStoreOrder } = await import("../lib/orders");
  const { createSnapTransaction } = await import("../lib/payments/midtrans");
  const { POST: webhookPost } = await import(
    "../app/api/webhooks/midtrans/route"
  );

  const serverKey = process.env.MIDTRANS_SERVER_KEY ?? "";
  check(
    "sandbox key ter-load, mode bukan produksi",
    serverKey.length > 0 && process.env.MIDTRANS_IS_PRODUCTION !== "true",
    `key len ${serverKey.length}, IS_PRODUCTION=${process.env.MIDTRANS_IS_PRODUCTION ?? "?"}`,
  );
  const sign = (orderId: string, statusCode: string, gross: string): string =>
    createHash("sha512")
      .update(`${orderId}${statusCode}${gross}${serverKey}`)
      .digest("hex");
  const webhookReq = (body: unknown): Request =>
    new Request("http://x/api/webhooks/midtrans", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify(body),
    });

  // 1) Transaksi Snap ASLI via sandbox API (order tes Rp1.500).
  const orderA = await createStoreOrder({
    catalogProductId: "muse-token-1b",
    quantity: 1,
    buyerRef: "tg:111",
    target: "https://muse.ai/invite/123",
  });
  check("order tes dibuat (Rp1.500)", orderA.retailTotalIdr === 1500, `total ${orderA.retailTotalIdr}`);
  let snap: { token: string; redirectUrl: string } | null = null;
  try {
    snap = await createSnapTransaction(orderA.id);
  } catch (err) {
    check("Snap sandbox menghasilkan token + redirect URL", false, err instanceof Error ? err.message : String(err));
  }
  check(
    "Snap sandbox menghasilkan token + redirect URL",
    !!snap &&
      snap.token.length > 0 &&
      snap.redirectUrl.startsWith("https://app.sandbox.midtrans.com/snap/"),
    snap ? snap.redirectUrl.slice(0, 80) : "tidak ada snap",
  );

  // 2) Bukti perilaku API status sandbox: transaksi Snap yang baru dibuat
  //    (belum dibayar) dijawab HTTP 200 dengan status_code "404" DI DALAM
  //    body ("Transaction doesn't exist") — inilah yang dulu membuat
  //    getMidtransStatus mengira API berhasil dengan transaction_status "".
  const statusRes = await realFetch(
    `https://api.sandbox.midtrans.com/v2/${encodeURIComponent(orderA.id)}/status`,
    {
      headers: {
        Authorization: `Basic ${Buffer.from(`${serverKey}:`).toString("base64")}`,
      },
    },
  );
  const statusBody = (await statusRes.json().catch(() => ({}))) as Record<
    string,
    unknown
  >;
  check(
    "status API sandbox: transaksi Snap baru = 404 di dalam body",
    statusRes.status === 200 && statusBody.status_code === "404",
    `http ${statusRes.status}, body status_code ${String(statusBody.status_code ?? "?")}`,
  );

  // 3) Webhook untuk order A dengan klaim settlement + signature asli ->
  //    re-confirm gagal (404 di body -> getMidtransStatus throw) -> fallback
  //    ke nilai webhook yang sudah terverifikasi signature -> lunas ->
  //    fulfill (Sondev mock) -> delivered.
  const bodyA = {
    order_id: orderA.id,
    status_code: "200",
    gross_amount: "1500",
    signature_key: sign(orderA.id, "200", "1500"),
    transaction_status: "settlement",
  };
  const resA = await webhookPost(webhookReq(bodyA));
  check(
    "webhook order A: signature asli diterima (200)",
    resA.status === 200,
    `status ${resA.status}`,
  );
  for (let i = 0; i < 100; i++) {
    const o = await getStoreOrder(orderA.id);
    if (o?.status === "delivered") break;
    await new Promise((r) => setTimeout(r, 100));
  }
  const doneA = await getStoreOrder(orderA.id);
  check(
    "webhook order A: fallback status API -> lunas -> fulfill -> delivered",
    doneA?.status === "delivered" &&
      (doneA.deliveredItems?.length ?? 0) > 0 &&
      sondevCreateCalls === 1,
    `status ${doneA?.status}, sondev creates ${sondevCreateCalls}`,
  );

  // 4) Webhook untuk order B (tidak ada di Midtrans sandbox sama sekali) ->
  //    jalur fallback yang sama -> delivered.
  const orderB = await createStoreOrder({
    catalogProductId: "muse-token-1b",
    quantity: 1,
    buyerRef: "tg:111",
    target: "https://muse.ai/invite/123",
  });
  const bodyB = {
    order_id: orderB.id,
    status_code: "200",
    gross_amount: "1500",
    signature_key: sign(orderB.id, "200", "1500"),
    transaction_status: "settlement",
  };
  const createsBeforeB = sondevCreateCalls;
  const resB = await webhookPost(webhookReq(bodyB));
  check(
    "webhook order B: signature asli diterima (200)",
    resB.status === 200,
    `status ${resB.status}`,
  );
  for (let i = 0; i < 100; i++) {
    const o = await getStoreOrder(orderB.id);
    if (o?.status === "delivered") break;
    await new Promise((r) => setTimeout(r, 100));
  }
  const doneB = await getStoreOrder(orderB.id);
  check(
    "webhook order B: lunas -> fulfill (Sondev mock) -> delivered",
    doneB?.status === "delivered" &&
      (doneB.deliveredItems?.length ?? 0) > 0 &&
      sondevCreateCalls === createsBeforeB + 1,
    `status ${doneB?.status}, sondev creates ${sondevCreateCalls}`,
  );

  // 5) Notifikasi duplikat -> diproses sekali (tidak ada order Sondev ke-2).
  const resDup = await webhookPost(webhookReq(bodyB));
  await new Promise((r) => setTimeout(r, 300));
  check(
    "webhook duplikat: tetap 200, tidak ada order Sondev ke-2",
    resDup.status === 200 && sondevCreateCalls === createsBeforeB + 1,
    `status ${resDup.status}, sondev creates ${sondevCreateCalls}`,
  );

  // 6) Signature salah -> ditolak 401, order tidak berubah.
  const badBody = { ...bodyB, signature_key: "0".repeat(128) };
  const resBad = await webhookPost(webhookReq(badBody));
  check(
    "webhook signature salah -> 401, order tidak berubah",
    resBad.status === 401 &&
      (await getStoreOrder(orderB.id))?.status === "delivered",
    `status ${resBad.status}`,
  );

  // ---- ringkasan ----
  const failed = results.filter((r) => !r.ok);
  console.log(
    `\n==== ${results.length - failed.length}/${results.length} lolos ====`,
  );
  if (failed.length) {
    for (const f of failed)
      console.log(`  GAGAL: ${f.name} — ${f.detail ?? ""}`);
  }
  try {
    unlinkSync(TEST_DB);
    unlinkSync(`${TEST_DB}-wal`);
    unlinkSync(`${TEST_DB}-shm`);
  } catch {
    // abaikan
  }
  process.exit(failed.length ? 1 : 0);
}

main().catch((err: unknown) => {
  console.error("SANDBOX E2E CRASH:", err);
  process.exit(1);
});
