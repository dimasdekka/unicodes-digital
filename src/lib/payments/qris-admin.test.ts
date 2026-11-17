// Tes panel admin QRIS GoPay. Jalankan: npx tsx src/lib/payments/qris-admin.test.ts
//
// - DATABASE_URL diarahkan ke file /tmp supaya dev.db live TIDAK kotor.
// - TIDAK ada request jaringan: helper settings + validasi QRIS + auth cookie.
// - Test vector sintetis DISUSUN dari spek EMVCo (bukan QRIS merchant asli),
//   sama seperti di qris.test.ts — jangan dipakai untuk pembayaran sungguhan.
// - Route HTTP-nya sendiri diuji terpisah via smoke test curl ke dev server
//   (handler route memakai next/server yang tidak bisa diimpor di tsx murni).
process.env.DATABASE_URL = `file:/tmp/unicodes-qris-admin-test-${Date.now()}.db`;
process.env.ADMIN_PASSWORD = "testpass-admin";

import { createHash } from "node:crypto";
import { ADMIN_COOKIE } from "../admin-auth";
import { isAdminRequest } from "../admin-api";
import { getSetting, setSetting } from "../db";
import {
  createQrisPayment,
  serializeQrisPayload,
  type QrisTlv,
} from "./qris";
import {
  extractMerchantInfo,
  listRecentQrisPayments,
  validateStaticQris,
} from "./qris-settings";

// --- Test vector sintetis (speks EMVCo, BUKAN merchant asli) ---
const TEST_VECTOR_TLVS: QrisTlv[] = [
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
];
const STATIC_PAYLOAD = serializeQrisPayload(TEST_VECTOR_TLVS);

// --- Runner kecil (gaya harness bot: PASS/FAIL + ringkasan + exit code) ---
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

function throws(fn: () => unknown): boolean {
  try {
    fn();
    return false;
  } catch {
    return true;
  }
}

function adminRequest(cookieValue: string | null): Request {
  const headers = new Headers();
  if (cookieValue !== null) {
    headers.set("Cookie", `${ADMIN_COOKIE}=${cookieValue}; other=1`);
  }
  return new Request("http://localhost/api/admin/payments/qris", { headers });
}

async function main(): Promise<void> {
  // ===== 1. Settings storage =====
  check("settings: key belum ada -> undefined", getSetting("nope") === undefined);
  setSetting("k1", "v1");
  check("settings: round-trip", getSetting("k1") === "v1");
  setSetting("k1", "v2");
  check("settings: upsert menimpa", getSetting("k1") === "v2");
  setSetting("k2", "x");
  check("settings: key lain tidak terganggu", getSetting("k1") === "v2");

  // ===== 2. Validasi QRIS statis =====
  const info = validateStaticQris(STATIC_PAYLOAD);
  check("validasi: payload valid diterima", info.merchantName === "TOKO TEST QRIS");
  check("validasi: kota terbaca", info.merchantCity === "JAKARTA");
  check("validasi: ID merchant terbaca", info.merchantId === "ID1234567890123");

  const badCrc =
    STATIC_PAYLOAD.slice(0, -1) +
    (STATIC_PAYLOAD.slice(-1) === "0" ? "1" : "0");
  check("validasi: checksum salah ditolak", throws(() => validateStaticQris(badCrc)));
  check(
    "validasi: payload terpotong ditolak",
    throws(() => validateStaticQris(STATIC_PAYLOAD.slice(0, 20))),
  );
  check("validasi: string kosong ditolak", throws(() => validateStaticQris("")));
  check(
    "validasi: spasi di ujung diterima (di-trim)",
    validateStaticQris(`  ${STATIC_PAYLOAD}\n`).merchantName === "TOKO TEST QRIS",
  );

  // ===== 3. Extract info tanpa tag merchant =====
  const noMerchant = serializeQrisPayload([
    { tag: "00", value: "01" },
    { tag: "01", value: "11" },
    { tag: "59", value: "TOKO TANPA ID" },
    { tag: "60", value: "BANDUNG" },
  ]);
  const info2 = extractMerchantInfo(noMerchant);
  check(
    "extract: tanpa tag 26 -> merchantId null, nama tetap terbaca",
    info2.merchantId === null && info2.merchantName === "TOKO TANPA ID",
  );
  check(
    "extract: payload bukan QRIS ditolak",
    throws(() => extractMerchantInfo("hello world, bukan qris")),
  );

  // ===== 4. Auth cookie admin (route) =====
  const goodCookie = createHash("sha256").update("testpass-admin").digest("hex");
  check("auth: cookie benar diterima", isAdminRequest(adminRequest(goodCookie)));
  check(
    "auth: cookie salah ditolak",
    !isAdminRequest(adminRequest("00000000000000000000000000000000")),
  );
  check("auth: tanpa cookie ditolak", !isAdminRequest(adminRequest(null)));
  check(
    "auth: header cookie rusak ditolak",
    !isAdminRequest(
      new Request("http://localhost/x", {
        headers: { Cookie: "bukan-cookie-yang-benar" },
      }),
    ),
  );

  // ===== 5. Daftar pembayaran terbaru =====
  check("payments: awalnya kosong", listRecentQrisPayments().length === 0);
  const { payment } = createQrisPayment({
    reference: "ADMIN-TEST-UNIT",
    amountIdr: 1500,
    staticPayload: STATIC_PAYLOAD,
  });
  const recent = listRecentQrisPayments();
  check("payments: 1 baris setelah create", recent.length === 1);
  check(
    "payments: baris berisi reference + nominal unik",
    recent[0]?.reference === "ADMIN-TEST-UNIT" &&
      recent[0]?.unique_amount_idr === payment.unique_amount_idr &&
      recent[0]?.status === "pending",
  );

  // ===== Ringkasan =====
  console.log(`\n==== ${passed}/${passed + failed} lolos ====`);
  if (failed > 0) {
    console.log("Gagal:", failures.join(", "));
    process.exitCode = 1;
  }
}

void main();
