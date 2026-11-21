# Deploy Unicodes Digital di VPS murah / Oracle Always Free ($0)

Bot + web ini ringan banget (diukur 21 Nov 2026 di VM: bot ~65 MB RAM, web
~57 MB RAM, CPU ~0%, database 86 KB). **Total ~125 MB RAM — instance
paling kecil pun cukup.** Yang bikin mahal bukan botnya, tapi sewa server
24 jam. Jadi ambil yang paling kecil / gratis:

## Pilihan (termurah dulu)

1. **Oracle Cloud Always Free — $0/bulan, selamanya.** VM Ampere A1
   (ARM, sampai 4 OCPU + 24 GB RAM) atau 2× VM micro AMD (1 OCPU + 1 GB
   RAM — sudah 8× lebih dari cukup). Syarat: daftar pakai kartu (cuma
   verifikasi, tidak ditarik), dan slot gratis kadang "out of capacity" —
   coba lagi lain waktu kalau penuh. Setup manual lewat SSH.
2. **VPS murah ~$3–5/bulan (±Rp50–80rb).** Contoh: Hetzner CX22 ~€3,79,
   DigitalOcean Basic ~$4–6, Vultr ~$5. Paling gampang, langsung dapat.
3. **Railway Hobby $5/bulan** — sudah disiapkan dari awal (railway.json +
   scripts/railway-start.sh). Paling gampang kalau mau tanpa urus server.

Yang TIDAK cocok: Railway Free ($1/bulan — terlalu kecil buat proses
24 jam), Render free (tidur kalau sepi — bot polling mati), Vercel
serverless (SQLite tidak tersimpan + bot butuh proses jalan terus).

## Setup (Ubuntu 22.04 / 24.04, ~15 menit)

```sh
# 1. Node 24 + git
curl -fsSL https://deb.nodesource.com/setup_24.x | sudo -E bash -
sudo apt-get install -y nodejs git

# 2. Kode (repo privat kamu — pakai HTTPS + token, atau scp dari laptop)
sudo mkdir -p /opt/unicodes-digital
sudo chown $USER:$USER /opt/unicodes-digital
git clone https://github.com/dimasdekka/unicodes-digital.git /opt/unicodes-digital
cd /opt/unicodes-digital

# 3. Install + build
npm ci
npm run build

# 4. Isi .env.local (SALIN dari VM sekarang — JANGAN commit):
#    TELEGRAM_BOT_TOKEN, TELEGRAM_ADMIN_IDS, ADMIN_PASSWORD,
#    SONDEV_KEY, SONDEV_SECRET, GOBIZ_PHONE, BOT_BANNER_FILE_ID
#    + file src/bot/assets/banner.jpg dan src/bot/assets/qris-standee.jpg
#    (dua file ini tidak ada di GitHub — upload manual).

# 5. Service systemd (bot + web jalan bareng, restart otomatis)
sudo cp scripts/unicodes-digital.service /etc/systemd/system/
sudo systemctl daemon-reload
sudo systemctl enable --now unicodes-digital
sudo systemctl status unicodes-digital
sudo journalctl -u unicodes-digital -f
```

## Setelah hidup

- Cek web: `curl -s -o /dev/null -w "%{http_code}" http://localhost:3100/` (harus 200).
- Cek bot: tap /start di @unicodesdigital_bot — harus jawab.
- **Matikan bot + web di VM lama** sebelum yang baru live — dua poller
  berebut update Telegram.
- Webhook Sondev + tombol menu "Buka Toko" baru bisa disetel kalau sudah
  ada domain publik (lihat DEPLOY.md).
- Kalau suatu saat pindah host lagi: cukup pindahkan file `dev.db`
  (semua order, dompet, promo, referral ada di situ).
