// Helper harga & margin. Kurs di bawah adalah nilai saat dicek 2026-11-17 —
// TODO: ambil kurs live (atau set manual tiap hari) sebelum dipakai produksi.
// LahaStore: VND/USD dari /balance (26.500 saat dicek). Sondev: harga sudah ada
// dalam IDR dan USD di /products, jadi tidak perlu konversi.
export const VND_PER_USD = 26500;
export const IDR_PER_USD = 16500;

export function vndToIdr(vnd: number): number {
  return Math.round((vnd / VND_PER_USD) * IDR_PER_USD);
}

export function usdToIdr(usd: number): number {
  return Math.round(usd * IDR_PER_USD);
}

/** Harga jual = harga grosir (IDR) + margin yang kamu mau. */
export function retailFromWholesale(
  wholesaleIdr: number,
  marginIdr: number,
): number {
  return wholesaleIdr + marginIdr;
}

/**
 * Formula harga jual resmi Sondev (docs v1.2):
 *   retail = grosir × (1 + margin%) + margin flat, dibulatkan ke atas ke 500 terdekat.
 * Default: margin 25% + Rp1.000 flat.
 * Contoh: grosir 8.500 → 8.500 × 1,25 + 1.000 = 11.625 → Rp12.000.
 * (Setelah MDR QRIS ~0,7%: 12.000 − 84 − 8.500 = margin bersih Rp3.416 / 28%.)
 */
export function retailPrice(
  wholesaleIdr: number,
  marginPct = 25,
  marginFlatIdr = 1000,
): number {
  const price = wholesaleIdr * (1 + marginPct / 100) + marginFlatIdr;
  return Math.ceil(price / 500) * 500;
}
