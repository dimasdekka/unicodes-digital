// Tes kartu QRIS komposit. Jalankan: npx tsx src/lib/payments/qris-card.test.ts
//
// - Memakai template standee ASLI di src/bot/assets/qris-standee.jpg.
// - Payload dinamis dibuat dari vektor sintetis (speks EMVCo, bukan
//   merchant asli) — sama seperti qris.test.ts / qris-admin.test.ts.
// - Bukti utama: gambar komposit di-decode kembali (decoder PNG minimal di
//   bawah, filter 0) lalu jsqr HARUS membaca payload yang SAMA dengan
//   payload dinamis yang dibuat. Kalau modul bergeser / kontras rusak,
//   jsqr gagal dan tes ini merah.
// - TIDAK ada request jaringan.
import { inflateSync } from "node:zlib";
import jsqr from "jsqr";
import { buildDynamicQris, serializeQrisPayload, type QrisTlv } from "./qris";
import {
  composeQrisCard,
  QRIS_CARD_QR_BOX,
  renderQrisCardOrPlain,
  STANDEE_WIDTH,
  STANDEE_HEIGHT,
} from "./qris-card";

// --- Vektor sintetis (speks EMVCo, BUKAN merchant asli) ---
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
const DYNAMIC_PAYLOAD = buildDynamicQris(STATIC_PAYLOAD, 1501);

// --- Decoder PNG minimal (untuk verifikasi). Menangani filter 0..4
// (None/Sub/Up/Average/Paeth) karena PNG polos dari paket `qrcode`
// memakai filter 1 di sebagian baris; encoder kita sendiri hanya
// menulis filter 0. ---
function decodePng(png: Buffer): {
  width: number;
  height: number;
  data: Uint8ClampedArray;
} {
  if (
    png[0] !== 0x89 ||
    png[1] !== 0x50 ||
    png[2] !== 0x4e ||
    png[3] !== 0x47
  ) {
    throw new Error("Bukan PNG");
  }
  let offset = 8;
  let width = 0;
  let height = 0;
  const idat: Buffer[] = [];
  while (offset < png.length) {
    const len = png.readUInt32BE(offset);
    const type = png.toString("ascii", offset + 4, offset + 8);
    const data = png.subarray(offset + 8, offset + 8 + len);
    if (type === "IHDR") {
      width = data.readUInt32BE(0);
      height = data.readUInt32BE(4);
      if (data[8] !== 8 || data[9] !== 6) {
        throw new Error(`IHDR tak terduga: depth ${data[8]} type ${data[9]}`);
      }
    } else if (type === "IDAT") {
      idat.push(Buffer.from(data));
    } else if (type === "IEND") {
      break;
    }
    offset += 12 + len;
  }
  const raw = inflateSync(Buffer.concat(idat));
  const stride = width * 4;
  const bpp = 4;
  const out = Buffer.alloc(width * height * 4);
  let prev = Buffer.alloc(stride);
  for (let y = 0; y < height; y++) {
    const filter = raw[y * (stride + 1)];
    const row = raw.subarray(y * (stride + 1) + 1, (y + 1) * (stride + 1));
    const cur = Buffer.alloc(stride);
    for (let i = 0; i < stride; i++) {
      const a = i >= bpp ? cur[i - bpp] : 0;
      const b = prev[i];
      const c = i >= bpp ? prev[i - bpp] : 0;
      const v = row[i];
      let recon: number;
      switch (filter) {
        case 0:
          recon = v;
          break;
        case 1:
          recon = v + a;
          break;
        case 2:
          recon = v + b;
          break;
        case 3:
          recon = v + Math.floor((a + b) / 2);
          break;
        case 4: {
          const p = a + b - c;
          const pa = Math.abs(p - a);
          const pb = Math.abs(p - b);
          const pc = Math.abs(p - c);
          const pr = pa <= pb && pa <= pc ? a : pb <= pc ? b : c;
          recon = v + pr;
          break;
        }
        default:
          throw new Error(`Filter PNG tak dikenal: ${filter}`);
      }
      cur[i] = recon & 0xff;
    }
    cur.copy(out, y * stride);
    prev = cur;
  }
  return { width, height, data: new Uint8ClampedArray(out) };
}

function decodeQr(png: Buffer): string | null {
  const { width, height, data } = decodePng(png);
  const res = jsqr(data, width, height);
  return res?.data ?? null;
}

// --- Runner kecil (gaya tes lain di repo) ---
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

async function main(): Promise<void> {
  // ===== 1. Komposit: decode kembali == payload dinamis =====
  const card = await composeQrisCard(DYNAMIC_PAYLOAD);
  check("komposit: template terbaca, PNG dihasilkan", card !== null);
  if (card) {
    console.log(`      (ukuran PNG komposit: ${(card.length / 1024).toFixed(0)} KB)`);
    const decoded = decodeQr(card);
    check(
      "komposit: jsqr membaca payload dinamis yang SAMA",
      decoded === DYNAMIC_PAYLOAD,
      decoded === null
        ? "jsqr tidak menemukan QR"
        : `decode: ${decoded.slice(0, 60)}...`,
    );
    const { width, height } = decodePng(card);
    check(
      "komposit: ukuran gambar tetap 1252x1252",
      width === STANDEE_WIDTH && height === STANDEE_HEIGHT,
      `${width}x${height}`,
    );
    // Quiet zone: cincin di sekeliling bbox QR harus putih (tidak ada
    // sisa modul lama / modul baru yang bocor keluar).
    const { data } = decodePng(card);
    const box = QRIS_CARD_QR_BOX;
    const isWhitePx = (x: number, y: number): boolean => {
      const i = (y * width + x) * 4;
      return data[i] > 240 && data[i + 1] > 240 && data[i + 2] > 240;
    };
    let ringWhite = true;
    for (let x = box.x0 - 10; x <= box.x1 + 10; x++) {
      if (!isWhitePx(x, box.y0 - 10) || !isWhitePx(x, box.y1 + 10)) {
        ringWhite = false;
        break;
      }
    }
    for (let y = box.y0 - 10; y <= box.y1 + 10; y++) {
      if (!isWhitePx(box.x0 - 10, y) || !isWhitePx(box.x1 + 10, y)) {
        ringWhite = false;
        break;
      }
    }
    check("komposit: quiet zone 10px di sekeliling QR tetap putih", ringWhite);
  }

  // ===== 2. Fallback: template tidak ada -> QR polos, tetap terbaca =====
  process.env.QRIS_STANDEE_PATH = "/tmp/tidak-ada-standee.jpg";
  const fallback = await renderQrisCardOrPlain(DYNAMIC_PAYLOAD);
  check("fallback: tanpa template -> composited false", fallback.composited === false);
  check(
    "fallback: QR polos tetap terbaca jsqr",
    decodeQr(fallback.png) === DYNAMIC_PAYLOAD,
    "decode polos gagal",
  );
  const missing = await composeQrisCard(DYNAMIC_PAYLOAD);
  check("fallback: composeQrisCard null saat template hilang", missing === null);
  delete process.env.QRIS_STANDEE_PATH;

  // ===== 3. Template rusak -> null (bukan crash) =====
  process.env.QRIS_STANDEE_PATH = "/tmp/qrdecode/package.json";
  const broken = await composeQrisCard(DYNAMIC_PAYLOAD);
  check("template rusak: composeQrisCard null", broken === null);
  delete process.env.QRIS_STANDEE_PATH;

  // ===== Ringkasan =====
  console.log(`\n==== ${passed}/${passed + failed} lolos ====`);
  if (failed > 0) {
    console.log("Gagal:", failures.join(", "));
    process.exitCode = 1;
  }
}

void main();
