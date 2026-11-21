// Tes modul Midtrans Snap. Jalankan: npx tsx src/lib/payments/midtrans.test.ts
//
// - DATABASE_URL diarahkan ke file /tmp supaya dev.db live TIDAK kotor.
// - TIDAK ada request jaringan sungguhan: fetch di-mock (Snap + status).
// - Server key di sini SINTETIS ("TEST-SERVER-KEY") — bukan key asli.
process.env.DATABASE_URL =
  `file:/tmp/unicodes-midtrans-test-${Date.now()}.db?mode=rwc`;
process.env.MIDTRANS_SERVER_KEY = "TEST-SERVER-KEY";
process.env.MIDTRANS_CLIENT_KEY = "TEST-CLIENT-KEY";
delete process.env.MIDTRANS_IS_PRODUCTION; // sandbox

import { createHash } from "node:crypto";
import { unlinkSync } from "node:fs";
import {
  createMidtransPayment,
  createSnapTransaction,
  expireMidtransPayments,
  getMidtransPayment,
  getTransactionStatus,
  listExpiredPendingMidtransPayments,
  listPendingMidtransPayments,
  mapMidtransStatus,
  markMidtransPaymentFailed,
  markMidtransPaymentPaid,
  verifyNotificationSignature,
} from "./midtrans";
import { getDb } from "../db";

let passed = 0;
let failed = 0;
const failures: string[] = [];
function check(name: string, cond: boolean, detail?: string): void {
  if (cond) {
    passed += 1;
    console.log(`PASS  ${name}`);
  } else {
    failed += 1;
    failures.push(name);
    console.log(`FAIL  ${name} — ${detail ?? ""}`);
  }
}

async function main(): Promise<void> {
  // ===== Mapping status =====
  check("map: settlement -> paid", mapMidtransStatus("settlement") === "paid");
  check("map: capture -> paid", mapMidtransStatus("capture") === "paid");
  check("map: pending -> pending", mapMidtransStatus("pending") === "pending");
  check("map: authorize -> pending", mapMidtransStatus("authorize") === "pending");
  check("map: deny -> failed", mapMidtransStatus("deny") === "failed");
  check("map: cancel -> failed", mapMidtransStatus("cancel") === "failed");
  check("map: expire -> failed", mapMidtransStatus("expire") === "failed");
  check("map: failure -> failed", mapMidtransStatus("failure") === "failed");

  // ===== Signature webhook (dihitung independen di sini) =====
  const body = {
    order_id: "UD-TEST123",
    status_code: "200",
    gross_amount: "1500",
    signature_key: "",
  };
  const expected = createHash("sha512")
    .update(`${body.order_id}${body.status_code}${body.gross_amount}TEST-SERVER-KEY`)
    .digest("hex");
  check(
    "signature: benar -> true",
    verifyNotificationSignature({ ...body, signature_key: expected }) === true,
  );
  check(
    "signature: salah -> false",
    verifyNotificationSignature({ ...body, signature_key: "0".repeat(128) }) === false,
  );
  check(
    "signature: gross_amount beda -> false",
    verifyNotificationSignature({
      ...body,
      gross_amount: "1501",
      signature_key: expected,
    }) === false,
  );

  // ===== Tabel midtrans_payments =====
  const p1 = createMidtransPayment({
    orderId: "UD-MT-1",
    amountIdr: 1500,
    snapToken: "SNAP-1",
    redirectUrl: "https://app.sandbox.midtrans.com/snap/v1/redirection/1",
  });
  check("tabel: baris pending", p1.status === "pending");
  check(
    "tabel: kedaluwarsa 30 menit",
    new Date(p1.expires_at).getTime() - new Date(p1.created_at).getTime() ===
      30 * 60_000,
  );
  check(
    "tabel: markPaid sekali true, kedua false",
    markMidtransPaymentPaid("UD-MT-1") === true &&
      markMidtransPaymentPaid("UD-MT-1") === false,
  );
  check(
    "tabel: get setelah paid",
    getMidtransPayment("UD-MT-1")?.status === "paid",
  );
  const p2 = createMidtransPayment({
    orderId: "UD-MT-2",
    amountIdr: 7500,
    snapToken: "SNAP-2",
    redirectUrl: "https://app.sandbox.midtrans.com/snap/v1/redirection/2",
  });
  check(
    "tabel: pending hanya UD-MT-2",
    listPendingMidtransPayments().map((p) => p.order_id).join(",") === "UD-MT-2",
  );
  check(
    "tabel: markFailed",
    markMidtransPaymentFailed("UD-MT-2") === true &&
      getMidtransPayment("UD-MT-2")?.status === "failed",
  );
  const p3 = createMidtransPayment({
    orderId: "UD-MT-3",
    amountIdr: 1500,
    snapToken: "SNAP-3",
    redirectUrl: "https://app.sandbox.midtrans.com/snap/v1/redirection/3",
  });
  void p3;
  getDb()
    .prepare("UPDATE midtrans_payments SET expires_at = ? WHERE order_id = ?")
    .run(new Date(Date.now() - 1000).toISOString(), "UD-MT-3");
  check(
    "tabel: expired terdaftar",
    listExpiredPendingMidtransPayments().some((p) => p.order_id === "UD-MT-3"),
  );
  check("tabel: expireMidtransPayments", expireMidtransPayments() >= 1);
  check(
    "tabel: UD-MT-3 jadi expired",
    getMidtransPayment("UD-MT-3")?.status === "expired",
  );

  // ===== Snap + status (fetch di-mock) =====
  const fetchCalls: { url: string; headers: Record<string, string>; body: string }[] = [];
  (globalThis as unknown as { fetch: typeof fetch }).fetch = (async (
    input: unknown,
    init?: { headers?: Record<string, string>; body?: unknown },
  ) => {
    const url = String(input);
    fetchCalls.push({
      url,
      headers: (init?.headers ?? {}) as Record<string, string>,
      body: typeof init?.body === "string" ? init.body : "",
    });
    if (url.includes("/snap/v1/transactions")) {
      return {
        ok: true,
        status: 201,
        json: async () => ({
          token: "SNAP-TOKEN-1",
          redirect_url: "https://app.sandbox.midtrans.com/snap/v1/redirection/abc",
        }),
      } as unknown as Response;
    }
    if (url.includes("/v2/UD-MT-9/status")) {
      return {
        ok: true,
        status: 200,
        json: async () => ({
          transaction_status: "settlement",
          status_code: "200",
          gross_amount: "1500",
          payment_type: "qris",
          order_id: "UD-MT-9",
        }),
      } as unknown as Response;
    }
    if (url.includes("/v2/UD-MT-BELUM-DIBUKA/status")) {
      // Snap baru yang pembelinya belum buka halaman bayar: Midtrans
      // menjawab 404 di body (HTTP 200) — harus jadi pending, bukan gagal.
      return {
        ok: true,
        status: 200,
        json: async () => ({
          status_code: "404",
          status_message: "Transaction doesn't exist.",
          order_id: "UD-MT-BELUM-DIBUKA",
        }),
      } as unknown as Response;
    }
    if (url.includes("/v2/UD-MT-404HTTP/status")) {
      return {
        ok: false,
        status: 404,
        json: async () => ({
          status_code: "404",
          status_message: "Transaction doesn't exist.",
          order_id: "UD-MT-404HTTP",
        }),
      } as unknown as Response;
    }
    throw new Error(`fetch tak terduga: ${url}`);
  }) as typeof fetch;

  const snap = await createSnapTransaction({
    orderId: "UD-MT-9",
    amountIdr: 1500,
    itemName: "Muse AI Token — Recharge 1B",
  });
  check("snap: token + redirect_url", snap.token === "SNAP-TOKEN-1" && snap.redirectUrl.includes("redirection/abc"));
  const snapCall = fetchCalls[0];
  check("snap: ke sandbox", snapCall.url.includes("app.sandbox.midtrans.com"));
  const snapBody = JSON.parse(snapCall.body) as Record<string, unknown>;
  check(
    "snap: hanya QRIS + expiry 30 menit",
    JSON.stringify(snapBody.enabled_payments) === JSON.stringify(["qris"]) &&
      (snapBody.expiry as { duration?: number })?.duration === 30,
  );
  check(
    "snap: order_id + gross_amount",
    (snapBody.transaction_details as { order_id?: string })?.order_id === "UD-MT-9" &&
      (snapBody.transaction_details as { gross_amount?: number })?.gross_amount === 1500,
  );
  check(
    "snap: Basic auth pakai server key",
    snapCall.headers.Authorization ===
      `Basic ${Buffer.from("TEST-SERVER-KEY:").toString("base64")}`,
  );
  const status = await getTransactionStatus("UD-MT-9");
  check(
    "status: settlement terbaca",
    status.transaction_status === "settlement" && status.payment_type === "qris",
  );
  check(
    "status: ke api sandbox",
    fetchCalls[1].url.includes("api.sandbox.midtrans.com/v2/UD-MT-9/status"),
  );
  const notOpened = await getTransactionStatus("UD-MT-BELUM-DIBUKA");
  check(
    "status: Snap belum dibuka -> pending (bukan gagal)",
    notOpened.transaction_status === "pending" &&
      notOpened.status_code === "404",
  );
  const http404 = await getTransactionStatus("UD-MT-404HTTP");
  check(
    "status: HTTP 404 -> pending juga",
    http404.transaction_status === "pending",
  );

  // ===== Ringkasan =====
  console.log(`\n==== ${passed}/${passed + failed} lolos ====`);
  if (failed > 0) {
    console.log("Gagal:", failures.join(", "));
    process.exit(1);
  }
  // Bersihkan file DB tes (nama file unik per run — lihat atas).
  try {
    const url = process.env.DATABASE_URL ?? "";
    const file = url.replace(/^file:/, "").replace(/\?mode=rwc$/, "");
    unlinkSync(file);
    unlinkSync(`${file}-wal`);
    unlinkSync(`${file}-shm`);
  } catch {
    // file mungkin tidak ada — abaikan
  }
}

main().catch((err) => {
  console.error("Tes crash:", err);
  process.exit(1);
});
