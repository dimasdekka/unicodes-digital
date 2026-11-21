# Kelompok D — Bot Telegram (status kerja)

Tanggal: 2026-11-21. Sumber kebenaran: `~/workspace/reseller-web/src/bot/`
(`bot.ts` 1969 baris dibaca seluruhnya, `i18n.ts` 678 baris, `session.ts`,
`run.ts`, `news-bot.ts`, `news-run.ts`, `menu-button.ts`, `set-menu-button.ts`)
+ `harness.ts` (197 checks, dibaca pola alurnya).
Kontrak: `~/workspace/reseller-rust/README.md` (commit TS `a5ed97a`).

## Modul selesai

| File | Isi |
|---|---|
| `src/bot/mod.rs` | `BotState`, `run_store_bot`, `run_news_bot`, poller loop (tick pertama 10 dtk, lalu tiap 30 dtk, guard anti-tumpang-tindih). Distribution function eksplisit `\|upd\| upd.chat().map(\|c\| c.id)` — paralel antar-chat, sekuensial dalam satu chat (terverifikasi di source teloxide 0.17: `dispatcher.rs` "Default distribution function" + `distribution.rs` `default_distribution_function` — sama seperti `@grammyjs/runner` + `sequentialize`). `setMyCommands` sekali saat start (gagal = log saja). |
| `src/bot/handlers.rs` | Seluruh bot toko: /start format baru ala Lhie (banner foto, sapaan WIB, INFORMASI AKUN + STATISTIK TOKO), menu keyboard, katalog (hanya Sondev), detail produk + banner, checkout kode-invite-dulu → qty (paket 1/3/5/10/30 + custom, maks 30) → promo → bayar (saldo / Midtrans Snap QRIS / QRIS GoBiz fallback), dompet + top-up, pesanan, referral, promo, tiket support, bahasa, channel gate, tombol "Saya Sudah Bayar" (throttle 10 dtk/referensi), command admin `/fulfill`, `/promo_add`, `/promo_list`. Sesi checkout in-memory via `crate::lib::session::SessionStore` (restart = draft hilang, order di DB aman). |
| `src/bot/extract.rs` | `extract_invite_code` + 4 tes unit (kode huruf-saja `TZVXJR` valid, link `muse.ai/invite/<kode>`, teks share butuh konteks invite/kode/muse.ai, `"halo dunia"` ditolak). |
| `src/bot/news.rs` | Bot news `@unicodesdigitalbot`: /start + pesan apa pun → info toko/komunitas; token kosong → diam (seperti `news-run.ts`). |
| `src/bot/menu_button.rs` | `get_web_app_url` (None saat localhost) + `set_web_app_menu_button` ("Buka Toko", idempotent) + 2 tes unit. |
| `src/bot/settlement.rs` | `BotSettlement: PaymentSettlement` — implementasi ASLI untuk kedua poller (credit_topup, mark_order_paid atomik, mark_order_failed, cancel_order_expired, order_status, order_buyer_tg_id, maybe_reward_referrer, spawn_fulfill_and_notify). JANGAN pakai `UnimplementedSettlement` di bot (tick panic saat ada pembayaran lunas). |
| `src/bot/deps.rs` | Hanya adapter + helper murni (lihat bawah). |

## Yang dipakai langsung dari kelompok lain (sudah jadi saat bot ditulis)

- **A**: `src/catalog.rs` (3 produk, harga dasar Rp2.000/B, override via `product_prices`), `src/i18n.rs` (ID+EN lengkap, `t(lang)`), `src/lib/session.rs` (`SessionStore`), `src/lib/wallet.rs` (semua: user/dompet/promo/referral), `src/lib/orders.rs` (semua: order/fulfill/markPaid), `src/lib/support_tickets.rs`, `src/lib/notify.rs` (`NotifyCtx`, `notify_buyer`, `notify_channel_order`).
- **B**: `src/lib/suppliers/` (`get_supplier`, `AnySupplier`, `SupplierAdapter::get_stock`).
- **C**: `src/lib/payments/midtrans.rs` (Snap + tabel payment), `midtrans_poller.rs` (`tick_midtrans_payments`, trait `PaymentSettlement`), `qris.rs`, `qris_poller.rs` (`tick_qris_payments`), `qris_settings.rs` (`get_gopay_static_qris`), `qris_card.rs` (`render_qris_card_or_plain` — standee kalau template ada, QR polos kalau tidak), `gopay.rs` (`GopayClient::new(db, cfg)`).

Catatan proses: saat bot mulai ditulis semua modul lib masih stub; kelompok
A/B/C menyelesaikannya PARALEL selagi bot ditulis. Shim `todo!()` yang sempat
ada di `deps.rs` untuk Midtrans/QRIS sudah DIHAPUS dan diganti pemanggilan
asli. `src/bot/strings.rs` dan `src/bot/session.rs` (duplikat buatan D)
sudah DIHAPUS — pakai `crate::i18n` dan `crate::lib::session`.

## DAFTAR KEBUTUHAN — sudah terpenuhi semua (tidak ada yang menggantung)

Tidak ada lagi fungsi yang diasumsikan: semua call site bot memakai signature
asli yang sudah ada di repo. Yang tersisa hanya dua masalah integrasi +
satu pekerjaan koordinator (di bawah).

## Keputusan perilaku (TS ambigu / quirk yang dipertahankan persis)

1. `targetNote` detail produk di `bot.ts` hardcode Bahasa Indonesia untuk
   semua bahasa — dipertahankan verbatim (kandidat perbaikan i18n, bukan
   perubahan diam-diam).
2. Label status di "Tiket Saya" (`🟢 Terbuka` / `🟡 Dibalas admin` /
   `🔒 Ditutup`) dan kata validitas di `/promo_list` (`kedaluwarsa` /
   `s.d. …` / `tanpa batas`) hardcode ID di TS — dipertahankan.
3. `pay_qris` (GoBiz) TIDAK muncul di layar bayar (hanya fallback +
   handler terdaftar) — sama seperti TS.
4. Menu bot TIDAK menampilkan tombol Referral/Promo/Reseller & API
   (handler tetap ada) — sama seperti TS.
5. `showMenu` dari command kirim banner foto; dari tombol callback edit
   teks (tidak menumpuk foto). Pesan foto diedit via `editMessageCaption` /
   `editMessageMedia`; pesan teks dihapus lalu kirim foto baru.
6. `fulfill` admin: `markPaid` lalu `fulfillOrder` lalu `notifyBuyer`,
   di-await (bukan background) — sama seperti TS.
7. Harga referral di teks i18n: `rupiah(1500)` hardcode di template
   (bukan `cfg.referral_reward_idr`) — sama seperti TS.
8. `fixed-fetch.ts` TIDAK di-port (workaround Node 24 + grammY; teloxide
   pakai reqwest native).
9. Sapaan EN: TS memakai "Good afternoon" untuk jam 10–18 (ikon tetap
   mengikuti jam: ☀️ 10–15, 🌇 15–18). Diimplementasikan via `crate::i18n`.
10. `extract_invite_code` port TS verbatim termasuk dua quirk-nya (tes di
    `src/bot/extract.rs` mengunci perilaku ini):
    - Teks bagikan tanpa link mengambil token 4–12 huruf pertama yang bukan
      stopword — `"Invite friends to Muse, use code UVI0QD!"` → `"FRIENDS"`.
      Teks bagikan Muse asli selalu berisi link invite, jadi jalur link yang
      menangani kasus nyata.
    - Kata polos 4–12 alnum lolos sebagai kode — `"hello"` → `"HELLO"`.
      Kode mati baru ketahuan saat Sondev menebus (lalu refund), persis
      komentar di `bot.ts`.

## Masalah integrasi untuk koordinator

1. **Duplikasi tipe supplier** — `SupplierPort` (orders.rs) dan
   `SupplierAdapter` (suppliers/types.rs) mendefinisikan ulang tipe yang
   sama (`SupplierBalance`, `SupplierProduct`, `SupplierOrderResult`,
   `CreateOrderInput`, `SupplierOrderStatus`) sebagai tipe BERBEDA.
   Satu-satunya beda isi: `wholesale_price_idr` (`Option<i64>` vs
   `Option<f64>`). `src/bot/deps.rs` berisi impl `SupplierPort for
   AnySupplier` yang mengonversi antar keduanya. Yang benar: SATU sumber
   kebenaran (mis. orders.rs memakai ulang tipe dari suppliers/types.rs).
   (Sempat terjadi konflik E0119 sesaat karena kelompok lain juga menulis
   impl yang sama di file yang lalu dihapus; saat ini hanya ada satu impl,
   di `deps.rs`.)
2. **Dua `PaymentSettlement`** — `BotSettlement` (`src/bot/settlement.rs`,
   untuk poller bot) dan `WebSettlement` (`src/web/settlement.rs`, untuk
   webhook/poller web). Keduanya implementasi trait yang sama dari
   `midtrans_poller.rs`. Perlu dipastikan keduanya tetap konsisten
   (atau digabung ke satu tempat).
3. **`set_web_app_menu_button` belum dipanggil dari mana pun** — butuh
   subcommand CLI sekali jalan di `main.rs` (mis. `cargo run --
   set-menu-button`) atau pemanggilan manual. Fungsi sudah ada +
   `#[allow(dead_code)]` + tes. JANGAN auto-panggil saat bot start
   (TS juga tidak melakukannya; akan Err saat URL masih localhost).
4. **`Config` belum punya `GOPAY_POLL_INTERVAL_MS`** — interval poller
   dibaca langsung dari env di `src/bot/mod.rs` (`poll_interval_ms()`).
   Kalau koordinator menambahkannya ke `Config`, pakai field itu.
5. **Belum dites end-to-end live** — bot belum pernah jalan dengan token
   asli di sesi ini (tidak ada token di repo; `cargo check` + tes unit
   saja). Sebelum go-live: jalankan dengan `.env.local`, tap /start,
   dan uji alur beli 1B sampai fulfill (token tetap terkirim beneran
   dari saldo Sondev — target harus akun Muse Dim sendiri, seperti
   rencana TS).

## Verifikasi

- `cargo check`: LOLOS (0 error; hanya warning non-bot + `set_web_app_menu_button`
  yang sengaja `#[allow(dead_code)]`).
- `cargo test --bin unicodes-digital bot::`: **6/6 lolos** (extract 4 +
  menu_button 2). Dua ekspektasi tes awal sempat salah (bukan bug port):
  `"hello"` → `Some("HELLO")` dan teks bagikan tanpa link → `"FRIENDS"` —
  keduanya perilaku TS asli, kini dikunci sebagai tes + didokumentasikan
  sebagai quirk di `src/bot/extract.rs`.
- TIDAK menyentuh `~/workspace/reseller-web`, EC2, GitHub, `main.rs`,
  `config.rs`, `Cargo.toml`. Tidak ada secret yang ditulis ke repo/laporan.
