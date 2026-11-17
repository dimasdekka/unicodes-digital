// Kartu QRIS komposit — QR dinamis di dalam frame standee QRIS Dim.
// Template: foto standee QRIS (Scaramouche, "UNICODES DIGITAL", NMID,
// A01) di src/bot/assets/qris-standee.jpg — file lokal, TIDAK di-commit
// (lihat .gitignore + DEPLOY.md). QR statis di tengah template DITIMPA
// dengan QR dinamis per pembayaran (nominal unik) — pembeli scan kartu ini,
// bukan QR polos.
//
// Geometri (DIUKUR dari piksel template 1252x1252, bukan tebakan):
//   - Kotak putih: x ~320..930, y ~478..~1065.
//   - QR lama (bbox modul hitam): x 359..885, y 502..1027 (527x526).
//   - Tidak ada piksel gelap lain di dalam kotak — jadi menimpa bbox QR
//     lama saja sudah cukup; quiet zone putih di sekelilingnya (±24..45px)
//     tetap utuh.
//   - QR baru digambar tepat di bbox lama (tengah, skala modul bulat).
//
// Output PNG lossless (encoder ditulis sendiri, tanpa dep baru) supaya
// kontras modul hitam-putih terjaga — JPEG bisa mengaburkan tepi modul.
// Kalau template tidak ada di host, composeQrisCard() mengembalikan null
// dan pemanggil memakai QR polos (renderQrisPng) — bot/web tidak boleh
// crash gara-gara file template.
import { deflateSync } from "node:zlib";
import fs from "node:fs";
import path from "node:path";
import jpeg from "jpeg-js";
import QRCode from "qrcode";
import { renderQrisPng } from "./qris";

/** Ukuran template standee (px). */
export const STANDEE_WIDTH = 1252;
export const STANDEE_HEIGHT = 1252;

/**
 * Bbox QR lama di template (inklusif) — area yang ditimpa QR dinamis.
 * Diukur dengan scan piksel (lihat /tmp/measure-standee2.mjs):
 * modul hitam QR lama x 359..885, y 502..1027.
 */
export const QRIS_CARD_QR_BOX = { x0: 359, y0: 502, x1: 885, y1: 1027 };

/** Path template standee. Bisa di-override env QRIS_STANDEE_PATH (untuk tes). */
export function standeeTemplatePath(): string {
  const override = (process.env.QRIS_STANDEE_PATH ?? "").trim();
  if (override) return override;
  return path.join(process.cwd(), "src", "bot", "assets", "qris-standee.jpg");
}

// ---------------------------------------------------------------------------
// Encoder PNG minimal (RGBA, filter 0, tanpa interlace) — lossless.
// ---------------------------------------------------------------------------

const CRC_TABLE = ((): Uint32Array => {
  const table = new Uint32Array(256);
  for (let n = 0; n < 256; n++) {
    let c = n;
    for (let k = 0; k < 8; k++) c = c & 1 ? 0xedb88320 ^ (c >>> 1) : c >>> 1;
    table[n] = c >>> 0;
  }
  return table;
})();

function crc32(buf: Buffer): number {
  let c = 0xffffffff;
  for (const b of buf) c = CRC_TABLE[(c ^ b) & 0xff] ^ (c >>> 8);
  return (c ^ 0xffffffff) >>> 0;
}

function pngChunk(type: string, data: Buffer): Buffer {
  const len = Buffer.alloc(4);
  len.writeUInt32BE(data.length, 0);
  const typeBuf = Buffer.from(type, "ascii");
  const crc = Buffer.alloc(4);
  crc.writeUInt32BE(crc32(Buffer.concat([typeBuf, data])), 0);
  return Buffer.concat([len, typeBuf, data, crc]);
}

/** Encode buffer RGBA (width*height*4) jadi PNG. */
export function encodePngRgba(
  width: number,
  height: number,
  rgba: Buffer,
): Buffer {
  if (rgba.length !== width * height * 4) {
    throw new Error("Buffer RGBA tidak cocok dengan width x height");
  }
  const ihdr = Buffer.alloc(13);
  ihdr.writeUInt32BE(width, 0);
  ihdr.writeUInt32BE(height, 4);
  ihdr[8] = 8; // bit depth
  ihdr[9] = 6; // color type RGBA
  ihdr[10] = 0; // compression
  ihdr[11] = 0; // filter
  ihdr[12] = 0; // interlace
  const stride = width * 4;
  const raw = Buffer.alloc(height * (stride + 1));
  for (let y = 0; y < height; y++) {
    raw[y * (stride + 1)] = 0; // filter: none
    rgba.copy(raw, y * (stride + 1) + 1, y * stride, (y + 1) * stride);
  }
  return Buffer.concat([
    Buffer.from([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]),
    pngChunk("IHDR", ihdr),
    pngChunk("IDAT", deflateSync(raw, { level: 6 })),
    pngChunk("IEND", Buffer.alloc(0)),
  ]);
}

// ---------------------------------------------------------------------------
// Komposit
// ---------------------------------------------------------------------------

/**
 * Tempel QR dinamis (qrString) ke tengah template standee.
 * Mengembalikan PNG komposit, atau null kalau template tidak ada / tidak
 * bisa dibaca (pemanggil harus fallback ke QR polos).
 */
export async function composeQrisCard(
  qrString: string,
): Promise<Buffer | null> {
  let template: Buffer;
  try {
    template = fs.readFileSync(standeeTemplatePath());
  } catch {
    return null; // template tidak ada di host — fallback ke QR polos
  }
  let decoded: { width: number; height: number; data: Buffer };
  try {
    decoded = jpeg.decode(template, { maxMemoryUsageInMB: 512 });
  } catch {
    return null;
  }
  const { width, height } = decoded;
  if (width !== STANDEE_WIDTH || height !== STANDEE_HEIGHT) {
    // Template diganti ukurannya — jangan menimpa di koordinat yang salah.
    return null;
  }
  const rgba = Buffer.from(decoded.data);
  const box = QRIS_CARD_QR_BOX;
  const boxW = box.x1 - box.x0 + 1;
  const boxH = box.y1 - box.y0 + 1;

  // Matriks modul QR (tanpa render PNG dulu — langsung dari `qrcode`).
  const qr = QRCode.create(qrString, { errorCorrectionLevel: "M" });
  const size = qr.modules.size;
  const scale = Math.max(1, Math.floor(Math.min(boxW, boxH) / size));
  const drawn = scale * size;
  const offX = box.x0 + Math.floor((boxW - drawn) / 2);
  const offY = box.y0 + Math.floor((boxH - drawn) / 2);

  const setPx = (x: number, y: number, r: number, g: number, b: number): void => {
    const i = (y * width + x) * 4;
    rgba[i] = r;
    rgba[i + 1] = g;
    rgba[i + 2] = b;
    rgba[i + 3] = 255;
  };
  // 1) Hapus QR lama: isi putih area bbox (+6px agar tepi modul lama hilang).
  const pad = 6;
  const fx0 = Math.max(0, box.x0 - pad);
  const fy0 = Math.max(0, box.y0 - pad);
  const fx1 = Math.min(width - 1, box.x1 + pad);
  const fy1 = Math.min(height - 1, box.y1 + pad);
  for (let y = fy0; y <= fy1; y++) {
    for (let x = fx0; x <= fx1; x++) setPx(x, y, 255, 255, 255);
  }
  // 2) Gambar modul hitam QR baru (skala bulat -> tepi tajam).
  for (let my = 0; my < size; my++) {
    for (let mx = 0; mx < size; mx++) {
      if (!qr.modules.data[my * size + mx]) continue;
      const px0 = offX + mx * scale;
      const py0 = offY + my * scale;
      for (let y = py0; y < py0 + scale; y++) {
        for (let x = px0; x < px0 + scale; x++) setPx(x, y, 0, 0, 0);
      }
    }
  }
  return encodePngRgba(width, height, rgba);
}

/**
 * Gambar QR untuk pembeli: kartu komposit standee kalau template ada,
 * QR polos kalau tidak. TIDAK pernah throw karena template hilang —
 * hanya throw kalau qrString-nya sendiri tidak valid.
 */
export async function renderQrisCardOrPlain(
  qrString: string,
): Promise<{ png: Buffer; composited: boolean }> {
  const card = await composeQrisCard(qrString);
  if (card) return { png: card, composited: true };
  return { png: await renderQrisPng(qrString), composited: false };
}
