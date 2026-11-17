// Tes modul QRIS dinamis (Fase 1). Jalankan: npx tsx src/lib/payments/qris.test.ts
//
// - DATABASE_URL diarahkan ke file /tmp supaya dev.db live TIDAK kotor.
// - TIDAK ada request jaringan: semua pure function + SQLite lokal + render
//   QR (package `qrcode`, pure JS).
// - Test vector di bawah ini DISUSUN dari spek EMVCo (bukan QRIS merchant
//   asli): nama merchant "TOKO TEST QRIS", PAN dan ID merchant sintetis.
//   Jangan pakai vector ini untuk pembayaran sungguhan.
// - CRC16 diverifikasi dengan implementasi INDEPENDEN di file ini (bitwise),
//   berbeda jalur kode dari implementasi table-driven di qris.ts — jadi tes
//   tidak sekadar memeriksa modul terhadap dirinya sendiri.
process.env.DATABASE_URL = `file:/tmp/unicodes-qris-test-${Date.now()}.db`;

import {
  QRIS_PAYMENT_TTL_MS,
  allocateUniqueAmount,
  buildDynamicQris,
  createQrisPayment,
  crc16CcittFalse,
  expireQrisPayments,
  findQrisPaymentForTransaction,
  getQrisPayment,
  getQrisTag,
  isValidQrisChecksum,
  listPendingQrisPayments,
  markQrisPaymentCancelled,
  markQrisPaymentPaid,
  parseQrisPayload,
  reconcileFeedTransactions,
  renderQrisPng,
  serializeQrisPayload,
  type FeedTransaction,
  type QrisTlv,
} from "./qris";

// --- CRC16 independen (bitwise, jalur berbeda dari table-driven di qris.ts) ---
function crc16Independent(input: string): string {
  const bytes = new TextEncoder().encode(input);
  let crc = 0xffff;
  for (const b of bytes) {
    crc ^= b << 8;
    for (let i = 0; i < 8; i++) {
      crc = (crc & 0x8000) !== 0 ? ((crc << 1) ^ 0x1021) & 0xffff : (crc << 1) & 0xffff;
    }
  }
  return crc.toString(16).toUpperCase().padStart(4, "0");
}

// --- Test vector sintetis (speks EMVCo, BUKAN merchant asli) ---
// Tag 26 berisi nested TLV: 00 GUI "ID.CO.QRIS.WWW" (14),
// 01 PAN sintetis (16), 02 ID merchant sintetis (15) -> total 57 byte.
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

function must<T>(v: T | undefined | null, msg: string): T {
  if (v === undefined || v === null) throw new Error(`must: ${msg}`);
  return v;
}

function throws(fn: () => unknown): boolean {
  try {
    fn();
    return false;
  } catch {
    return true;
  }
}

async function main(): Promise<void> {
  // ===== 1. Parser + round-trip =====
  const parsed = parseQrisPayload(STATIC_PAYLOAD);
  check("parse: 9 tag terbaca (8 + CRC 63)", parsed.length === 9, `dapat ${parsed.length}`);
  check(
    "parse: tag terakhir = CRC 63 (4 hex)",
    parsed[8]?.tag === "63" && /^[0-9A-F]{4}$/.test(parsed[8]?.value ?? ""),
  );
  check(
    "parse: tag 26 nested utuh",
    getQrisTag(parsed, "26") ===
      "0014ID.CO.QRIS.WWW011693600123456789010215ID1234567890123",
  );
  check(
    "round-trip: serialize(parse(x)) === x",
    serializeQrisPayload(parsed) === STATIC_PAYLOAD,
  );
  check(
    "round-trip: tag 59 non-ASCII aman (TOKO TEST QRIS)",
    getQrisTag(parsed, "59") === "TOKO TEST QRIS",
  );
  check("parser: payload terpotong ditolak", throws(() => parseQrisPayload("00020101021126")));
  check("parser: length bukan digit ditolak", throws(() => parseQrisPayload("00AB01")));
  check("parser: string kosong ditolak", throws(() => parseQrisPayload("")));

  // ===== 2. CRC16 (modul vs independen) =====
  check("CRC: checksum vector valid", isValidQrisChecksum(STATIC_PAYLOAD));
  check(
    "CRC: modul == independen",
    crc16CcittFalse(STATIC_PAYLOAD.slice(0, -4)) ===
      crc16Independent(STATIC_PAYLOAD.slice(0, -4)),
  );
  check(
    "CRC: independen cocok dengan 4 char terakhir",
    crc16Independent(STATIC_PAYLOAD.slice(0, -4)) === STATIC_PAYLOAD.slice(-4),
  );
  const tampered = STATIC_PAYLOAD.replace("TOKO TEST QRIS", "TOKO TEST QRIT");
  check("CRC: payload diutak-atik ditolak", !isValidQrisChecksum(tampered));
  check("CRC: string pendek ditolak", !isValidQrisChecksum("1234"));

  // ===== 3. Builder dinamis =====
  const dynamic = buildDynamicQris(STATIC_PAYLOAD, 1500);
  const dynParsed = parseQrisPayload(dynamic);
  check("dinamis: tag 01 jadi 12", getQrisTag(dynParsed, "01") === "12");
  check("dinamis: tag 53 = 360", getQrisTag(dynParsed, "53") === "360");
  check("dinamis: tag 54 = 1500", getQrisTag(dynParsed, "54") === "1500");
  check("dinamis: berisi 54041500", dynamic.includes("54041500"));
  check("dinamis: CRC valid (independen)", isValidQrisChecksum(dynamic));
  check(
    "dinamis: CRC independen cocok",
    crc16Independent(dynamic.slice(0, -4)) === dynamic.slice(-4),
  );
  check(
    "dinamis: tag 26 merchant tidak berubah",
    getQrisTag(dynParsed, "26") === getQrisTag(parsed, "26"),
  );
  check("dinamis: nominal 0 ditolak", throws(() => buildDynamicQris(STATIC_PAYLOAD, 0)));
  check("dinamis: nominal negatif ditolak", throws(() => buildDynamicQris(STATIC_PAYLOAD, -5)));
  check("dinamis: nominal desimal ditolak", throws(() => buildDynamicQris(STATIC_PAYLOAD, 1500.5)));
  check("dinamis: payload checksum rusak ditolak", throws(() => buildDynamicQris(tampered, 1500)));
  check(
    "dinamis: nominal besar (Rp10.000.000) lolos",
    getQrisTag(parseQrisPayload(buildDynamicQris(STATIC_PAYLOAD, 10_000_000)), "54") === "10000000",
  );

  // ===== 4. Gambar QR PNG =====
  const png = await renderQrisPng(dynamic);
  check("QR PNG: buffer tidak kosong", png.length > 500, `${png.length} byte`);
  check(
    "QR PNG: magic bytes PNG",
    png[0] === 0x89 && png[1] === 0x50 && png[2] === 0x4e && png[3] === 0x47,
  );

  // ===== 5. Alokator nominal unik =====
  check(
    "alokator: kosong -> 1501 (offset 1)",
    JSON.stringify(allocateUniqueAmount(1500, [])) ===
      JSON.stringify({ uniqueAmount: 1501, offset: 1 }),
  );
  check(
    "alokator: 1501 terpakai -> 1502",
    allocateUniqueAmount(1500, [1501]).uniqueAmount === 1502,
  );
  const taken = new Set<number>();
  const allocated: number[] = [];
  for (let i = 0; i < 5; i++) {
    const r = allocateUniqueAmount(1500, taken);
    taken.add(r.uniqueAmount);
    allocated.push(r.uniqueAmount);
  }
  check(
    "alokator: 5 alokasi berurutan semua beda",
    new Set(allocated).size === 5 && allocated.join(",") === "1501,1502,1503,1504,1505",
    allocated.join(","),
  );
  check(
    "alokator: keunikan di HASIL, bukan offset (3499+2 == 3500+1)",
    allocateUniqueAmount(3499, [3501]).uniqueAmount === 3500,
  );
  check(
    "alokator: pool habis -> throw",
    throws(() => allocateUniqueAmount(1500, [1501, 1502], 2)),
  );

  // ===== 6. Store SQLite =====
  const now = new Date();
  const p1 = createQrisPayment({
    reference: "TEST-ORDER-1",
    amountIdr: 1500,
    staticPayload: STATIC_PAYLOAD,
    now,
  });
  check("store: p1 unique 1501", p1.payment.unique_amount_idr === 1501);
  check("store: p1 qr berisi 54041501", p1.qrString.includes("54041501"));
  check("store: p1 status pending", p1.payment.status === "pending");
  check(
    "store: p1 expires = created + 5 menit",
    new Date(p1.payment.expires_at).getTime() -
      new Date(p1.payment.created_at).getTime() ===
      QRIS_PAYMENT_TTL_MS,
  );
  const p2 = createQrisPayment({
    reference: "TEST-ORDER-2",
    amountIdr: 1500,
    staticPayload: STATIC_PAYLOAD,
    now,
  });
  check("store: p2 unique 1502", p2.payment.unique_amount_idr === 1502);
  check(
    "store: get by id",
    must(getQrisPayment(p1.payment.id), "p1 hilang").reference === "TEST-ORDER-1",
  );
  check("store: pending ada 2", listPendingQrisPayments(now).length === 2);
  check("store: cancel p1 berhasil", markQrisPaymentCancelled(p1.payment.id) === true);
  check("store: cancel 2x gagal (sudah bukan pending)", markQrisPaymentCancelled(p1.payment.id) === false);
  check("store: pending tinggal 1 setelah cancel", listPendingQrisPayments(now).length === 1);
  const p3 = createQrisPayment({
    reference: "TEST-ORDER-3",
    amountIdr: 1500,
    staticPayload: STATIC_PAYLOAD,
    now,
  });
  check("store: 1501 bebas lagi setelah cancel", p3.payment.unique_amount_idr === 1501);
  // Expiry: buat pembayaran 6 menit lalu (sudah lewat TTL 5 menit).
  const past = new Date(Date.now() - 6 * 60 * 1000);
  const pOld = createQrisPayment({
    reference: "TEST-ORDER-OLD",
    amountIdr: 1500,
    staticPayload: STATIC_PAYLOAD,
    now: past,
  });
  check("store: pembayaran lama dapat 1503", pOld.payment.unique_amount_idr === 1503);
  const expiredCount = expireQrisPayments(new Date());
  check("store: sweep expiry menandai 1", expiredCount === 1, `dapat ${expiredCount}`);
  check(
    "store: status lama jadi expired",
    must(getQrisPayment(pOld.payment.id), "pOld hilang").status === "expired",
  );
  const p4 = createQrisPayment({
    reference: "TEST-ORDER-4",
    amountIdr: 1500,
    staticPayload: STATIC_PAYLOAD,
    now,
  });
  check("store: 1503 bebas lagi setelah expiry", p4.payment.unique_amount_idr === 1503);
  check("store: bayar p2 berhasil", markQrisPaymentPaid(p2.payment.id) === true);
  check(
    "store: pending tinggal p3+p4",
    listPendingQrisPayments(now).length === 2,
  );

  // ===== 7. Matcher =====
  const pending = listPendingQrisPayments(now); // p3 (1501), p4 (1503)
  const txOk: FeedTransaction = {
    id: "tx1",
    amountIdr: 1501,
    time: new Date(),
    status: "settlement",
  };
  check(
    "matcher: nominal + waktu cocok -> p3",
    findQrisPaymentForTransaction(txOk, pending)?.id === p3.payment.id,
  );
  check(
    "matcher: nominal salah -> null",
    findQrisPaymentForTransaction(
      { id: "tx2", amountIdr: 1502, time: new Date() },
      pending,
    ) === null,
  );
  check(
    "matcher: waktu 10 mnt setelah expiry -> null",
    findQrisPaymentForTransaction(
      { id: "tx3", amountIdr: 1503, time: new Date(Date.now() + 10 * 60 * 1000) },
      pending,
    ) === null,
  );
  check(
    "matcher: waktu 10 mnt sebelum created -> null",
    findQrisPaymentForTransaction(
      { id: "tx4", amountIdr: 1501, time: new Date(Date.now() - 10 * 60 * 1000) },
      pending,
    ) === null,
  );
  check(
    "matcher: status failed ditolak",
    findQrisPaymentForTransaction(
      { id: "tx5", amountIdr: 1503, time: new Date(), status: "failed" },
      pending,
    ) === null,
  );
  check(
    "matcher: status kosong fail-open -> cocok",
    findQrisPaymentForTransaction(
      { id: "tx6", amountIdr: 1503, time: new Date() },
      pending,
    )?.id === p4.payment.id,
  );
  check(
    "matcher: tanpa waktu -> cocok nominal saja",
    findQrisPaymentForTransaction(
      { id: "tx7", amountIdr: 1501, time: null },
      pending,
    )?.id === p3.payment.id,
  );
  check(
    "matcher: pending kosong -> null",
    findQrisPaymentForTransaction(txOk, []) === null,
  );
  const reconciled = reconcileFeedTransactions(pending, [
    txOk,
    { id: "tx8", amountIdr: 1503, time: new Date(), status: "paid" },
    { id: "tx9", amountIdr: 1501, time: new Date(), status: "paid" }, // duplikat nominal
  ]);
  check("reconcile: 2 match (tx duplikat tidak dipakai 2x)", reconciled.length === 2);
  check(
    "reconcile: tiap tx dipakai sekali",
    new Set(reconciled.map((m) => m.transaction.id)).size === 2,
  );

  // ===== Ringkasan =====
  console.log(`\n==== ${passed}/${passed + failed} lolos ====`);
  if (failed > 0) {
    console.log("Gagal:", failures.join(", "));
    process.exit(1);
  }
}

main().catch((err) => {
  console.error("Tes crash:", err);
  process.exit(1);
});
