// Katalog gabungan — SATU etalase untuk semua supplier.
// Tiap produk menunjuk ke suppliernya + cara fulfillment-nya.
// Pembagian sekarang (sesuai maumu):
//   - Sondev  = Muse
//   - LahaStore = AI lain (Gemini dkk)
//
// Tambah produk baru = tambah 1 objek di bawah ini. Tambah store baru =
// lihat src/lib/suppliers/index.ts (cukup tambah adapter + daftarkan).
import { retailPrice } from "./pricing";
import type { SupplierId } from "./suppliers/types";

export type Fulfillment =
  | "standard" // order shop biasa via adapter.createOrder
  | "lahastore-xiaomi3" // Gemini Pro Xiaomi 3 bln — butuh akun pembeli (email|password|2FA)
  | "lahastore-sheerid"; // Gemini Student Pro — verifikasi SheerID

export interface CatalogProduct {
  id: string; // slug internal, dipakai di URL & order
  name: string;
  description: string;
  supplier: SupplierId;
  supplierProductCode: string; // product_code (Sondev) / productId atau code (LahaStore)
  fulfillment: Fulfillment;
  /** Harga jual ke pembeli (IDR). Dihitung dari harga grosir pakai formula docs (lihat pricing.ts). */
  retailPriceIdr: number;
  active: boolean;
  /** Produk on-demand (mis. Muse) WAJIB target: kode invite / email target pembeli.
   *  Tanpa target, Sondev menolak order dengan 400 INVALID_ORDER (docs v1.2). */
  requiresTarget?: boolean;
  /** Label input target di checkout (mis. "Kode invite Muse"). */
  targetLabel?: string;
  /** Belum bisa dibeli (mis. saldo supplier kosong) — tampil di katalog sebagai
   *  "Segera hadir", tanpa alasan internal. Pembeli tidak perlu tahu soal
   *  supplier/saldo; info itu hanya ada di /admin. */
  comingSoon?: boolean;
  /** Label kecil di kartu (mis. "Terlaris") — hanya yang relevan buat pembeli. */
  badge?: string;
  /** Satuan harga (mis. "per 1B token") — tampil di samping harga. */
  unit?: string;
  /** Poin-poin keunggulan buat pembeli (tampil sebagai daftar centang). */
  features?: string[];
  /** Warna aksen kartu (gradient). */
  accent?: string;
}

export const CATALOG: CatalogProduct[] = [
  // --- Sondev = Muse ---
  {
    id: "muse-token-1b",
    name: "Muse AI Token — Recharge 1B",
    description: "Recharge 1 miliar token Muse ke akun kamu.",
    supplier: "sondev",
    supplierProductCode: "muse_token",
    fulfillment: "standard",
    // CONTOH — harga final dari katalog live Sondev setelah deploy.
    // Target: jual sedekat mungkin dengan Rp1.000/1B (harga bot Sondev sendiri)
    // kalau grosirnya mengizinkan. Angka Rp8.500/Rp12.000 di docs ternyata
    // hanya contoh di dokumentasi, BUKAN harga live.
    retailPriceIdr: 1500,
    active: true,
    badge: "Terlaris",
    unit: "per 1B token",
    features: [
      "Masuk ke akun kamu dalam hitungan menit",
      "Cukup kirim kode invite / link referral",
      "Harga per 1B — beli sesuai kebutuhan",
    ],
    accent: "#7c3aed",
    // Produk on-demand: pembeli WAJIB isi target (kode invite / link referral).
    requiresTarget: true,
    targetLabel: "Kode invite / link referral Muse (cth: https://muse.ai/invite/3015497298687799)",
  },

  // --- LahaStore = AI lain (Gemini) ---
  {
    id: "gemini-xiaomi-3bln",
    name: "Gemini Pro — Xiaomi 3 bulan",
    description:
      "Gemini Pro 3 bulan via Xiaomi. Saat checkout, isi akun (email|password|2FA) yang mau di-upgrade.",
    supplier: "lahastore",
    supplierProductCode: "xiaomi3",
    fulfillment: "lahastore-xiaomi3",
    retailPriceIdr: 35000,
    active: true,
    comingSoon: true,
    badge: "Hemat",
    unit: "per 3 bulan",
    features: [
      "Gemini Pro penuh selama 3 bulan",
      "Isi akun kamu saat checkout",
      "Tanpa ribet — kami yang proses",
    ],
    accent: "#2563eb",
  },
  {
    id: "gemini-student-pro",
    name: "Gemini Student Pro",
    description: "Gemini Student Pro dengan verifikasi pelajar (SheerID).",
    supplier: "lahastore",
    supplierProductCode: "sheerid",
    fulfillment: "lahastore-sheerid",
    retailPriceIdr: 25000,
    active: true,
    comingSoon: true,
    unit: "per akun",
    features: [
      "Harga pelajar, fitur Pro",
      "Verifikasi pelajar dibantu sampai lolos",
    ],
    accent: "#059669",
  },
];

export function getCatalogProduct(id: string): CatalogProduct {
  const product = CATALOG.find((p) => p.id === id);
  if (!product) throw new Error(`Produk tidak ditemukan: ${id}`);
  return product;
}

export function listActiveProducts(): CatalogProduct[] {
  return CATALOG.filter((p) => p.active);
}
