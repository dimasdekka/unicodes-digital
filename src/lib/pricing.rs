//! Helper harga & margin — port `src/lib/pricing.ts`.
//!
//! Kurs di bawah adalah nilai saat dicek 2026-11-17 —
//! TODO: ambil kurs live (atau set manual tiap hari) sebelum dipakai produksi.
//! LahaStore: VND/USD dari /balance (26.500 saat dicek). Sondev: harga sudah ada
//! dalam IDR dan USD di /products, jadi tidak perlu konversi.

//! Helper port dari TS pricing.ts — pemakaian produksi tidak langsung
//! (harga grosir dibaca dari /products supplier). Fungsinya hidup untuk
//! tes + referensi konversi, jadi dead-code diizinkan di modul ini.
#![allow(dead_code)]

pub const VND_PER_USD: i64 = 26500;
pub const IDR_PER_USD: i64 = 16500;

pub fn vnd_to_idr(vnd: i64) -> i64 {
    // Math.round((vnd / VND_PER_USD) * IDR_PER_USD) — pakai f64 seperti TS.
    ((vnd as f64 / VND_PER_USD as f64) * IDR_PER_USD as f64).round() as i64
}

pub fn usd_to_idr(usd: f64) -> i64 {
    (usd * IDR_PER_USD as f64).round() as i64
}

/// Harga jual = harga grosir (IDR) + margin yang kamu mau.
pub fn retail_from_wholesale(wholesale_idr: i64, margin_idr: i64) -> i64 {
    wholesale_idr + margin_idr
}

/// Formula harga jual resmi Sondev (docs v1.2):
///   retail = grosir × (1 + margin%) + margin flat, dibulatkan ke atas ke 500 terdekat.
/// Default: margin 25% + Rp1.000 flat.
/// Contoh: grosir 8.500 → 8.500 × 1,25 + 1.000 = 11.625 → Rp12.000.
pub fn retail_price(wholesale_idr: i64, margin_pct: f64, margin_flat_idr: i64) -> i64 {
    let price = wholesale_idr as f64 * (1.0 + margin_pct / 100.0) + margin_flat_idr as f64;
    ((price / 500.0).ceil() as i64) * 500
}

/// `retail_price` dengan default TS: margin 25% + Rp1.000 flat.
pub fn retail_price_default(wholesale_idr: i64) -> i64 {
    retail_price(wholesale_idr, 25.0, 1000)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vnd_to_idr_uses_26500_rate() {
        // 26.500 VND = $1 = Rp16.500.
        assert_eq!(vnd_to_idr(26500), 16500);
        assert_eq!(vnd_to_idr(53000), 33000);
        assert_eq!(vnd_to_idr(0), 0);
    }

    #[test]
    fn usd_to_idr_conversions() {
        assert_eq!(usd_to_idr(1.0), 16500);
        assert_eq!(usd_to_idr(0.5), 8250);
        assert_eq!(usd_to_idr(2.0), 33000);
    }

    #[test]
    fn retail_price_ceils_to_500() {
        // Contoh resmi docs Sondev v1.2: grosir 8.500 -> 8.500 x 1,25 +
        // 1.000 = 11.625 -> Rp12.000.
        assert_eq!(retail_price(8500, 25.0, 1000), 12000);
        assert_eq!(retail_price_default(8500), 12000);
        // Sudah kelipatan 500: tetap.
        assert_eq!(retail_price(8000, 25.0, 1000), 11000);
        // Tanpa margin: 800 -> ceil ke 1000.
        assert_eq!(retail_price(800, 0.0, 0), 1000);
    }

    #[test]
    fn retail_from_wholesale_adds_margin_ok() {
        assert_eq!(retail_from_wholesale(800, 1200), 2000);
        assert_eq!(retail_from_wholesale(0, 0), 0);
    }

    #[test]
    fn muse_retail_matches_catalog() {
        // Grosir Sondev Rp800/1B -> retail katalog Rp2.000 (margin Rp1.200).
        assert_eq!(retail_from_wholesale(800, 1200), 2000);
    }
}
