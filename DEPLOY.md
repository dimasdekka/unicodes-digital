# Deploy Unicodes Digital (Railway / VPS / Oracle)

Satu service menjalankan **web + bot Telegram + database SQLite**
(berbagi satu volume, jadi `dev.db` dipakai bareng). Ini alasan tidak pakai
Vercel: di Vercel filesystem serverless tidak awet (order bisa hilang)
dan bot butuh proses yang jalan terus.

Prinsipnya hosting-agnostik — yang dibutuhkan cuma:

1. **Satu service / satu mesin** yang menjalankan web + bot sekaligus
   (bot jalan di background, web di foreground — lihat
   `scripts/railway-start.sh`).
2. **Satu volume / disk persisten** untuk `dev.db` (database SQLite +
   tabel `settings` berisi QRIS statis + sesi GoBiz — hilang kalau
   volumenya hilang).
3. **URL publik** untuk web store / Mini App dan webhook Sondev.

> **Catatan pembayaran:** QRIS tidak butuh domain publik (polling feed,
> bukan webhook) — domain hanya perlu untuk web store / Mini App dan
> webhook Sondev. Midtrans sudah dihapus total 2026-11-17 — tidak ada
> lagi setup Midtrans di deploy ini.

## Yang perlu disiapkan Dim dulu

1. **Hosting** — salah satu:
   - **Railway** — daftar/login di https://railway.app (pakai GitHub).
     Trial $5 sekali pakai 30 hari sudah habis; setelah trial jadi Free
     plan $1 kredit/bulan (tidak cukup untuk bot jalan terus) — Hobby
     $5/bulan (termasuk $5 usage).
   - **VPS murah** (~$5–10/bulan) atau **Oracle Cloud Always Free**
     (VM Ampere, gratis selamanya, tapi daftar + setup lebih ribet dan
     butuh kartu untuk verifikasi akun).
2. **Repo GitHub** — `dimasdekka/unicodes-digital`, branch `main`
   (sudah ada, sudah ter-push).
3. **Key/secret** (nama variabel saja — nilainya JANGAN ditulis di sini):
   - `SONDEV_API_KEY`, `SONDEV_API_SECRET` — sudah ada di `.env.local`
   - `LAHASTORE_API_KEY` — key di `.env.local` sekarang DITOLAK server
     (INVALID_API_KEY); kirim ulang key yang benar sebelum isi di hosting
   - `TELEGRAM_BOT_TOKEN` — sudah ada di `.env.local`
   - `TELEGRAM_ADMIN_IDS` — ID angka Telegram kamu (dari @userinfobot)
   - `ADMIN_PASSWORD` — password login halaman `/admin` (sudah dibuat, ada di `.env.local`)
   - `GOBIZ_PHONE` — nomor HP akun merchant GoBiz (default saat minta OTP)
   - `NEXT_PUBLIC_APP_URL` — isi SETELAH deploy pertama, pakai URL publik
     hosting, lalu redeploy

> **Penting sebelum hosting baru hidup:** bot Telegram + web saat ini
> jalan sementara dari VM ini. Setelah service hosting hidup, matikan
> bot/web di VM supaya tidak ada dua poller yang rebutan update dan dua
> poller feed GoBiz yang rebutan memproses pembayaran yang sama.
> (Matikan proses bot di VM sebelum service baru start.)

## Daftar env var (tanpa Midtrans)

Isi semua ini di hosting (tab Variables / file `.env.local` di VPS).
Tidak ada lagi env `MIDTRANS_*` — jangan isi, kodenya sudah tidak ada.

| Variabel | Wajib | Keterangan |
| --- | --- | --- |
| `TELEGRAM_BOT_TOKEN` | Ya | Token bot dari @BotFather |
| `TELEGRAM_ADMIN_IDS` | Ya | ID angka Telegram admin, pisah koma kalau >1 |
| `ADMIN_PASSWORD` | Ya | Password login `/admin` |
| `SONDEV_API_KEY` | Ya | Key Sondev |
| `SONDEV_API_SECRET` | Ya | Secret Sondev (tampil sekali di bot Sondev) |
| `SONDEV_BASE_URL` | Tidak | Default `https://api.sondevs.tech/api/v1` |
| `LAHASTORE_API_KEY` | Ya | Key LahaStore (butuh kirim ulang — yang sekarang 401) |
| `LAHASTORE_BASE_URL` | Tidak | Default `https://lahastore.up.railway.app/api/v1` |
| `DATABASE_URL` | Tidak | Default `file:./dev.db` — arahkan ke file di volume |
| `NEXT_PUBLIC_APP_URL` | Ya* | URL publik hosting; *diisi setelah domain didapat, lalu redeploy |
| `BOT_BANNER_FILE_ID` | Tidak | `file_id` Telegram banner /start (sudah ada di `.env.local`) |
| `GOBIZ_PHONE` | Ya | Nomor HP akun merchant GoBiz (default minta OTP) |
| `GOPAY_POLL_INTERVAL_MS` | Tidak | Interval poller feed GoBiz, ms, default `30000`, minimal `5000` |
| `PORT` | — | Diisi otomatis oleh Railway; di VPS isi sendiri (default `3000`) |

Semua env dibaca terpusat di `src/lib/config.ts`. QRIS statis merchant
**bukan env** — disimpan di database lewat panel `/admin` (lihat bawah).

## Langkah deploy — Railway

1. Railway → **New Project** → **Deploy from GitHub repo** → pilih
   `dimasdekka/unicodes-digital`.
   Railway otomatis build (`npm install && npm run build`) lalu start
   (`sh scripts/railway-start.sh`: bot di background + web di foreground
   di `$PORT`).
2. **Volume (WAJIB):** di service → Settings → Volumes → Add Volume,
   mount path `/app` (tempat `dev.db` tinggal). Tanpa volume, order +
   QRIS statis + sesi GoBiz hilang tiap kali service restart/redeploy.
3. **Variables:** isi semua env di tabel atas. `PORT` diisi otomatis
   oleh Railway.
4. **Domain:** Settings → Networking → Generate Domain. Catat URL-nya,
   isi ke `NEXT_PUBLIC_APP_URL`, lalu redeploy.
5. **Webhook setelah deploy:**
   - Sondev: bot Telegram Sondev → `/reseller` → Webhook URL:
     `https://<domain>/api/webhooks/sondev`, lalu `POST /webhook/test`
     → `delivered: true`.
   - Tidak ada webhook pembayaran — pembayaran QRIS dibaca lewat polling
     feed GoBiz dari proses bot (lihat "Setup sesi GoBiz" di bawah).
6. **Telegram Web App (Mini App) — toko di dalam Telegram:**
   - Halaman toko Mini App ada di route `/tg` (katalog + checkout asli,
     QRIS ditampilkan inline, status dipantau tiap 10 detik).
   - Setelah `NEXT_PUBLIC_APP_URL` terisi domain publik + redeploy, tombol
     inline "🌐 Buka Toko" muncul otomatis di menu utama bot.
   - Menu Button bot (tombol "Menu" kiri bawah chat) jadi "Buka Toko":
     jalankan SEKALI dari repo ini (butuh `.env.local` berisi
     `TELEGRAM_BOT_TOKEN` + `NEXT_PUBLIC_APP_URL` domain publik):
     ```sh
     npx tsx src/bot/set-menu-button.ts
     ```
     Sebelum domain publik ada, perintah di atas sengaja menolak jalan.
7. **Tes end-to-end:** 1 order kecil muse_token (target kode invite diisi)
   — ini juga verifikasi live pertama key Sondev (dari VM ini selalu 403).

## Langkah deploy — VPS / Oracle (alternatif Railway)

Prinsipnya sama — satu mesin, satu disk persisten, dua proses:

1. Clone repo ke mesin, `npm install`, salin `.env.example` jadi
   `.env.local` dan isi semua env di tabel atas.
2. **Disk persisten untuk `dev.db`:** pastikan `DATABASE_URL`
   menunjuk ke file di disk yang tidak hilang saat reboot/redeploy
   (bukan `/tmp`). Di Oracle/VPS biasa, file di home directory sudah
   persisten.
3. Jalankan web + bot sekaligus (sama seperti `scripts/railway-start.sh`):
   ```sh
   npx tsx src/bot/run.ts &   # bot long polling + poller feed GoBiz, background
   npm run start -- -p "${PORT:-3000}"   # web, foreground
   ```
   Untuk produksi, bungkus keduanya dengan `systemd` / `pm2` supaya
   restart otomatis kalau mati.
4. Domain: arahkan domain ke mesin (reverse proxy, mis. Caddy/Nginx,
   ke port web), isi ke `NEXT_PUBLIC_APP_URL`, restart web.
5. Webhook Sondev + Mini App: sama seperti langkah 5–6 Railway di atas
   (ganti `<domain>` dengan domain VPS/Oracle).

## Setup sesi GoBiz (login OTP) — setelah deploy

Poller feed GoBiz tidak bisa jalan tanpa sesi login merchant yang valid.
Lakukan sekali setelah deploy (dan ulang tiap sesi kedaluwarsa total /
logout):

1. Buka `https://<domain>/admin`, login dengan `ADMIN_PASSWORD`.
2. Buka seksi **"Pembayaran — QRIS GoPay"** → **"Sesi GoBiz (login
   merchant)"**.
3. Nomor HP terisi otomatis dari `GOBIZ_PHONE` (bisa diketik manual kalau
   kosong) → klik **Minta OTP**. OTP dikirim ke HP merchant (SMS/WhatsApp
   GoBiz), berlaku 10 menit.
4. Ketik kode OTP di kolom yang sama → klik **Verifikasi OTP**.
   Status berubah jadi "login" + nomor HP + token berlaku s.d. (waktu
   kedaluwarsa). Token **tidak pernah tampil** di panel.
5. Setelah login, profil merchant dimuat otomatis — periksa baris
   "QRIS statis outlet": harus **cocok dengan panel**. Kalau BERBEDA,
   perbarui string QRIS statis (lihat bawah) supaya QR yang dibuat bot
   cocok dengan outlet yang dicek feed-nya.
6. Sesi di-refresh otomatis (proaktif 5 menit sebelum kedaluwarsa) selama
   refresh token masih valid. Kalau refresh ditolak, status jadi "belum
   login" — ulangi langkah 3–4.

> ⚠️ **Endpoint GoBiz belum terverifikasi live** (dari referensi
> paygateme yang provider GoPay-nya dinyatakan "in development —
> untested"). Kalau "Minta OTP" / "Verifikasi OTP" ditolak GoBiz saat
> live-check pertama ini, itu artinya mapping endpoint di
> `src/lib/payments/gopay.ts` perlu diperbaiki — bukan salah nomor HP /
> OTP. Jangan ulangi OTP berkali-kali tanpa jeda.

## Cara pakai panel admin QRIS (`/admin` → "Pembayaran — QRIS GoPay")

Panel ini punya 4 bagian (semua butuh login `/admin`):

1. **Sesi GoBiz (login merchant)** — lihat atas: status sesi, Minta OTP,
   Verifikasi OTP, Keluar (hapus sesi — poller berhenti sampai login
   lagi), dan tombol **"Cek pembayaran masuk sekarang"** (menjalankan
   satu tick poller manual, hasilnya: dicek X pending — lunas Y,
   kedaluwarsa Z).
2. **QRIS statis GoPay** — tempel string QRIS statis merchant (bukan foto
   QR-nya; ambil dari aplikasi GoBiz → menu QRIS / kode QR toko → salin
   string, diawali `00020101021126...`) → **Simpan QRIS GoPay**. String
   tersimpan di database server dan **tidak pernah ditampilkan lagi** di
   halaman ini. Status menampilkan merchant + kota + ID hasil parsing
   string. QRIS ini adalah bahan baku semua QR dinamis — wajib terpasang
   sebelum ada yang bisa bayar (checkout akan gagal dengan pesan "QRIS
   statis belum dipasang" kalau belum ada).
3. **QR tes** — isi nominal (mis. Rp1.500) → **Buat QR Tes** → muncul QR
   dinamis sungguhan (nominal unik + batas 5 menit). Scan dari e-wallet
   untuk melihat nama merchant + nominal sebelum bayar. Tidak perlu
   dibayar — QR kedaluwarsa sendiri dalam 5 menit. Cocok untuk live-check
   pertama: bayar QR tes sungguhan sekali, lalu cek apakah poller
   menandainya lunas.
4. **Pembayaran QRIS terbaru** — tabel pembayaran (referensi, nominal,
   nominal unik, status, waktu). Referensi `UD-...` = order,
   `TOPUP-...` = isi saldo dompet, `ADMIN-TEST-...` = QR tes.

Catatan operasional:

- **QR berlaku 5 menit.** Pembeli yang telat bayar harus buat order baru
  (order lama otomatis dibatalkan saat QR-nya kedaluwarsa).
- **Refund QRIS manual.** Kalau ada order `failed` setelah dibayar via
  QRIS, admin mendapat notifikasi "Refund manual dibutuhkan" — transfer
  balik ke pembeli secara manual, lalu kabari pembeli. Tidak ada tombol
  refund otomatis untuk QRIS (QRIS tidak punya API refund). Order yang
  dibayar dari saldo dompet di-refund otomatis ke dompet.
- **Sesi GoBiz mati = pembayaran tidak lunas.** Kalau ada pembeli
  mengeluh "sudah bayar tapi order tidak jalan", cek dulu status sesi
  GoBiz di panel ini, lalu tekan "Cek pembayaran masuk sekarang".

## Template standee & banner — file yang HARUS ada di host

Dua file gambar di `src/bot/assets/` bersifat **UNTRACKED dan tidak
di-commit** (lihat `.gitignore`) — snapshot push GitHub tidak bisa
membawa file binary, jadi keduanya tidak ada di repo:

| File | Dipakai untuk | Kalau tidak ada |
| --- | --- | --- |
| `src/bot/assets/qris-standee.jpg` | Template kartu QRIS komposit (QR dinamis ditempel di tengah standee "UNICODES DIGITAL", 1252×1252) | Bot/web fallback ke QR polos — pembayaran tetap jalan, tampilan saja beda |
| `src/bot/assets/banner.jpg` | Foto banner `/start` + `/menu` bot | Bot fallback ke teks sambutan tanpa foto |

Cara menambahkan ke host:

- **Railway:** file binary tidak bisa ikut lewat push — tambahkan via
  **GitHub web UI** (buka repo → folder `src/bot/assets/` → Add file →
  Upload files → commit). Deploy berikutnya akan membawa file itu.
  Alternatif: upload langsung ke volume service (tidak disarankan —
  hilang kalau service di-recreate).
- **VPS / Oracle:** salin file dari VM/lokal ke mesin dengan `scp` ke
  path yang sama di repo yang di-clone.

Jangan ganti ukuran `qris-standee.jpg` (harus 1252×1252) — kalau
ukurannya berubah, `qris-card.ts` menolak menimpa (koordinat QR lama
tidak berlaku lagi) dan fallback ke QR polos.

## Catatan

- Harga retail masih CONTOH di `src/lib/catalog.ts` — final setelah katalog
  live Sondev dicek (target jual Muse 1B sedekat mungkin dengan Rp1.000).
- Saldo LahaStore masih USD 0 — sisi Gemini belum bisa jualan sampai deposit
  (top up $60 = diskon tier 10%).
- Saldo Sondev Rp100.000 (akun @Dimas0227) — program reseller unlocked.
- Verifikasi live Sondev dari VM ini selalu 403 (Cloudflare) — verifikasi
  pertama (saldo, katalog, order kecil) dilakukan setelah deploy.

## Yang masih butuh Dim

- [ ] **Login OTP GoBiz live saat siap** — buka panel `/admin` setelah
      deploy, Minta OTP → ketik OTP → Verifikasi (lihat "Setup sesi
      GoBiz" di atas). Tanpa ini, tidak ada pembayaran QRIS yang bisa
      lunas.
- [ ] **Kirim ulang key LahaStore yang benar** — key di `.env.local`
      sekarang 401 `INVALID_API_KEY`; jangan ambil key dari riwayat chat.
- [ ] **Hosting + domain** — pilih Railway (Hobby $5/bulan setelah trial
      habis) / VPS / Oracle, lalu Generate Domain + isi
      `NEXT_PUBLIC_APP_URL` + redeploy.
- [ ] **Test order kecil pertama ke Sondev** (muse_token 1B, target
      diisi) setelah deploy — verifikasi live key Sondev + alur bayar
      QRIS end-to-end.
- [ ] **Matikan bot/web VM sebelum hosting baru hidup** — supaya tidak
      ada dua poller (bot & feed GoBiz) yang rebutan update/pembayaran.
- [ ] **Template standee + banner di host** — `qris-standee.jpg` dan
      `banner.jpg` ke `src/bot/assets/` via GitHub web UI (Railway) atau
      `scp` (VPS/Oracle).
- [ ] **Webhook Sondev** — set `https://<domain>/api/webhooks/sondev`
      di bot Sondev (`/reseller` → Webhook URL) setelah domain ada.
