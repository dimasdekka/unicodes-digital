// Pengaturan QRIS GoPay (merchant) untuk panel admin.
// String QRIS statis disimpan di tabel settings (bukan di env/file),
// supaya Dim bisa menempelnya sendiri dari /admin tanpa kirim lewat chat.
// Nilai ini TIDAK pernah di-echo lengkap ke client — API hanya mengembalikan
// info merchant hasil parse (nama/kota/ID).
import { getDb, getSetting, setSetting } from "../db";
import {
  getQrisTag,
  isValidQrisChecksum,
  parseQrisPayload,
  type QrisPaymentRow,
} from "./qris";

/** Key settings untuk string QRIS statis GoBiz. */
export const GOPAY_STATIC_QRIS_KEY = "gopay_static_qris";

export function getGopayStaticQris(): string | undefined {
  return getSetting(GOPAY_STATIC_QRIS_KEY);
}

export function setGopayStaticQris(value: string): void {
  setSetting(GOPAY_STATIC_QRIS_KEY, value);
}

export interface QrisMerchantInfo {
  merchantName: string | null;
  merchantCity: string | null;
  merchantId: string | null;
}

/**
 * Ambil info merchant dari payload QRIS statis: tag 59 (nama), tag 60 (kota),
 * dan sub-tag 02 di dalam tag 26 (ID merchant; fallback tag 27 kalau 26 kosong).
 * Throw dengan pesan jelas kalau payload bukan QRIS / rusak.
 */
export function extractMerchantInfo(payload: string): QrisMerchantInfo {
  const trimmed = (payload ?? "").trim();
  if (!trimmed) {
    throw new Error("String QRIS kosong");
  }
  let tlvs;
  try {
    tlvs = parseQrisPayload(trimmed);
  } catch (err) {
    throw new Error(
      `String QRIS tidak bisa dibaca: ${err instanceof Error ? err.message : "payload rusak"}`,
    );
  }
  if (getQrisTag(tlvs, "00") === undefined) {
    throw new Error("Bukan payload QRIS (tag 00 tidak ada)");
  }
  let merchantId: string | null = null;
  for (const tag of ["26", "27"]) {
    const nested = getQrisTag(tlvs, tag);
    if (!nested) continue;
    try {
      const sub = parseQrisPayload(nested);
      const id = getQrisTag(sub, "02");
      if (id) {
        merchantId = id;
        break;
      }
    } catch {
      // Tag merchant rusak — abaikan, lanjut ke tag berikutnya.
    }
  }
  return {
    merchantName: getQrisTag(tlvs, "59") ?? null,
    merchantCity: getQrisTag(tlvs, "60") ?? null,
    merchantId,
  };
}

/**
 * Validasi string QRIS statis sebelum disimpan: payload harus bisa di-parse,
 * checksum CRC16 harus valid (payload hasil edit tangan / terpotong ditolak),
 * dan harus ada tag 00. Kembalikan info merchant kalau valid.
 */
export function validateStaticQris(payload: string): QrisMerchantInfo {
  const trimmed = (payload ?? "").trim();
  if (!trimmed) {
    throw new Error("String QRIS kosong");
  }
  if (!isValidQrisChecksum(trimmed)) {
    throw new Error(
      "Checksum QRIS tidak valid — string mungkin terpotong atau salah salin. Salin ulang dari GoBiz.",
    );
  }
  return extractMerchantInfo(trimmed);
}

/** 20 pembayaran QRIS terbaru (untuk tabel admin). */
export function listRecentQrisPayments(limit = 20): QrisPaymentRow[] {
  return getDb()
    .prepare(
      "SELECT * FROM qris_payments ORDER BY created_at DESC LIMIT ?",
    )
    .all(limit) as unknown as QrisPaymentRow[];
}
