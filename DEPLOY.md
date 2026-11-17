# Deploy Unicodes Digital ke Railway

Satu service Railway menjalankan **web + bot Telegram + database SQLite**
(berbagi satu volume, jadi `dev.db` dipakai bareng). Ini alasan pilih Railway
bukan Vercel: di Vercel filesystem serverless tidak awet (order bisa hilang)
dan bot butuh proses yang jalan terus.

## Yang perlu disiapkan Dim dulu

1. **Akun Railway** — daftar/login di https://railway.app (pakai GitHub).
2. **Repo GitHub** — buat repo kosong di https://github.com/new
   (mis. `unicodes-digital`), lalu push kode ini ke sana (lihat bawah).
3. **Key/secret** (nama variabel saja — nilainya JANGAN ditulis di sini):
   - `SONDEV_API_KEY`, `SONDEV_API_SECRET` — sudah ada di `.env.local`
   - `LAHASTORE_API_KEY` — key di `.env.local` sekarang DITOLAK server
     (INVALID_API_KEY); kirim ulang key yang benar sebelum isi di Railway
   - `TELEGRAM_BOT_TOKEN` — sudah ada di `.env.local`
   - `TELEGRAM_ADMIN_IDS` — ID angka Telegram kamu (dari @userinfobot)
   - `ADMIN_PASSWORD` — password login halaman `/admin` (sudah dibuat, ada di `.env.local`)
   - `MIDTRANS_SERVER_KEY`, `MIDTRANS_CLIENT_KEY` — key PRODUKSI sudah di-set
     (terverifikasi, `MIDTRANS_IS_PRODUCTION=true`); isi juga di Railway.
   - `NEXT_PUBLIC_APP_URL` — isi SETELAH deploy pertama, pakai URL publik
     Railway, lalu redeploy

> **Penting sebelum deploy Railway:** bot Telegram saat ini jalan sementara
> dari VM ini. Setelah service Railway hidup, matikan bot di VM supaya tidak
> ada dua poller yang rebutan update (`pkill -f "tsx src/bot/run.ts"`).

## Langkah deploy

1. Push repo ini ke GitHub:
   ```sh
   git remote add origin https://github.com/dimasdekka/unicodes-digital.git
   git push -u origin main
   ```
2. Railway → **New Project** → **Deploy from GitHub repo** → pilih repo.
   Railway otomatis build (`npm install && npm run build`) lalu start
   (`sh scripts/railway-start.sh`: web + bot sekaligus).
3. **Volume (WAJIB):** di service → Settings → Volumes → Add Volume,
   mount path `/app` (tempat `dev.db` tinggal). Tanpa volume, order hilang
   tiap kali service restart/redeploy.
4. **Variables:** isi semua env di atas (tab Variables). `PORT` diisi
   otomatis oleh Railway.
5. **Domain:** Settings → Networking → Generate Domain. Catat URL-nya,
   isi ke `NEXT_PUBLIC_APP_URL`, lalu redeploy.
6. **Webhook setelah deploy:**
   - Sondev: bot Telegram Sondev → `/reseller` → Webhook URL:
     `https://<domain>/api/webhooks/sondev`, lalu `POST /webhook/test`
   - Midtrans: dashboard → Settings → Configuration → Payment Notification
     URL: `https://<domain>/api/webhooks/midtrans`
7. **Tes end-to-end:** 1 order kecil muse_token (target kode invite diisi)
   — ini juga verifikasi live pertama key Sondev (dari VM ini selalu 403).

## Catatan

- Harga retail masih CONTOH di `src/lib/catalog.ts` — final setelah katalog
  live Sondev dicek (target jual Muse 1B sedekat mungkin dengan Rp1.000).
- `/admin` belum ada login — lindungi sebelum promosi ke publik.
- Saldo LahaStore masih USD 0 — sisi Gemini belum bisa jualan sampai deposit
  (top up $60 = diskon tier 10%).
