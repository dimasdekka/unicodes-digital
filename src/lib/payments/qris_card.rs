//! Kartu QRIS komposit — port dari `src/lib/payments/qris-card.ts`.
//!
//! QR dinamis di dalam frame standee QRIS Dim. Template: foto standee QRIS
//! (Scaramouche, "UNICODES DIGITAL", NMID, A01) di
//! `src/bot/assets/qris-standee.jpg` — file lokal, TIDAK di-commit.
//! QR statis di tengah template DITIMPA dengan QR dinamis per pembayaran
//! (nominal unik) — pembeli scan kartu ini, bukan QR polos.
//!
//! Geometri (DIUKUR dari piksel template 1252x1252, bukan tebakan):
//!   - Kotak putih: x ~320..930, y ~478..~1065.
//!   - QR lama (bbox modul hitam): x 359..885, y 502..1027 (527x526).
//!   - Tidak ada piksel gelap lain di dalam kotak — jadi menimpa bbox QR
//!     lama saja sudah cukup; quiet zone putih di sekelilingnya (±24..45px)
//!     tetap utuh.
//!   - QR baru digambar tepat di bbox lama (tengah, skala modul bulat).
//!
//! Output PNG lossless supaya kontras modul hitam-putih terjaga — JPEG bisa
//! mengaburkan tepi modul. Kalau template tidak ada di host,
//! `compose_qris_card()` mengembalikan None dan pemanggil memakai QR polos
//! (`render_qris_png`) — bot/web tidak boleh crash gara-gara file template.
//!
//! Beda kecil dari TS: TS menulis encoder PNG sendiri; di sini dipakai
//! crate `image` (PNG lossless, filter adaptif — hasil tetap terbaca
//! scanner yang sama).

use std::io::Cursor;
use std::path::PathBuf;

use super::qris::render_qris_png;

/// Ukuran template standee (px).
pub const STANDEE_WIDTH: u32 = 1252;
pub const STANDEE_HEIGHT: u32 = 1252;

/// Bbox QR lama di template (inklusif) — area yang ditimpa QR dinamis.
/// Diukur dengan scan piksel: modul hitam QR lama x 359..885, y 502..1027.
pub const QRIS_CARD_QR_BOX: QrBox = QrBox {
    x0: 359,
    y0: 502,
    x1: 885,
    y1: 1027,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct QrBox {
    pub x0: u32,
    pub y0: u32,
    pub x1: u32,
    pub y1: u32,
}

/// Path template standee. Bisa di-override env QRIS_STANDEE_PATH (untuk tes).
pub fn standee_template_path() -> PathBuf {
    if let Ok(override_path) = std::env::var("QRIS_STANDEE_PATH") {
        let trimmed = override_path.trim().to_string();
        if !trimmed.is_empty() {
            return PathBuf::from(trimmed);
        }
    }
    std::env::current_dir()
        .unwrap_or_else(|_| PathBuf::from("."))
        .join("src")
        .join("bot")
        .join("assets")
        .join("qris-standee.jpg")
}

/// Tempel QR dinamis (qr_string) ke tengah template standee.
/// Mengembalikan PNG komposit, atau None kalau template tidak ada / tidak
/// bisa dibaca / ukurannya bukan 1252x1252 (pemanggil harus fallback ke
/// QR polos).
pub fn compose_qris_card(qr_string: &str) -> Option<Vec<u8>> {
    compose_qris_card_from_path(qr_string, &standee_template_path())
}

/// Sama seperti `compose_qris_card`, tapi dengan path template eksplisit
/// (dipakai tes supaya tidak menyentuh env proses).
pub fn compose_qris_card_from_path(qr_string: &str, path: &std::path::Path) -> Option<Vec<u8>> {
    let template = std::fs::read(path).ok()?;
    let decoded = image::load_from_memory(&template).ok()?;
    let mut rgba = decoded.to_rgba8();
    let (width, height) = rgba.dimensions();
    if width != STANDEE_WIDTH || height != STANDEE_HEIGHT {
        // Template diganti ukurannya — jangan menimpa di koordinat yang salah.
        return None;
    }
    let code =
        qrcode::QrCode::with_error_correction_level(qr_string.as_bytes(), qrcode::EcLevel::M)
            .ok()?;
    let size = code.width();
    let bx = QRIS_CARD_QR_BOX;
    let box_w = bx.x1 - bx.x0 + 1;
    let box_h = bx.y1 - bx.y0 + 1;
    let scale = std::cmp::max(1, std::cmp::min(box_w, box_h) / size as u32);
    let drawn = scale * size as u32;
    let off_x = bx.x0 + (box_w - drawn) / 2;
    let off_y = bx.y0 + (box_h - drawn) / 2;

    // 1) Hapus QR lama: isi putih area bbox (+6px agar tepi modul lama hilang).
    let pad: u32 = 6;
    let fx0 = bx.x0.saturating_sub(pad);
    let fy0 = bx.y0.saturating_sub(pad);
    let fx1 = std::cmp::min(width - 1, bx.x1 + pad);
    let fy1 = std::cmp::min(height - 1, bx.y1 + pad);
    for y in fy0..=fy1 {
        for x in fx0..=fx1 {
            rgba.put_pixel(x, y, image::Rgba([255, 255, 255, 255]));
        }
    }
    // 2) Gambar modul hitam QR baru (skala bulat -> tepi tajam).
    for my in 0..size {
        for mx in 0..size {
            if code[(mx, my)] != qrcode::Color::Dark {
                continue;
            }
            let px0 = off_x + mx as u32 * scale;
            let py0 = off_y + my as u32 * scale;
            for y in py0..py0 + scale {
                for x in px0..px0 + scale {
                    rgba.put_pixel(x, y, image::Rgba([0, 0, 0, 255]));
                }
            }
        }
    }
    let mut buf = Vec::new();
    image::DynamicImage::ImageRgba8(rgba)
        .write_to(&mut Cursor::new(&mut buf), image::ImageFormat::Png)
        .ok()?;
    Some(buf)
}

/// Hasil render kartu QR — `composited` dipakai tes (memastikan kartu
/// komposit dipakai saat template ada); jalur produksi memakai `png`.
#[allow(dead_code)]
#[derive(Debug, Clone)]
pub struct QrisCardImage {
    pub png: Vec<u8>,
    pub composited: bool,
}

/// Gambar QR untuk pembeli: kartu komposit standee kalau template ada,
/// QR polos kalau tidak. TIDAK pernah gagal karena template hilang —
/// hanya error kalau qr_string-nya sendiri tidak valid.
pub fn render_qris_card_or_plain(qr_string: &str) -> anyhow::Result<QrisCardImage> {
    if let Some(card) = compose_qris_card(qr_string) {
        return Ok(QrisCardImage {
            png: card,
            composited: true,
        });
    }
    Ok(QrisCardImage {
        png: render_qris_png(qr_string, 512)?,
        composited: false,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lib::payments::qris::{build_dynamic_qris, serialize_qris_payload, QrisTlv};
    use image::GenericImageView;

    fn dynamic_payload() -> String {
        let static_payload = serialize_qris_payload(&[
            QrisTlv { tag: "00".into(), value: "01".into() },
            QrisTlv { tag: "01".into(), value: "11".into() },
            QrisTlv {
                tag: "26".into(),
                value: "0014ID.CO.QRIS.WWW011693600123456789010215ID1234567890123".into(),
            },
            QrisTlv { tag: "52".into(), value: "4814".into() },
            QrisTlv { tag: "53".into(), value: "360".into() },
            QrisTlv { tag: "58".into(), value: "ID".into() },
            QrisTlv { tag: "59".into(), value: "TOKO TEST QRIS".into() },
            QrisTlv { tag: "60".into(), value: "JAKARTA".into() },
        ])
        .unwrap();
        build_dynamic_qris(&static_payload, 1501).unwrap()
    }

    #[test]
    fn geometry_constants() {
        assert_eq!((STANDEE_WIDTH, STANDEE_HEIGHT), (1252, 1252));
        assert_eq!(
            QRIS_CARD_QR_BOX,
            QrBox { x0: 359, y0: 502, x1: 885, y1: 1027 }
        );
    }

    #[test]
    fn fallback_without_template() {
        let payload = dynamic_payload();
        assert!(compose_qris_card_from_path(&payload, std::path::Path::new("/tmp/tidak-ada-standee.jpg")).is_none());
        // render_qris_card_or_plain tanpa template di env default test runner:
        // template default tidak ada di cwd tes -> fallback QR polos.
        let fallback = render_qris_card_or_plain(&payload).unwrap();
        assert!(!fallback.composited);
        assert_eq!(&fallback.png[0..4], &[0x89, 0x50, 0x4e, 0x47]);
    }

    #[test]
    fn broken_template_returns_none() {
        // File ada tapi bukan JPEG.
        let path = "/tmp/qris-card-broken-test.txt";
        std::fs::write(path, "bukan gambar").unwrap();
        assert!(compose_qris_card_from_path(
            &dynamic_payload(),
            std::path::Path::new(path)
        )
        .is_none());
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn composite_with_generated_template() {
        // Template putih 1252x1252 (JPEG) yang dibuat di tes — komposit harus
        // jadi PNG 1252x1252 dengan modul hitam di dalam bbox QR.
        let path = "/tmp/qris-card-template-test.jpg";
        let white = image::RgbaImage::from_pixel(1252, 1252, image::Rgba([255, 255, 255, 255]));
        image::DynamicImage::ImageRgba8(white)
            .save_with_format(path, image::ImageFormat::Jpeg)
            .unwrap();
        let card = compose_qris_card_from_path(&dynamic_payload(), std::path::Path::new(path))
            .expect("komposit harus jadi");
        assert_eq!(&card[0..4], &[0x89, 0x50, 0x4e, 0x47]);
        let decoded = image::load_from_memory(&card).unwrap();
        assert_eq!(decoded.dimensions(), (1252, 1252));
        // Ada piksel hitam di dalam bbox QR (modul QR baru tergambar).
        let rgba = decoded.to_rgba8();
        let mut black = 0u32;
        for y in QRIS_CARD_QR_BOX.y0..=QRIS_CARD_QR_BOX.y1 {
            for x in QRIS_CARD_QR_BOX.x0..=QRIS_CARD_QR_BOX.x1 {
                let p = rgba.get_pixel(x, y);
                if p[0] < 50 && p[1] < 50 && p[2] < 50 {
                    black += 1;
                }
            }
        }
        assert!(black > 1000, "modul hitam QR baru tidak ditemukan: {black}");
        // Quiet zone 10px di sekeliling bbox tetap putih.
        for x in (QRIS_CARD_QR_BOX.x0 - 10)..=(QRIS_CARD_QR_BOX.x1 + 10) {
            for y in [QRIS_CARD_QR_BOX.y0 - 10, QRIS_CARD_QR_BOX.y1 + 10] {
                let p = rgba.get_pixel(x, y);
                assert!(p[0] > 240 && p[1] > 240 && p[2] > 240, "quiet zone kotor di ({x},{y})");
            }
        }
        let _ = std::fs::remove_file(path);
    }
}
