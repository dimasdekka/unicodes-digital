// Registry supplier. Semua kode lain (katalog, order, checkout) hanya kenal
// interface SupplierAdapter — mereka tidak tahu/tidak peduli supplier mana
// yang dipakai di baliknya.
//
// CARA TAMBAH STORE BARU (ke-3, ke-4, dst):
//   1. Buat file src/lib/suppliers/namastore.ts — class yang implement
//      SupplierAdapter dari ./types (contoh: sondev.ts / lahastore.ts).
//   2. Tambah id-nya ke union SupplierId di ./types.ts.
//   3. Daftarkan di SUPPLIERS di bawah ini.
//   4. Tambah produknya di src/lib/catalog.ts (supplier: "namastore").
// Selesai — tidak ada kode lain yang perlu diubah.
import { LahaStoreAdapter } from "./lahastore";
import { SondevAdapter } from "./sondev";
import type { SupplierAdapter, SupplierId } from "./types";

export const SUPPLIERS: Record<SupplierId, SupplierAdapter> = {
  // Sondev = Muse
  sondev: new SondevAdapter(),
  // LahaStore = AI lain (Gemini dkk)
  lahastore: new LahaStoreAdapter(),
};

export function getSupplier(id: SupplierId): SupplierAdapter {
  const supplier = SUPPLIERS[id];
  if (!supplier) throw new Error(`Supplier belum terdaftar: ${id}`);
  return supplier;
}
