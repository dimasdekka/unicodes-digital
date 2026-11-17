# Unicodes Digital — Reseller AI & Digital

Web store + bot Telegram untuk jualan produk digital dari dua supplier:

| Supplier | Buat apa | Auth |
| --- | --- | --- |
| **Sondev** | Muse | API key + HMAC-SHA256 |
| **LahaStore** | AI lain (Gemini dkk) | Header `X-API-Key` |

Pembeli bayar ke kamu → backend beli grosir ke supplier → supplier kirim
barang → kamu ambil margin (harga jual − harga grosir).

## Alur order

```
Pembeli (web / Telegram)
  -> checkout & bayar (Midtrans — TODO)
  -> backend verifikasi bayar -> status "paid"
  -> pilih supplier dari katalog (per produk)
  -> cek saldo supplier -> buat order (idempotency key = ID order internal)
  -> poll status tiap 8 detik (maks ~5 menit) / webhook Sondev
  -> success: kirim hasil ke pembeli | failed: refund / coba supplier lain
```

Alur uang: pembeli → kamu (QRIS/e-wallet/bank via Midtrans) → deposit
kamu ke dompet supplier (LahaStore VND/USD, Sondev IDR/USDT) → supplier
potong grosir per order. Saldo kedua dompet harus dijaga — cek sebelum
ambil order, jangan ambil uang pembeli kalau saldo supplier kurang.

## Menjalankan

```bash
npm install
cp .env.example .env.local   # lalu isi key/secret
npm run dev                  # web: http://localhost:3000
npm run bot                  # bot Telegram (long polling, proses terpisah)
```

Isi `.env.local`:

- `LAHASTORE_API_KEY` — sudah aktif (tier Standard).
- `SONDEV_API_KEY` + `SONDEV_API_SECRET` — dari bot Telegram Sondev
  (`/reseller` → "Reveal Secret"). Secret cuma tampil SEKALI, simpan langsung.
- `TELEGRAM_BOT_TOKEN` — dari @BotFather (`/newbot`).
- `TELEGRAM_ADMIN_IDS` — ID Telegram admin (cek via @userinfobot).
- `MIDTRANS_*` — TODO setelah daftar Midtrans.

**Keamanan:** semua key/secret hanya dibaca di server (API route / `src/lib`).
Jangan pernah panggil API supplier dari kode client/browser, dan jangan
commit `.env.local`.

## Struktur

```
src/
  app/                  # Next.js App Router (storefront + API)
    page.tsx            # etalase web
    api/products/       # GET katalog gabungan
    api/orders/         # POST buat order, GET status, POST fulfill (testing)
    api/webhooks/sondev # webhook Sondev (verifikasi HMAC)
  bot/
    bot.ts              # bot Telegram (grammY): /start /products /order /fulfill
    run.ts              # entry point: npm run bot
  lib/
    catalog.ts          # SATU katalog gabungan (produk -> supplier)
    orders.ts           # SATU alur order untuk web & bot
    pricing.ts          # helper margin & konversi kurs
    suppliers/
      types.ts          # interface SupplierAdapter (kontrak semua supplier)
      sondev.ts         # adapter Sondev (HMAC-SHA256)
      lahastore.ts      # adapter LahaStore (X-API-Key) + Xiaomi/SheerID
      index.ts          # registry supplier
```

## Tambah store ke-3 (atau ke-4, dst)

Arsitektur ini memang dibuat supaya tambah supplier gampang — cukup 4 langkah,
tidak ada kode lain yang berubah:

1. Buat `src/lib/suppliers/namastore.ts` — class yang implement
   `SupplierAdapter` dari `src/lib/suppliers/types.ts`
   (contoh: `sondev.ts`, `lahastore.ts`).
2. Tambah id-nya ke union `SupplierId` di `types.ts`.
3. Daftarkan di `SUPPLIERS` di `src/lib/suppliers/index.ts`.
4. Tambah produknya di `src/lib/catalog.ts` (`supplier: "namastore"`).

## Yang masih TODO sebelum dibuka ke publik

- [x] Payment gateway Midtrans (Snap + webhook → `markPaid` →
      `fulfillOrder` otomatis). Daftarkan notification URL di dashboard
      Midtrans. Testing pakai sandbox dulu (`MIDTRANS_IS_PRODUCTION=false`).
- [x] Database SQLite lokal via `node:sqlite` (`src/lib/db.ts`, file
      `dev.db`) — order tersimpan permanen, tidak hilang saat restart.
      Produksi: ganti driver di `db.ts` ke Postgres.
- [x] Halaman admin (`/admin`): saldo kedua supplier, daftar order, harga
      retail. TODO: lindungi dengan login admin sebelum dibuka ke publik.
- [x] Sondev API v1.2 diterapkan penuh (lihat bagian di atas): field benar,
      target wajib, retry + idempotency, webhook dedupe, polling ladder,
      formula harga, pre-flight, re-konfirmasi Midtrans.
- [ ] Deploy ke hosting publik (Vercel/Railway/VPS) — dibutuhkan untuk:
      webhook URL Sondev + Midtrans (keduanya menolak localhost), dan
      untuk verifikasi live Sondev (VM ini kena block Cloudflare 403).
- [ ] Setelah deploy: set webhook URL di panel Sondev (`/reseller` →
      Webhook URL) + di dashboard Midtrans, lalu `POST /webhook/test` →
      `delivered: true`.
- [ ] Test 1 order kecil end-to-end (muse_token, target diisi) setelah
      deploy — verifikasi saldo, katalog, dan pengiriman kode.
- [ ] Daftar Midtrans (isi key di .env.local, daftarkan notification URL webhook),
      token bot Telegram dari @BotFather, deposit LahaStore (saldo 0).
- [ ] Lindungi /admin dengan login.
- [ ] Worker/queue untuk polling (jangan polling di dalam request HTTP).
- [ ] Panel admin ubah harga retail (saat ini harga diatur di `catalog.ts`).
- [ ] Rekonsiliasi harian (docs): `GET /orders?limit=100` vs order lokal,
      `GET /logs` (cek `errors_24h`), dead-letters + replay, bandingkan
      `balance_idr` dengan perhitungan sendiri.
- [ ] Rate limit: Sondev 120 req/menit, 1.000 order/hari. LahaStore tidak
      disebut di docs — polling sewajarnya (5–10 detik).

## Sondev API v1.2 — sudah diterapkan penuh

Dokumentasi lengkap (`SONDEV_RESELLER_API_EN.md`) + template resmi
(`sondev_reseller_bot.py`) sudah dipelajari dan diterapkan ke kode ini
(2026-11-17). Yang paling penting:

- **Field yang benar:** saldo = `balance_idr` / `balance_usdt` (flat) di
  `/profile`; produk = `code`, `price_idr`/`price_usd` (grosir),
  `available_stock` (`"UNLIMITED"` atau angka), `min_qty`/`max_qty`.
- **Target wajib** untuk produk on-demand (Muse): kode invite / email target.
  Tanpa target, Sondev menolak dengan `400 INVALID_ORDER`. Checkout web &
  bot Telegram sudah meminta target sebelum bayar.
- **Signature:** yang ditandatangani hanya PATH (tanpa query string);
  timestamp + signature baru tiap percobaan (pakai ulang = `409
  REPLAY_DETECTED`); retry hanya untuk 429 `RATE_LIMIT_MINUTE`, 409
  `REPLAY_DETECTED`, dan 5xx — `DAILY_LIMIT`, 402, 400, 401, 403 tidak
  di-retry. `request_id` disimpan di pesan error untuk tracing.
- **Status order:** `data.status` = `DELIVERED` / `PROCESSING` / `FAILED` /
  `ALREADY_EXISTS` (ref_id dipakai ulang → baca hasil dari
  `delivery_status`); `delivery_status` final = `DELIVERED` / `FAILED` /
  `REFUNDED` / `PARTIAL_DELIVERED`. Polling Sondev: 5→10→20→30 detik,
  berhenti 10 menit.
- **Webhook:** header `X-Sondev-Timestamp` + `X-Sondev-Signature`
  (boleh berawalan `sha256=`), toleransi ±300 detik, jawab 200 cepat
  (< 12 detik, ideal < 2 detik). Dedupe via `X-Sondev-Delivery`.
  Event: `order.completed`, `order.failed`, `order.updated`,
  `webhook.test`. Kalau order gagal, saldo grosir dikembalikan otomatis
  oleh Sondev — refund ke PEMBELI jadi tanggung jawab kita (manual via
  Midtrans).
- **Harga jual (formula docs):** grosir × 1,25 + Rp1.000, dibulatkan ke
  atas ke 500 terdekat. Contoh: grosir 8.500 → Rp12.000.
- **Pre-flight:** storefront cek `/status/public` (`accepting_orders`)
  + saldo sebelum menampilkan tombol beli.
- **Pembayaran pembeli (aturan docs):** status Midtrans dikonfirmasi ulang
  via API sebelum `markPaid` (webhook = kapan bertanya, API = apakah
  benar lunas); `markPaid` hanya dari status `awaiting_payment`
  (atomic guard); webhook dibalas cepat, fulfill jalan di background.
- **Batas:** 120 req/menit, 1.000 order/hari. Aktivasi reseller: saldo
  ≥ Rp50.000 (menurut docs; panel bot menyebut Rp100.000 — Dim sudah
  top up Rp100.000, jadi aman di keduanya).

⚠️ **Catatan penting:** panggilan API langsung dari VM ini masih kena
block Cloudflare (HTTP 403) — kemungkinan IP egress VM yang diblokir,
bukan salah key/signature (docs menyebut `/api/*` sudah dikecualikan dari
Browser Integrity Check, tapi blok tetap terjadi dari sini). Key sudah
terpasang di `.env.local` dan kode mengikuti docs persis — verifikasi
live (saldo, katalog, order pertama) harus dilakukan setelah deploy ke
Vercel/Railway/VPS, atau dari jaringan lain.

## Catatan per supplier

- **Sondev:** (lihat bagian v1.2 di atas). `ref_id` (1–100 karakter) unik
  per reseller; dipakai ulang → order asli dikembalikan (`ALREADY_EXISTS`,
  HTTP 200, tanpa charge ulang). Saldo USDT dipakai dulu baru IDR; kurang
  saldo → 402. IP allowlist: saat ini "all IPs allowed" — isi dengan IP
  server setelah deploy (direkomendasikan docs).
- **LahaStore:** produk shop lewat `POST /orders` (`shopOrderId` untuk
  idempotency). Gemini Xiaomi (`POST /xiaomi3/orders`, batch 1–200 akun,
  1 akun = 1 order supplier) dan Gemini Student Pro (`POST /sheerid/verify`)
  jalurnya beda — lihat `lahastore.ts` dan `fulfillment` di `catalog.ts`.
  Order gagal (Xiaomi) di-refund otomatis per order. Poll tiap 5–10 detik;
  `submission_unknown` jangan dibuatkan order baru.
