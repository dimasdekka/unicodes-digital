// Katalog gabungan — SATU etalase untuk semua supplier.
// Tiap produk menunjuk ke suppliernya + cara fulfillment-nya.
// Pembagian sekarang (sesuai maumu):
//   - Sondev  = Muse
//   - LahaStore = AI lain (Gemini dkk)
//
// Tambah produk baru = tambah 1 objek di bawah ini, dengan:
//   - category: kelompok tampilannya ("Muse" / "AI Lain" / ...).
//     Bot menampilkan pilih kategori dulu, web menampilkan filter kategori.
//   - stockState: "available" (bisa dibeli), "out_of_stock" (stok habis),
//     "coming_soon" (segera hadir). Bot + web menampilkan badge & tombol
//     berbeda per state; produk yang tidak available TIDAK bisa dibeli.
//   - supplier + supplierProductCode + fulfillment sesuai adapter supplier.
// Tambah store baru = lihat src/lib/suppliers/index.ts (cukup tambah
// adapter + daftarkan).
//
// CATATAN: katalog asli LahaStore (banyak produk + kategorinya) BELUM BISA
// ditarik — API key di .env.local ditolak 401 dan key yang benar belum
// dikirim ulang. JANGAN mengarang produk/kategori LahaStore; katalog di sini
// masih 3 produk seed. Begitu key benar tersedia, produk asli tinggal
// ditambahkan di bawah dengan category + stockState masing-masing.
import { retailPrice } from "./pricing";
import type { SupplierId } from "./suppliers/types";

export type Fulfillment =
  | "standard" // order shop biasa via adapter.createOrder
  | "lahastore-xiaomi3" // Gemini Pro Xiaomi 3 bln — butuh akun pembeli (email|password|2FA)
  | "lahastore-sheerid"; // Gemini Student Pro — verifikasi SheerID

/** Status stok produk — menentukan badge & apakah produk bisa dibeli. */
export type StockState = "available" | "out_of_stock" | "coming_soon";

export interface CatalogProduct {
  id: string; // slug internal, dipakai di URL & order
  name: string;
  description: string;
  supplier: SupplierId;
  supplierProductCode: string; // product_code (Sondev) / productId atau code (LahaStore)
  fulfillment: Fulfillment;
  /** Kelompok tampilan di katalog ("Muse" / "AI Lain"). */
  category: string;
  /** Status stok — sumber kebenaran untuk badge & tombol beli. */
  stockState: StockState;
  /** Harga jual ke pembeli (IDR). Dihitung dari harga grosir pakai formula docs (lihat pricing.ts). */
  retailPriceIdr: number;
  active: boolean;
  /** Produk on-demand (mis. Muse) WAJIB target: kode invite / email target pembeli.
   *  Tanpa target, Sondev menolak order dengan 400 INVALID_ORDER (docs v1.2). */
  requiresTarget?: boolean;
  /** Label input target di checkout (mis. "Kode invite Muse"). */
  targetLabel?: string;
  /** DEPRECATED — turunan dari stockState ("coming_soon"). Dipakai sebagian
   *  kode lama; logika baru harus membaca stockState. Selalu jaga selaras:
   *  comingSoon === true hanya bila stockState === "coming_soon". */
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
    name: "Muse AI Token 1B",
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
    category: "Muse",
    stockState: "available",
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
    category: "AI Lain",
    stockState: "coming_soon",
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
    category: "AI Lain",
    stockState: "coming_soon",
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

/** Daftar kategori (urutan kemunculan pertama di katalog) dari produk aktif. */
export function listCategories(): string[] {
  const seen: string[] = [];
  for (const p of listActiveProducts()) {
    if (!seen.includes(p.category)) seen.push(p.category);
  }
  return seen;
}

/** Produk aktif per kategori; "Semua" = semua produk aktif. */
export function listActiveProductsByCategory(category: string): CatalogProduct[] {
  if (category === "Semua") return listActiveProducts();
  return CATALOG.filter((p) => p.active && p.category === category);
}
