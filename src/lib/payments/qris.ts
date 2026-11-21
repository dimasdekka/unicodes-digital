// QRIS dinamis — versi sendiri (Fase 1). TypeScript, tanpa library pihak ketiga.
//
// Pendekatan ini meniru POLA kerja library Go paygateme (ATM: amati, tiru,
// modifikasi) — login OTP + polling feed GoBiz BELUM ada di sini (Fase 2).
// Yang ada di modul ini, semuanya ditulis dari nol:
//   1. parser payload QRIS statis format EMVCo (TLV),
//   2. builder QRIS dinamis (suntik tag 54 nominal + tag 53 mata uang IDR,
//      ubah tag 01 jadi dinamis, hitung ulang CRC16),
//   3. generator gambar QR (PNG) dari string payload,
//   4. alokator nominal unik (supaya pembayaran bisa dibedakan),
//   5. store pembayaran QRIS di SQLite,
//   6. matcher transaksi feed -> pembayaran pending,
//   7. expiry 5 menit.
//
// Standar yang dipakai: EMVCo QR Code (dipakai QRIS Indonesia).
// - TLV: tag 2 digit + length 2 digit (PANJANG BYTE UTF-8, bukan karakter)
//   + value.
// - CRC16 tag 63: CRC-16/CCITT-FALSE (poly 0x1021, init 0xFFFF, tanpa
//   refleksi, tanpa XOR akhir), dihitung atas payload termasuk "6304"
//   (tag + length CRC, tanpa nilai), hasilnya 4 hex huruf besar.
// - Tag 01: "11" statis, "12" dinamis. Tag 53: kode mata uang numerik,
//   "360" = IDR. Tag 54: nominal transaksi (rupiah, bilangan bulat).
import { randomUUID } from "node:crypto";
import QRCode from "qrcode";
import { getDb } from "../db";

/** Pembayaran pending kedaluwarsa setelah 5 menit (satu tempat, satu konstanta). */
export const QRIS_PAYMENT_TTL_MS = 5 * 60 * 1000;
/** Kode mata uang EMVCo untuk rupiah. */
export const QRIS_CURRENCY_IDR = "360";
/** Jendela offset nominal unik: delta 1..999 rupiah di atas nominal dasar. */
export const QRIS_MAX_UNIQUE_OFFSET = 999;
/** Toleransi selisih jam saat mencocokkan waktu transaksi feed. */
export const QRIS_CLOCK_SKEW_MS = 60 * 1000;

const TAG_PAYLOAD_FORMAT = "00";
const TAG_POINT_OF_INITIATION = "01";
const TAG_CURRENCY = "53";
const TAG_AMOUNT = "54";
const TAG_CRC = "63";
const POI_STATIC = "11";
const POI_DYNAMIC = "12";

// ---------------------------------------------------------------------------
// CRC-16/CCITT-FALSE (implementasi table-driven, ditulis sendiri)
// ---------------------------------------------------------------------------

const CRC16_POLY = 0x1021;
const CRC16_INIT = 0xffff;

function buildCrc16Table(): Uint16Array {
  const table = new Uint16Array(256);
  for (let n = 0; n < 256; n++) {
    let crc = n << 8;
    for (let i = 0; i < 8; i++) {
      crc =
        (crc & 0x8000) !== 0
          ? ((crc << 1) ^ CRC16_POLY) & 0xffff
          : (crc << 1) & 0xffff;
    }
    table[n] = crc;
  }
  return table;
}

const CRC16_TABLE = buildCrc16Table();

/** CRC-16/CCITT-FALSE atas string (byte UTF-8) -> 4 hex huruf besar. */
export function crc16CcittFalse(input: string): string {
  const bytes = new TextEncoder().encode(input);
  let crc = CRC16_INIT;
  for (const b of bytes) {
    crc = ((crc << 8) ^ CRC16_TABLE[((crc >> 8) ^ b) & 0xff]) & 0xffff;
  }
  return crc.toString(16).toUpperCase().padStart(4, "0");
}

// ---------------------------------------------------------------------------
// Parser + serializer TLV
// ---------------------------------------------------------------------------

export interface QrisTlv {
  tag: string;
  value: string;
}

const TWO_DIGITS = /^\d{2}$/;

/**
 * Parse payload QRIS/EMVCo jadi daftar TLV berurutan (urutan asli dipertahankan,
 * tag yang sama boleh muncul berkali-kali, mis. 26 dan 27). Throw kalau rusak:
 * TLV tidak lengkap, tag/length bukan 2 digit, atau value terpotong.
 * Panjang dihitung dalam BYTE UTF-8 supaya nama merchant non-ASCII tetap benar.
 */
export function parseQrisPayload(payload: string): QrisTlv[] {
  if (typeof payload !== "string" || payload.length < 8) {
    throw new Error("Payload QRIS terlalu pendek atau bukan string");
  }
  const bytes = new TextEncoder().encode(payload);
  const decoder = new TextDecoder("utf-8");
  const out: QrisTlv[] = [];
  let cursor = 0;
  while (cursor < bytes.length) {
    if (cursor + 4 > bytes.length) {
      throw new Error(
        `Payload QRIS rusak di byte ${cursor}: TLV tidak lengkap`,
      );
    }
    const tag = decoder.decode(bytes.subarray(cursor, cursor + 2));
    cursor += 2;
    const lengthText = decoder.decode(bytes.subarray(cursor, cursor + 2));
    cursor += 2;
    if (!TWO_DIGITS.test(tag) || !TWO_DIGITS.test(lengthText)) {
      throw new Error(
        `Payload QRIS rusak di byte ${cursor - 4}: tag/length bukan 2 digit`,
      );
    }
    const length = parseInt(lengthText, 10);
    if (cursor + length > bytes.length) {
      throw new Error(
        `Payload QRIS rusak: tag ${tag} mengaku ${length} byte tapi payload habis`,
      );
    }
    const value = decoder.decode(bytes.subarray(cursor, cursor + length));
    cursor += length;
    out.push({ tag, value });
  }
  return out;
}

/** Ambil nilai tag pertama yang cocok (undefined kalau tidak ada). */
export function getQrisTag(tlvs: QrisTlv[], tag: string): string | undefined {
  return tlvs.find((t) => t.tag === tag)?.value;
}

function setQrisTag(tlvs: QrisTlv[], tag: string, value: string): void {
  const existing = tlvs.find((t) => t.tag === tag);
  if (existing) {
    existing.value = value;
  } else {
    tlvs.push({ tag, value });
  }
}

/**
 * Serialisasi daftar TLV jadi payload lengkap: tag diurutkan menaik secara
 * numerik (CRC selalu terakhir), lalu CRC16 dihitung atas body + "6304"
 * dan ditempel sebagai nilai tag 63.
 */
export function serializeQrisPayload(tlvs: QrisTlv[]): string {
  const encoder = new TextEncoder();
  const bodyParts: string[] = [];
  const sorted = tlvs
    .filter((t) => t.tag !== TAG_CRC)
    .slice()
    .sort((a, b) => Number(a.tag) - Number(b.tag));
  for (const { tag, value } of sorted) {
    const byteLength = encoder.encode(value).length;
    if (byteLength > 99) {
      throw new Error(
        `Nilai tag ${tag} ${byteLength} byte, melebihi batas 99 byte TLV`,
      );
    }
    bodyParts.push(`${tag}${String(byteLength).padStart(2, "0")}${value}`);
  }
  const withCrcHeader = bodyParts.join("") + TAG_CRC + "04";
  return withCrcHeader + crc16CcittFalse(withCrcHeader);
}

/**
 * Validasi CRC trailing payload: CRC dihitung atas seluruh payload kecuali
 * 4 karakter terakhir, lalu dibandingkan (case-insensitive) dengan 4 karakter
 * terakhir itu.
 */
export function isValidQrisChecksum(payload: string): boolean {
  if (typeof payload !== "string" || payload.length < 8) return false;
  const provided = payload.slice(-4).toUpperCase();
  if (!/^[0-9A-F]{4}$/.test(provided)) return false;
  return crc16CcittFalse(payload.slice(0, -4)) === provided;
}

// ---------------------------------------------------------------------------
// Builder QRIS dinamis
// ---------------------------------------------------------------------------

/**
 * Ubah payload QRIS statis jadi dinamis untuk satu nominal: verifikasi dulu
 * checksum sumbernya (payload rusak / hasil edit tangan DITOLAK — supaya QR
 * tidak mengarahkan pembayaran ke merchant lain), lalu set tag 01 = "12",
 * tag 53 = "360" (IDR), tag 54 = nominal rupiah, dan hitung ulang CRC.
 */
export function buildDynamicQris(
  staticPayload: string,
  amountIdr: number,
): string {
  if (!Number.isInteger(amountIdr) || amountIdr <= 0) {
    throw new Error("Nominal QRIS harus bilangan bulat positif (rupiah)");
  }
  if (!isValidQrisChecksum(staticPayload)) {
    throw new Error(
      "Checksum QRIS statis tidak valid — menolak membangun QR bayar dari payload ini",
    );
  }
  const tlvs = parseQrisPayload(staticPayload);
  if (getQrisTag(tlvs, TAG_PAYLOAD_FORMAT) === undefined) {
    throw new Error("Input bukan payload QRIS (tag 00 tidak ada)");
  }
  const next = tlvs.filter((t) => t.tag !== TAG_CRC);
  setQrisTag(next, TAG_POINT_OF_INITIATION, POI_DYNAMIC);
  setQrisTag(next, TAG_CURRENCY, QRIS_CURRENCY_IDR);
  setQrisTag(next, TAG_AMOUNT, String(amountIdr));
  return serializeQrisPayload(next);
}

// ---------------------------------------------------------------------------
// Gambar QR (PNG)
// ---------------------------------------------------------------------------

/**
 * Render string payload jadi PNG (Buffer) yang bisa dikirim bot ke pembeli.
 * Pure JS (package `qrcode`), tanpa request jaringan.
 */
export async function renderQrisPng(
  qrString: string,
  width = 512,
): Promise<Buffer> {
  return QRCode.toBuffer(qrString, {
    type: "png",
    errorCorrectionLevel: "M",
    width,
    margin: 2,
  });
}

// ---------------------------------------------------------------------------
// Alokator nominal unik
// ---------------------------------------------------------------------------

/**
 * Cari offset terkecil 1..maxOffset sehingga nominal dasar + offset belum
 * dipakai pembayaran aktif lain. Keunikan dicek pada HASIL (nominal dasar +
 * offset), bukan offsetnya saja: 3500+1 dan 3499+2 sama-sama mendarat di 3501.
 * Throw kalau semua slot penuh.
 */
export function allocateUniqueAmount(
  baseAmount: number,
  takenUniqueAmounts: Iterable<number>,
  maxOffset: number = QRIS_MAX_UNIQUE_OFFSET,
): { uniqueAmount: number; offset: number } {
  if (!Number.isInteger(baseAmount) || baseAmount <= 0) {
    throw new Error("Nominal dasar harus bilangan bulat positif (rupiah)");
  }
  if (!Number.isInteger(maxOffset) || maxOffset < 1) {
    throw new Error("maxOffset harus bilangan bulat positif");
  }
  const taken = new Set(takenUniqueAmounts);
  for (let offset = 1; offset <= maxOffset; offset++) {
    const candidate = baseAmount + offset;
    if (!taken.has(candidate)) {
      return { uniqueAmount: candidate, offset };
    }
  }
  throw new Error(
    `Tidak ada slot nominal unik untuk ${baseAmount} (offset 1..${maxOffset} sudah terpakai semua)`,
  );
}

// ---------------------------------------------------------------------------
// Store pembayaran QRIS (SQLite, via getDb — pola yang sama dengan modul lain)
// ---------------------------------------------------------------------------

export type QrisPaymentStatus = "pending" | "paid" | "expired" | "cancelled";

export interface QrisPaymentRow {
  id: string;
  reference: string;
  amount_idr: number;
  unique_amount_idr: number;
  qr_string: string;
  status: QrisPaymentStatus;
  created_at: string;
  expires_at: string;
}

/**
 * Buat satu pembayaran QRIS: sapu dulu yang sudah kedaluwarsa (supaya slot
 * nominalnya bebas), alokasikan nominal unik, bangun payload dinamis, simpan
 * sebagai 'pending'. QR PNG-nya dibuat terpisah oleh pemanggil via
 * renderQrisPng(qrString) karena itu async.
 */
export function createQrisPayment(args: {
  reference: string;
  amountIdr: number;
  staticPayload: string;
  now?: Date;
}): { payment: QrisPaymentRow; qrString: string } {
  const now = args.now ?? new Date();
  const createdAt = now.toISOString();
  const expiresAt = new Date(now.getTime() + QRIS_PAYMENT_TTL_MS).toISOString();
  expireQrisPayments(now);
  const taken = listPendingQrisPayments(now).map((p) => p.unique_amount_idr);
  const { uniqueAmount } = allocateUniqueAmount(args.amountIdr, taken);
  const qrString = buildDynamicQris(args.staticPayload, uniqueAmount);
  const id = randomUUID();
  getDb()
    .prepare(
      "INSERT INTO qris_payments (id, reference, amount_idr, unique_amount_idr, qr_string, status, created_at, expires_at) VALUES (?, ?, ?, ?, ?, 'pending', ?, ?)",
    )
    .run(
      id,
      args.reference,
      args.amountIdr,
      uniqueAmount,
      qrString,
      createdAt,
      expiresAt,
    );
  const payment = getQrisPayment(id);
  if (!payment) throw new Error("Gagal membaca kembali pembayaran QRIS yang baru dibuat");
  return { payment, qrString };
}

export function getQrisPayment(id: string): QrisPaymentRow | undefined {
  return getDb()
    .prepare("SELECT * FROM qris_payments WHERE id = ?")
    .get(id) as QrisPaymentRow | undefined;
}

/** Pembayaran QRIS terbaru untuk satu referensi (order UD-… / top-up TOPUP-…). */
export function getQrisPaymentByReference(
  reference: string,
): QrisPaymentRow | undefined {
  return getDb()
    .prepare(
      "SELECT * FROM qris_payments WHERE reference = ? ORDER BY created_at DESC LIMIT 1",
    )
    .get(reference) as QrisPaymentRow | undefined;
}

/** Pembayaran pending yang belum kedaluwarsa (nominalnya masih "dipegang"). */
export function listPendingQrisPayments(now: Date = new Date()): QrisPaymentRow[] {
  return getDb()
    .prepare(
      "SELECT * FROM qris_payments WHERE status = 'pending' AND expires_at > ? ORDER BY created_at ASC",
    )
    .all(now.toISOString()) as unknown as QrisPaymentRow[];
}

/** Tandai semua pending yang lewat expires_at jadi 'expired'. Kembalikan jumlahnya. */
export function expireQrisPayments(now: Date = new Date()): number {
  const result = getDb()
    .prepare(
      "UPDATE qris_payments SET status = 'expired' WHERE status = 'pending' AND expires_at <= ?",
    )
    .run(now.toISOString());
  return Number(result.changes);
}

/** Daftar pembayaran pending yang sudah lewat expires_at (belum disapu).
 *  Dipakai poller supaya order yang QR-nya kedaluwarsa bisa dibatalkan. */
export function listExpiredPendingQrisPayments(now: Date = new Date()): QrisPaymentRow[] {
  return getDb()
    .prepare(
      "SELECT * FROM qris_payments WHERE status = 'pending' AND expires_at <= ? ORDER BY created_at ASC",
    )
    .all(now.toISOString()) as unknown as QrisPaymentRow[];
}

/** Tandai pending jadi 'paid'. False kalau id tidak ada / sudah bukan pending. */
export function markQrisPaymentPaid(id: string): boolean {
  const result = getDb()
    .prepare(
      "UPDATE qris_payments SET status = 'paid' WHERE id = ? AND status = 'pending'",
    )
    .run(id);
  return result.changes > 0;
}

/** Batalkan pending (nominal uniknya bebas lagi untuk pembayaran berikutnya). */
export function markQrisPaymentCancelled(id: string): boolean {
  const result = getDb()
    .prepare(
      "UPDATE qris_payments SET status = 'cancelled' WHERE id = ? AND status = 'pending'",
    )
    .run(id);
  return result.changes > 0;
}

// ---------------------------------------------------------------------------
// Matcher transaksi feed -> pembayaran pending
// ---------------------------------------------------------------------------

export interface FeedTransaction {
  /** ID transaksi di feed provider (untuk dedupe di reconcile). */
  id: string;
  /** Nominal yang masuk (rupiah). */
  amountIdr: number;
  /** Waktu transaksi; null kalau feed tidak memberi waktu (cocok nominal saja). */
  time: Date | string | null;
  /** Status dari feed; kosong/tidak dikenal = dianggap sukses (fail-open). */
  status?: string;
}

const FEED_FAILURE_STATUSES = new Set([
  "failed",
  "reversed",
  "cancelled",
  "canceled",
  "expired",
  "declined",
]);

/**
 * Apakah satu transaksi feed melunasi satu pembayaran pending?
 * Aturan: nominal harus SAMA PERSIS dengan unique_amount pembayaran; waktu
 * transaksi harus di dalam jendela [created_at, expires_at] (dengan toleransi
 * selisih jam); status gagal yang sudah dikenal ditolak; status kosong/tidak
 * dikenal diterima (fail-open — feed tiap provider beda-beda labelnya).
 */
export function matchFeedTransaction(
  tx: FeedTransaction,
  payment: QrisPaymentRow,
  opts: { clockSkewMs?: number } = {},
): boolean {
  if (!tx || !Number.isFinite(tx.amountIdr)) return false;
  if (payment.status !== "pending") return false;
  if (tx.amountIdr !== payment.unique_amount_idr) return false;
  const status = (tx.status ?? "").trim().toLowerCase();
  if (status && FEED_FAILURE_STATUSES.has(status)) return false;
  if (tx.time === null || tx.time === undefined) return true;
  const txTime = tx.time instanceof Date ? tx.time : new Date(tx.time);
  if (Number.isNaN(txTime.getTime())) return false;
  const skew = opts.clockSkewMs ?? QRIS_CLOCK_SKEW_MS;
  const start = new Date(payment.created_at).getTime() - skew;
  const end = new Date(payment.expires_at).getTime() + skew;
  const t = txTime.getTime();
  return t >= start && t <= end;
}

/** Cari pembayaran pending pertama yang cocok dengan satu transaksi feed. */
export function findQrisPaymentForTransaction(
  tx: FeedTransaction,
  pending: QrisPaymentRow[],
  opts: { clockSkewMs?: number } = {},
): QrisPaymentRow | null {
  for (const payment of pending) {
    if (matchFeedTransaction(tx, payment, opts)) return payment;
  }
  return null;
}

/**
 * Cocokkan banyak transaksi ke banyak pembayaran pending. Setiap transaksi
 * dipakai paling banyak satu kali (supaya tidak melunasi ganda), pembayaran
 * diurutkan dari unique_amount terkecil supaya hasilnya deterministik.
 */
export function reconcileFeedTransactions(
  pending: QrisPaymentRow[],
  transactions: FeedTransaction[],
  opts: { clockSkewMs?: number } = {},
): { payment: QrisPaymentRow; transaction: FeedTransaction }[] {
  const ordered = [...pending].sort(
    (a, b) => a.unique_amount_idr - b.unique_amount_idr,
  );
  const used = new Set<string>();
  const matches: { payment: QrisPaymentRow; transaction: FeedTransaction }[] = [];
  for (const payment of ordered) {
    for (const tx of transactions) {
      if (used.has(tx.id)) continue;
      if (matchFeedTransaction(tx, payment, opts)) {
        matches.push({ payment, transaction: tx });
        used.add(tx.id);
        break;
      }
    }
  }
  return matches;
}
