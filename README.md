# Unicodes Digital — Rust rewrite

Port penuh dari implementasi TypeScript (`~/workspace/reseller-web`,
commit `a5ed97a`, GitHub `dimasdekka/unicodes-digital` @ `71471272`)
ke Rust. Satu binary menjalankan bot Telegram + web (storefront, Mini App,
admin) dalam satu proses, sama seperti `scripts/vps-start.sh` di versi TS.

## Kontrak port (JANGAN dilanggar)

- **Skema SQLite IDENTIK** dengan versi TS (`dev.db` harus bisa dipakai
  langsung tanpa migrasi): tabel `users`, `wallets`, `store_orders`,
  `wallet_topups`, `midtrans_payments`, `qris_payments`, `promo_codes`,
  `referrals`, `support_tickets`, `support_messages`, `channel_posts`,
  `product_price_overrides`, `settings`. WAL mode + `busy_timeout = 5000`.
- **Nama env var SAMA** seperti `.env.local` versi TS
  (`TELEGRAM_BOT_TOKEN`, `MIDTRANS_SERVER_KEY`, `SONDEV_API_KEY`, dst).
- **Perilaku bot SAMA**: alur checkout Sondev (kode invite dulu → qty →
  ringkasan bayar), teks/format pesan, harga, batas (kode maks 30x,
  qty maks 30), i18n ID/EN.
- **Jalur uang SAMA persis**: transisi status order atomik
  (`UPDATE ... WHERE status=...`), refund dompet otomatis, Midtrans
  404 = pending, expiry sweep 30 menit.
- **Tidak ada secret di repo.** `.env.local` tidak pernah di-commit.

## Stack

- Bot: **teloxide** (long polling; runner paralel per-chat seperti
  `@grammyjs/runner` + `sequentialize` di versi TS).
- Web: **axum** + **maud** (server-rendered HTML; Mini App `/tg`
  tetap validasi `initData` HMAC-SHA256).
- DB: **rusqlite** (bundled SQLite). HTTP: **reqwest**.
- Crypto: **hmac + sha2** (HMAC-SHA256 Sondev, SHA512 webhook Midtrans).

## Jalankan

```sh
cp .env.local.example .env.local   # isi secret sendiri
cargo run --release                # bot + web (PORT, default 3100)
cargo run -- set-menu-button      # sekali jalan: set Menu Button bot (butuh NEXT_PUBLIC_APP_URL publik)
cargo test                         # tes (cerminan harness TS 197/197)
```

## Struktur

- `src/config.rs` — env
- `src/db.rs` — koneksi + skema
- `src/catalog.rs`, `src/i18n.rs` — katalog + teks bot
- `src/lib/` — orders, wallet, pricing, session, notify, tiket, harga
- `src/lib/suppliers/` — sondev, lahastore
- `src/lib/payments/` — midtrans, poller, qris
- `src/bot/` — bot toko + bot news
- `src/web/` — storefront, /tg, /admin, /api, webhook
