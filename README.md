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
  -> checkout -> bayar QRIS dinamis (GoPay merchant, lihat "Pembayaran" di bawah)
  -> poller feed GoBiz cocokkan pembayaran -> status "paid"
  -> pilih supplier dari katalog (per produk)
  -> cek saldo supplier -> buat order (idempotency key = ID order internal)
  -> poll status tiap 8 detik (maks ~5 menit) / webhook Sondev
  -> success: kirim hasil ke pembeli | failed: refund (lihat "Refund" di bawah)
```

Alur uang: pembeli → kamu (QRIS GoPay merchant) → deposit
kamu ke dompet supplier (LahaStore VND/USD, Sondev IDR/USDT) → supplier
potong grosir per order. Saldo kedua dompet harus dijaga — cek sebelum
ambil order, jangan ambil uang pembeli kalau saldo supplier kurang.

## Pembayaran — QRIS dinamis via GoPay merchant (GoBiz)

Tidak ada payment gateway pihak ketiga. Pembeli membayar dengan memindai
satu QRIS dinamis per pembayaran, dan toko membaca pembayaran masuk dari
feed transaksi akun merchant GoBiz (GoPay) sendiri.

> **Riwayat singkat:** Midtrans dihapus 2026-11-17, diganti QRIS dinamis.
> Jangan setup Midtrans lagi — tidak ada lagi kode, webhook, atau env
> Midtrans di repo ini.

### Fase 1 — QRIS dinamis versi sendiri (`src/lib/payments/qris.ts`)

Ditulis dari nol (TypeScript, tanpa library pihak ketiga, tanpa request
jaringan):

- **Parser payload QRIS statis** format EMVCo (TLV: tag 2 digit + panjang
  2 digit + value).
- **Builder QRIS dinamis:** suntik tag 54 (nominal) + tag 53 (`360` = IDR),
  ubah tag 01 jadi `12` (dinamis), hitung ulang CRC16 tag 63
  (CRC-16/CCITT-FALSE).
- **Alokator nominal unik:** nominal dasar + offset 1..999 rupiah, dicek
  pada hasil — supaya tiap pembayaran bisa dibedakan hanya dari nominalnya
  di feed.
- **Store pembayaran** di SQLite (tabel `qris_payments`): referensi
  (`UD-...` untuk order, `TOPUP-...` untuk isi saldo, `ADMIN-TEST-...`
  untuk QR tes), nominal dasar + nominal unik, status, waktu kedaluwarsa.
- **Matcher:** cocokkan transaksi feed dengan pembayaran pending
  (nominal unik + jendela waktu, toleransi selisih jam 60 detik).
- **TTL 5 menit:** QR yang tidak dibayar dalam 5 menit jadi `expired`
  dan order terkait dibatalkan (pembeli bisa buat order baru).

### Fase 2 — client GoBiz (`src/lib/payments/gopay.ts`)

Login OTP ke akun merchant GoBiz + baca profil merchant + feed transaksi.
Ditulis dari pola library Go paygateme (ATM: amati, tiru, modifikasi) —
bukan salinan kodenya.

- **Login OTP:** `requestOtp` (nomor HP default dari env `GOBIZ_PHONE`)
  → OTP diketik pemilik di panel `/admin` (bukan di chat) → `verifyOtp`
  menyimpan sesi. OTP context (`otp_token`) disimpan server-side di tabel
  `settings`; OTP-nya sendiri tidak pernah disimpan.
- **Sesi** (access token + refresh token + expiry) disimpan di tabel
  `settings` (`gopay_session`), satu baris upsert. **Token tidak pernah
  tampil di panel admin dan tidak pernah ditulis ke log.**
- **Refresh proaktif 5 menit** sebelum access token kedaluwarsa; 401 →
  refresh sekali → retry sekali, lalu menyerah dengan `auth_required`
  (login OTP ulang).
- **Profil merchant:** baca `/v1/users/me` + detail merchant, lalu
  cocokkan QRIS statis outlet GoBiz dengan QRIS statis yang tersimpan di
  panel admin — kalau beda, panel menampilkan peringatan (QR yang dibuat
  bot harus sama dengan outlet yang dicek feed-nya).
- **Feed transaksi:** jendela lookback 1 jam, status `settlement` /
  `capture`, payment `qris` / `gopay`, halaman maks 100, maks 10 halaman
  per tick. Nominal feed (satuan minor) dibagi 100 ke rupiah; transaksi
  yang nominalnya tidak bulat / tidak ada id-nya dilewati.

> ⚠️ **Endpoint GoBiz belum terverifikasi live.** Provider GoPay di
> paygateme dinyatakan "in development — untested against the live GoBiz
> API", jadi semua endpoint di `gopay.ts` adalah hipotesis dari referensi
> itu, bukan hasil live-check. Modul ini dibangun defensif (timeout
> 20 detik per request, error dibedakan `auth_required` /
> `auth_failed` / `captcha_required` / `api_error` / `http_error` /
> `config_invalid`) supaya kalau GoBiz menolak endpointnya, mapping-nya
> cukup diperbaiki di satu file ini. Tes `gopay.test.ts` mengunci perilaku
> client terhadap mock, bukan kebenaran endpoint — live-check pertama
> adalah login OTP sungguhan dari panel `/admin` (lihat DEPLOY.md).

### Poller — satu-satunya tempat pembayaran dinyatakan lunas

`src/lib/payments/qris-poller.ts` (`tickQrisPayments`, idempotent, tidak
pernah throw):

- Jalan dari **proses BOT saja** (`src/bot/run.ts`, interval default
  30 detik, tick pertama 10 detik setelah start, `GOPAY_POLL_INTERVAL_MS`
  opsional di env) — bukan dari web, supaya tidak ada dua poller yang
  rebutan memproses transaksi yang sama. Bisa juga dipicu manual dari
  panel admin (`POST /api/admin/payments/gopay/poll`).
- Tiap tick: ambil feed GoBiz → cocokkan dengan pembayaran pending →
  untuk tiap match, jalankan jalur yang sudah ada:
  - top-up (`TOPUP-...`) → `markTopupPaid` (kredit dompet, idempotent),
  - order (`UD-...`) → `setOrderPaidVia("qris")` → `markPaid` →
    `fulfillOrder` (background) + notifikasi ke pembeli (+ reward referral
    kalau ada).
- QR kedaluwarsa disapu di tick yang sama; order yang QR-nya mati
  dibatalkan dengan pesan "buat order baru".
- Kalau sesi GoBiz belum login / kedaluwarsa, tick mengembalikan
  `ok: false, code: "auth_required"` dan dicatat sebagai peringatan —
  bot tidak crash, pembayaran hanya tidak akan lunas sampai login ulang.

### Fase 3 — bayar dari bot & web

- **Bot Telegram** (`src/bot/bot.ts`): checkout order atau isi saldo
  (`/start` → Katalog / Isi Saldo) → pilih "bayar QRIS" → bot membuat
  pembayaran QRIS dinamis dan mengirim **foto kartu QRIS** (lihat di
  bawah) + nominal unik + batas waktu 5 menit. Setelah lunas, poller
  otomatis memprosesnya; bot mengirim hasil ke pembeli.
- **Storefront web** (`src/app/buy-button.tsx` + `POST /api/orders`):
  checkout menampilkan QR inline di halaman, lalu memantau status order
  tiap 10 detik sampai lunas / kedaluwarsa.
- **Mini App / Telegram Web App** (`src/app/tg/page.tsx`, route `/tg`):
  katalog + checkout asli di dalam Telegram (validasi initData HMAC),
  QR ditampilkan inline, status order dipantau tiap 10 detik sampai
  `delivered` / `failed`; setelah lunas, hasil order dikirim bot ke chat
  pembeli. Dibuka dari bot lewat tombol "🌐 Buka Toko" (butuh domain
  publik — lihat DEPLOY.md).

### Kartu QRIS — yang dilihat pembeli

QR yang dilihat pembeli bukan QR polos, melainkan **kartu standee
komposit** (`src/lib/payments/qris-card.ts`):

- QR dinamis ditempel ke tengah template standee
  `src/bot/assets/qris-standee.jpg` (foto standee QRIS "UNICODES
  DIGITAL", 1252×1252 px; QR lama di bbox x 359..885, y 502..1027
  ditimpa QR baru, skala modul bulat, output PNG lossless supaya kontras
  modul hitam-putih terjaga).
- Template ini **UNTRACKED, tidak di-commit** (lihat `.gitignore`) —
  file harus ada di host deploy; cara menambahkannya ada di DEPLOY.md.
- **Fallback:** kalau template tidak ada / ukurannya berubah / tidak bisa
  dibaca, `composeQrisCard()` mengembalikan `null` dan pemanggil memakai
  QR polos (`renderQrisPng`) — bot/web tidak boleh crash gara-gara file
  template.

### Refund

- **Bayar QRIS → tidak ada refund otomatis.** QRIS tidak punya API
  refund. Kalau fulfillment gagal setelah pembeli bayar via QRIS, order
  jadi `failed` + ada catatan refund **MANUAL** + notifikasi ke admin
  (transfer balik ke pembeli secara manual, lalu kabari pembeli).
- **Bayar saldo dompet → refund otomatis ke dompet.** Order yang dibayar
  dari saldo pembeli dan gagal fulfillment di-refund ke dompet pembeli
  (status `refunded`).

### Env & keamanan pembayaran

- Semua env dibaca terpusat di `src/lib/config.ts` — jangan baca
  `process.env` tersebar di modul lain. Getter melempar error yang jelas
  kalau env wajib belum diisi; nilainya tidak pernah di-log.
- **QRIS statis merchant disimpan di database** lewat panel `/admin`
  (tabel `settings`), bukan di env dan bukan di chat. String lengkapnya
  tidak pernah ditampilkan lagi setelah disimpan.
- Nomor HP GoBiz default dari env `GOBIZ_PHONE` (diisi pemilik di
  `.env.local`); OTP diketik pemilik di panel `/admin`.

## Menjalankan

```bash
npm install
cp .env.example .env.local   # lalu isi key/secret
npm run dev                  # web: http://localhost:3000
npm run bot                  # bot Telegram (long polling, proses terpisah)
```

Isi `.env.local` (daftar lengkap ada di `.env.example`):

- `LAHASTORE_API_KEY` — key LahaStore (key di `.env.local` sekarang
  DITOLAK server — `INVALID_API_KEY`; butuh kirim ulang key yang benar).
- `SONDEV_API_KEY` + `SONDEV_API_SECRET` — dari bot Telegram Sondev
  (`/reseller` → "Reveal Secret"). Secret cuma tampil SEKALI, simpan langsung.
- `TELEGRAM_BOT_TOKEN` — dari @BotFather (`/newbot`).
- `TELEGRAM_ADMIN_IDS` — ID Telegram admin (cek via @userinfobot).
- `ADMIN_PASSWORD` — password login halaman `/admin`.
- `GOBIZ_PHONE` — nomor HP akun merchant GoBiz (default saat minta OTP).
- `GOPAY_POLL_INTERVAL_MS` — opsional, interval poller feed (ms, default
  30000, minimal 5000).
- `NEXT_PUBLIC_APP_URL` — URL publik setelah deploy (localhost saat dev).
- `DATABASE_URL` — SQLite lokal (`file:./dev.db`).

**Keamanan:** semua key/secret hanya dibaca di server (API route / `src/lib`).
Jangan pernah panggil API supplier dari kode client/browser, dan jangan
commit `.env.local`. Token GoBiz tidak pernah tampil di panel/log.

## Cara menjalankan tes

Tidak ada script `npm test` — tiap modul tes dijalankan langsung dengan
`tsx` (semuanya tanpa request jaringan, database tes di `/tmp` supaya
`dev.db` live tidak kotor):

```bash
npx tsx src/lib/payments/qris.test.ts        # Fase 1: parser EMV, builder dinamis, CRC16, alokator nominal unik, matcher, expiry
npx tsx src/lib/payments/qris-admin.test.ts   # panel admin QRIS: helper settings, validasi QRIS, auth cookie admin
npx tsx src/lib/payments/gopay.test.ts        # Fase 2: client GoBiz terhadap mock (mengunci perilaku, BUKAN kebenaran endpoint — lihat peringatan di atas)
npx tsx src/lib/payments/qris-card.test.ts    # kartu komposit: decode ulang PNG hasil, jsqr harus membaca payload dinamis yang sama
npx tsx src/bot/harness.ts                   # harness end-to-end bot + web (semua fitur + edge case, DB tes di /tmp)
npx tsc --noEmit                             # typecheck
npm run build                                # build produksi Next.js
```

Catatan tes:

- `qris.test.ts` memakai vektor sintetis dari spek EMVCo (bukan QRIS
  merchant asli) dan memverifikasi CRC16 dengan implementasi independen
  (bitwise) yang berbeda jalur dari implementasi table-driven di `qris.ts`.
- `qris-card.test.ts` memakai template standee asli di
  `src/bot/assets/qris-standee.jpg` — gagal kalau file itu tidak ada di
  mesin yang menjalankan tes.
- `gopay.test.ts` mem-mock semua respons GoID/GoBiz mengikuti bentuk yang
  diringkas dari paygateme; endpoint-nya sendiri belum terverifikasi live.

## Struktur

```
src/
  app/                  # Next.js App Router (storefront + API)
    page.tsx            # etalase web — HALAMAN PEMBELI = KATALOG SAJA (tanpa info supplier/saldo/deposit/maintenance)
    buy-button.tsx      # checkout web: buat order + tampilkan QRIS inline + pantau status tiap 10 detik
    tg/page.tsx         # Mini App Telegram (/tg): katalog + checkout + QR inline + pantau status tiap 10 detik
    api/products/       # GET katalog gabungan
    api/orders/         # POST buat order (+ pembayaran QRIS), GET status
    api/webhooks/sondev # webhook Sondev (verifikasi HMAC)
    admin/              # /admin (login password): saldo supplier, order, panel QRIS
      qris-panel.tsx    # seksi "Pembayaran — QRIS GoPay": sesi GoBiz (OTP), QRIS statis, QR tes, daftar pembayaran
  bot/
    bot.ts              # bot Telegram (grammY): /start /products /order /fulfill + bayar QRIS + isi saldo + referral + promo
    run.ts              # entry point: npm run bot — bot long polling + poller feed GoBiz (interval 30 detik)
  lib/
    catalog.ts          # SATU katalog gabungan (produk -> supplier)
    orders.ts           # SATU alur order untuk web & bot (markPaid, fulfillOrder, refund)
    pricing.ts          # helper margin & konversi kurs
    wallet.ts           # dompet pembeli: isi saldo (top-up QRIS), bayar dari saldo, refund ke dompet, referral
    config.ts           # SATU tempat membaca env (jangan baca process.env tersebar)
    db.ts               # SQLite lokal (dev.db) + tabel settings
    notify.ts           # notifikasi hasil order ke pembeli (bot)
    payments/
      qris.ts           # Fase 1: parser EMV, builder QRIS dinamis, CRC16, alokator nominal unik, matcher, expiry, tabel qris_payments
      gopay.ts          # Fase 2: client GoBiz — login OTP, sesi + refresh, profil merchant, feed transaksi
      qris-poller.ts    # tick() tiap 30 detik (proses bot): feed -> match -> markPaid/fulfillOrder atau top-up; sapu QR kedaluwarsa
      qris-card.ts      # kartu QRIS komposit: QR dinamis di atas template standee (fallback QR polos)
      qris-settings.ts  # simpan/baca QRIS statis merchant di tabel settings
    suppliers/
      types.ts          # interface SupplierAdapter (kontrak semua supplier)
      sondev.ts         # adapter Sondev (HMAC-SHA256)
      lahastore.ts      # adapter LahaStore (X-API-Key) + Xiaomi/SheerID
      index.ts          # registry supplier
```

## Tema & halaman pembeli

- **Tema: ungu Scaramouche (Genshin Impact)** — background indigo gelap
  `#14092b`, kartu `#221244`, aksen violet `#7c3aed`, teks lavender
  `#ede9fe`, harga emas `#e8c874`, garis emas `#d9b45b` di header.
- **Halaman pembeli = katalog saja.** Pembeli tidak perlu tahu soal
  supplier, saldo grosir, deposit, atau maintenance — info internal itu
  hanya ada di `/admin`. Produk yang belum tersedia tampil sebagai
  "Segera hadir" tanpa alasan.

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

- [x] Pembayaran QRIS dinamis via GoPay merchant (Fase 1 qris.ts, Fase 2
      gopay.ts, poller feed → `markPaid` → `fulfillOrder` otomatis).
      **Belum live-check:** login OTP + feed GoBiz sungguhan (endpoint
      dari referensi paygateme, belum terverifikasi) — tes pertama setelah
      deploy: login OTP di panel `/admin` lalu QR tes dibayar sungguhan.
- [x] Database SQLite lokal via `node:sqlite` (`src/lib/db.ts`, file
      `dev.db`) — order tersimpan permanen, tidak hilang saat restart.
      Produksi: ganti driver di `db.ts` ke Postgres.
- [x] Halaman admin (`/admin`): saldo kedua supplier, daftar order, panel
      QRIS — dilindungi login password (cookie httpOnly).
- [x] Sondev API v1.2 diterapkan penuh (lihat bagian di atas): field benar,
      target wajib, retry + idempotency, webhook dedupe, polling ladder,
      formula harga, pre-flight.
- [ ] Deploy ke hosting publik (Railway/VPS/Oracle — lihat DEPLOY.md) —
      dibutuhkan untuk: webhook URL Sondev (menolak localhost), tombol
      "Buka Toko" Mini App, dan verifikasi live Sondev (VM ini kena block
      Cloudflare 403). **Catatan:** pembayaran QRIS tidak butuh domain
      publik (polling feed, bukan webhook) — domain hanya perlu untuk web
      store / Mini App.
- [ ] Setelah deploy: set webhook URL di panel Sondev (`/reseller` →
      Webhook URL, `https://<domain>/api/webhooks/sondev`) lalu
      `POST /webhook/test` → `delivered: true`.
- [ ] Test 1 order kecil end-to-end (muse_token, target diisi) setelah
      deploy — verifikasi saldo, katalog, dan pengiriman kode. **Jangan
      bayar top-up/order sungguhan dulu sebelum sesi GoBiz login** —
      pembayaran tidak akan lunas sampai poller feed jalan dengan sesi
      yang valid.
- [ ] Kirim ulang key LahaStore yang benar (key sekarang 401
      `INVALID_API_KEY`), deposit LahaStore (saldo 0).
- [ ] Panel admin ubah harga retail (saat ini harga diatur di `catalog.ts`).
- [ ] Worker/queue untuk polling (jangan polling di dalam request HTTP).
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
  oleh Sondev — refund ke PEMBELI jadi tanggung jawab kita (manual, karena
  QRIS tidak punya refund otomatis).
- **Harga jual (formula docs):** grosir × 1,25 + Rp1.000, dibulatkan ke
  atas ke 500 terdekat. Contoh: grosir 8.500 → Rp12.000.
- **Pre-flight:** storefront cek `/status/public` (`accepting_orders`)
  + saldo sebelum menampilkan tombol beli.
- **Pembayaran pembeli:** lunas dinyatakan oleh poller feed GoBiz
  (`qris-poller.ts`), bukan oleh webhook gateway; `markPaid` hanya dari
  status `awaiting_payment` (atomic guard); fulfill jalan di background.
- **Batas:** 120 req/menit, 1.000 order/hari. Aktivasi reseller: saldo
  ≥ Rp50.000 (menurut docs; panel bot menyebut Rp100.000 — Dim sudah
  top up Rp100.000, jadi aman di keduanya).

⚠️ **Catatan penting:** panggilan API langsung dari VM ini masih kena
block Cloudflare (HTTP 403) — kemungkinan IP egress VM yang diblokir,
bukan salah key/signature (docs menyebut `/api/*` sudah dikecualikan dari
Browser Integrity Check, tapi blok tetap terjadi dari sini). Key sudah
terpasang di `.env.local` dan kode mengikuti docs persis — verifikasi
live (saldo, katalog, order pertama) harus dilakukan setelah deploy ke
Railway/VPS/Oracle, atau dari jaringan lain.

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
