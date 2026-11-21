# Porting Unicodes Digital — TypeScript → Rust

Koordinator: subagent porting (mulai 2026-11-21 ~16:06 WIB).
Sumber kebenaran: `~/workspace/reseller-web` git HEAD `a5ed97a`.
Target: `~/workspace/reseller-rust/` — satu binary (bot toko + bot news + web).
Kontrak: `README.md` (skema SQLite IDENTIK, env var sama, perilaku bot sama,
jalur uang atomik sama, tanpa secret di repo).

## Status kelompok (2026-11-21 ~16:50 WIB)

| Kelompok | Modul | Status |
|---|---|---|
| A — Inti | db, catalog, i18n, pricing, wallet, orders, session, notify, tiket, harga, telegram_webapp | ✅ DITERIMA (terverifikasi koordinator) |
| B — Supplier | sondev, lahastore, types, registry | ✅ DITERIMA (terverifikasi koordinator) |
| C — Payments | midtrans, poller, qris, qris-card, qris-settings, qris-poller, gopay | ✅ DITERIMA (terverifikasi koordinator) |
| D — Bot | teloxide store bot + news bot | ✅ DITERIMA (terverifikasi koordinator) |
| E — Web | axum + maud, storefront, /tg, /admin, /api, webhook | ✅ DITERIMA (terverifikasi koordinator) |
| F — Tes | porting harness TS (197 checks) → cargo test | ✅ DITERIMA (terverifikasi koordinator) |

## Status akhir gelombang 1+2 (2026-11-21 ~18:15 WIB)

**Semua 6 kelompok selesai dan diterima. `cargo check` lolos,
`cargo test`: 180 passed, 0 failed (5,16 dtk). `cargo clippy`: 0 error,
72 warning (kebanyakan dead-code — fungsi/field belum di-wire penuh).**
Clippy baru diinstal koordinator (`rustup component add clippy`).

### Kelompok F — DITERIMA 2026-11-21 ~18:15 WIB
- **+139 tes baru: 41 → 180 lolos, 0 gagal — direproduksi koordinator**
  (`cargo test --bin unicodes-digital`). Hitungan per file konsisten:
  138 `#[test]` + 42 `#[tokio::test]` = 180.
- Peta section harness (di notes/F-tests.md): ter-cover penuh S2, S4, S4b,
  S5–S12, S14–S17, S19–S22, S24–S27, S30, S31, S34, S35; sebagian S1/S3/S13/
  S23/S28/S29/S33 (teks/logika, bukan alur kirim/handler).
- **Bug produksi: TIDAK ADA.** Hanya bug di kode tes F sendiri (deadlock
  Mutex di helper tes, ekspektasi mask_middle salah hitung, sha256 karangan,
  nama tes menabrak fungsi produksi) — semua diperbaiki.
- Modul A yang tadinya 0 tes kini semua ter-cover (wallet 18, orders 18,
  i18n 10, support_tickets 8, telegram_webapp 7, db 6, catalog 6, pricing 5,
  session 4, product_prices 3, notify 5).
- Tes memakai DB temporer `:memory:` + skema penuh; HTTP di-mock; tidak ada
  logika produksi diubah; main.rs/config.rs/Cargo.toml tidak diubah.
- Catatan: tes lama C di qris.rs memakai `test_db()` skema parsial (hanya
  tabel qris_payments) — dibiarkan (bukan buatan F).

## Verifikasi koordinator

### Kelompok E — DITERIMA 2026-11-21 ~17:10 WIB
- 7 file, 3.001 baris: api.rs (1109), views.rs (937), mod.rs (343),
  pages.rs (231), settlement.rs (133), util.rs (161), auth.rs (87).
- **Full crate: 41 passed, 0 failed — direproduksi koordinator**
  (`cargo test --bin unicodes-digital`; `web_smoke` yang dulu gagal kini
  lolos). `cargo check` lolos (hanya warning future-incompat
  proc-macro-error2 yang sudah dikenal).
- **Spot check lawan TS:** auth cookie `unicodes_admin` = sha256(password),
  HttpOnly, Max-Age 7 hari — cocok `admin-auth.ts`; webhook Midtrans pakai
  `verify_notification_signature` + `processMidtransResult` (transisi
  tunggal); webhook Sondev dedupe `X-Sondev-Delivery` via
  `record_webhook_delivery`; "Sondev" di views.rs HANYA di dashboard admin
  ("Saldo supplier") — storefront bersih (dicek smoke test).
- Semua route TS ada (storefront, /tg, /admin, /admin/tickets[/{id}],
  /api/products, /api/orders + {id} + fulfill, webhook midtrans/sondev,
  admin login/logout/tiket/harga/QRIS/GoPay). axum 0.8 pakai sintaks `{id}`
  (bukan `:id` — ditemukan saat smoke test).
- **Temuan penting E — `main.rs` rapuh:** `tokio::select` atas 3 task
  (bot toko + bot news + web) — kalau SATU task selesai/panic (mis. bot
  news panic `InvalidToken` dengan token dummy), `main` return dan
  SELURUH proses mati termasuk web. Gelombang integrasi harus membuat
  supervisor: task yang mati di-restart / minimal tidak mematikan yang lain.
- E memperbaiki 7 borrow error (E0597) di file A + menambahkan delegasi
  Xiaomi/SheerID ke bridge bot (`bot/deps.rs`) — kalau tidak, fulfillment
  Gemini selalu Err (laten, produk masih coming_soon).

### Kelompok D — DITERIMA 2026-11-21 ~17:05 WIB
- 7 file, 3.182 baris: handlers.rs (2359), mod.rs (215), deps.rs (222),
  settlement.rs (114), extract.rs (113), menu_button.rs (89), news.rs (70).
- **Tes bot 6/6 LOLOS — direproduksi koordinator** (`cargo test bot::`).
  Full crate saat dicek: 40 passed, 1 failed (`web_smoke`, modul E).
- **Dua quirk `extract_invite_code` terverifikasi ASLI dari TS** (bukan
  temuan D): regex dicek lawan `bot.ts:443-475` — PLAIN
  `^[A-Za-z0-9_-]{4,12}$` ("hello" → "HELLO"), TOKEN `[A-Z0-9]{4,12}` +
  stopword ("Invite friends… UVI0QD!" → "FRIENDS"). Semua regex Rust
  identik dengan TS.
- Spot check alur: QTY_PACKS [1,3,5,10,30], MAX_QTY 30 + clamp custom;
  `code_used_up` dipakai di entry kode DAN saat order (seperti TS);
  `require_joined` fail-open bila channel kosong; throttle "Saya Sudah
  Bayar" 10 dtk; distribution function eksplisit per-chat (`mod.rs:203`,
  paralel antar-chat, sekuensial dalam satu chat); `BotSettlement`
  implementasi `PaymentSettlement` ASLI (memakai fungsi A — `mark_topup_paid`
  dst), `UnimplementedSettlement` tidak dipakai bot.
- Catatan: `set_web_app_menu_button` belum dipanggil dari mana pun (butuh
  subcommand CLI sekali jalan — TS juga tidak auto-panggil).

### Kelompok C — DITERIMA 2026-11-21 ~17:00 WIB
- 7 modul, 4.041 baris: midtrans.rs (580), midtrans_poller.rs (371),
  qris.rs (889), qris_card.rs (246), qris_settings.rs (235),
  qris_poller.rs (254, baru), gopay.rs (1440). config.rs +1 field env
  (`GOBIZ_PHONE`, sama seperti `gobizPhone()` di TS `config.ts:134`).
- **Tes payments 26/26 LOLOS — direproduksi koordinator** (`cargo test --bin
  unicodes-digital payments`). Full crate saat dicek: **40 passed, 1 failed**
  — satu-satunya gagal `web::tests::web_smoke` (modul E, masih dikerjakan;
  panic di `src/web/mod.rs:210` soal cookie login admin — BUKAN tanggung
  jawab C; klaim C "39/40, gagal di bot extract test" adalah snapshot lama,
  tes bot itu sudah diperbaiki D).
- **Midtrans 404 = pending terverifikasi** lawan TS (`midtrans.ts:136-141`
  dan `midtrans.rs:186` — HTTP 404 maupun status_code "404" di body).
- **Quirk `markOrderFailed` terverifikasi** lawan TS (`orders.ts:386`:
  hanya dari `processing`/`paid`; dari `awaiting_payment` = no-op) — C
  mem-portnya apa adanya via trait `PaymentSettlement`.
- **Verifikasi webhook SHA512 terverifikasi**: `SHA512(order_id +
  status_code + gross_amount + server_key)` + constant-time compare.
- **KOREKSI atas laporan C**: klaim "ensure_tables di db.rs masih no-op"
  SALAH — `db.rs` kelompok A sudah mengimplementasikannya penuh (semua
  tabel termasuk midtrans_payments/qris_payments/settings). BUKAN blocker.
  Yang benar: ada DUPLIKASI `get_setting`/`set_setting` (db.rs:241,252 DAN
  qris_settings.rs:27,39) — masuk daftar integrasi.
- Deviasi kecil dicatat: QR PNG quiet zone 4 modul (TS margin 2 — QR tetap
  valid); encoder PNG kartu pakai crate `image` (bukan encoder tangan TS);
  `GopayClient::call` di-refactor jadi loop 2-attempt (E0733).
- `UnimplementedSettlement` (todo!()) di midtrans_poller.rs WAJIB diganti
  implementasi nyata (orders/wallet/notify) sebelum bot live — tick akan
  panic saat pembayaran lunas kalau masih ini. Kabar baik: D sudah menulis
  `src/bot/settlement.rs` (`BotSettlement`) menurut laporan C.

### Kelompok B — DITERIMA 2026-11-21 ~16:25 WIB
- 4 file penuh: types.rs (199), sondev.rs (723), lahastore.rs (449), mod.rs (134);
  config.rs +2 field env (`SONDEV_BASE_URL`, `LAHASTORE_BASE_URL`, default = TS).
- Formula HMAC dicek lawan `sondev.ts:76`: `timestamp\nMETHOD\npath\nbody`,
  path LENGKAP via `base_path_of()` — cocok. Unit test menegaskan signature
  path lengkap ≠ path relatif.
- Stok: prioritas `sellable_quantity → available_now → available_stock`,
  `null` = tidak diketahui; `ALREADY_EXISTS` dibaca dari `delivery_status` — cocok TS.
- Tes: **8 passed, 0 failed** — diverifikasi koordinator SECARA INDEPENDEN di
  scratch crate (salinan verbatim config.rs + suppliers/*.rs, `cargo test --offline`),
  scratch crate sudah dihapus. Tes live API tidak dilakukan (butuh kunci live).
- 1 warning: `unused import: SupplierOrderStatus` di mod.rs:23 (re-export,
  hilang saat dipakai kelompok lain).

### Kelompok A — DITERIMA 2026-11-21 ~16:50 WIB
- 11 modul, 4.458 baris: db.rs (260), catalog.rs (221), i18n.rs (1015),
  pricing.rs (37), wallet.rs (877), orders.rs (1025), session.rs (105),
  notify.rs (309), support_tickets.rs (357), product_prices.rs (76),
  telegram_webapp.rs (176).
- Skema: SQL `ensure_tables()` dicek baris-per-baris lawan `db.ts` — SEMUA tabel
  identik (store_orders + webhook_deliveries + users + wallets + wallet_txns +
  wallet_topups + promo_codes + promo_redemptions + referrals + qris_payments +
  settings + channel_posts + midtrans_payments) + support_tickets/messages +
  product_price_overrides dari ensure* TS. Migrasi kolom target/discount_idr/
  promo_code/paid_via via PRAGMA read-only — cocok TS.
- Jalur uang: `mark_paid` / `cancel_order_for_expired_payment` pakai
  `UPDATE ... WHERE id=? AND status='...'` (klaim atomik) + early-return
  idempotent; `fulfill_order` klaim `paid→processing` atomik; `count_code_uses`
  SQL IDENTIK dengan TS (LIKE `%/invite/` + ESCAPE); `with_tx` BEGIN IMMEDIATE.
- i18n: format /start + outOfStock disalin persis (dicek grep lawan i18n.ts);
  katalog Muse 1B Rp2.000.
- **Gap: modul A punya 0 unit test** (`#[test]` = 0 di semua file A) — tes
  menjadi tugas kelompok F (porting harness).

## Keputusan perilaku (dicatat worker, disetujui koordinator)

1. **Web pakai maud 0.27, BUKAN askama** (update skeleton dari parent 16:08 WIB:
   askama_axum 0.5 deprecated, askama 0.14 tanpa fitur axum). Sudah diteruskan
   ke worker E; `cargo check` skeleton lolos.
2. **B: trait `SupplierAdapter` tidak `dyn`-compatible** — metode `-> impl Future`,
   registry pakai enum dispatch `AnySupplier`. `get_stock`/`get_order` pakai
   lifetime eksplisit `'a`. Pemakaian normal tidak terpengaruh.
3. **B: client-side rate limiter Sondev 120 req/menit + retry/backoff** — tambahan
   yang tidak ada di TS, didokumentasikan (bukan penyimpangan perilaku).
4. **B: `is_configured()`** — false kalau kunci kosong (fail-open / "Segera hadir").
5. **A: tidak ada singleton DB global** — semua fungsi terima `&Db`
   (`Arc<Mutex<Connection>>`); helper internal terima `&Connection` (anti-deadlock).
6. **A: `fulfill_order` memakai trait `SupplierPort`** (bukan adapter konkret)
   supaya bisa di-test/di-mock.

## Penyimpangan kecil yang harus dibereskan

- **INTEGRASI — duplikasi tipe supplier (ditemukan 16:55 WIB, dari handoff A):**
  `src/lib/orders.rs:103–198` mendefinisikan sendiri `SupplierOrderStatus`,
  `SupplierBalance`, `SupplierProduct`, `SupplierOrderResult`,
  `CreateOrderInput` + trait `SupplierPort`; `src/lib/suppliers/types.rs`
  (kelompok B) mendefinisikan tipe-tipe sama nama + trait `SupplierAdapter`.
  `fulfill_order` generik atas `SupplierPort`, jadi adapter B (yang impl
  `SupplierAdapter`) TIDAK bisa langsung dipakai — gelombang integrasi harus
  menyatukan keduanya (satu sumber tipe di `suppliers/types.rs` + impl
  `SupplierPort` untuk `AnySupplier`, atau sebaliknya). Jangan biarkan dua
  definisi paralel hidup.
- **INTEGRASI — duplikasi `get_setting`/`set_setting`** (ditemukan 17:00 WIB):
  ada di `db.rs:241,252` (A) DAN `qris_settings.rs:27,39` (C). Satukan ke
  `crate::db`, hapus dari qris_settings.
- **INTEGRASI — dua `PaymentSettlement`** (dari handoff D): `BotSettlement`
  (`src/bot/settlement.rs`) vs `WebSettlement` (`src/web/settlement.rs`) —
  implementasi trait yang sama di dua tempat. Pastikan konsisten atau gabung
  ke satu tempat saat integrasi.
- **INTEGRASI — `GOPAY_POLL_INTERVAL_MS` belum ada di `Config`** (D):
  interval poller dibaca langsung dari env di `src/bot/mod.rs`. Tambahkan
  ke `Config` bila disentuh.
- **INTEGRASI — `UnimplementedSettlement` (todo!())** di midtrans_poller.rs
  wajib diganti implementasi nyata sebelum bot live (D sudah menulis
  `src/bot/settlement.rs` — verifikasi saat handoff D masuk).
- **A: perbandingan hash initData pakai `==` byte** (bukan timing-safe;
  crate `subtle` bukan dependensi) — deviasi kecil dari praktik TS
  (`crypto.timingSafeEqual`); putuskan saat integrasi (tambah `subtle`
  atau terima).
- **A `db.rs` open(): `PRAGMA foreign_keys = ON`** — TS tidak mengaturnya.
  Risiko rendah (wallet selalu dibuat setelah user), tapi demi paritas ketat
  sebaiknya dicabut atau didokumentasikan sebagai keputusan sadar. (Ditandai
  untuk gelombang integrasi.)
- **Crate sempat gagal `cargo check` sesaat** saat worker C/D menulis bersamaan
  (error test-profile 25–30 error di `src/bot/handlers.rs` + `src/lib/payments/`,
  0 error di modul A/B) — normal untuk kerja paralel; `cargo check` (non-test)
  lolos saat dicek 16:25 WIB. Verifikasi akhir menunggu semua worker selesai.
- **437 warning dead-code** di modul A (fungsi pub belum dipanggil bot/web) —
  wajar, hilang saat di-wire oleh D/E.

## Sisa kerja — GELOMBANG INTEGRASI (SELESAI 2026-11-21 ~18:45 WIB)

Lihat "Gelombang integrasi — SELESAI" di bawah. Semua 9 item di bawah ini
dikerjakan; daftar aslinya dipertahankan sebagai jejak.

1. Satukan tipe supplier (orders.rs vs suppliers/types.rs — `wholesale_price_idr`
   `Option<i64>` vs `Option<f64>`) + bridge `SupplierPort`/`SupplierAdapter`;
   `bot/deps.rs` kini kanonik (bridge web sudah dihapus).
2. Satukan `get_setting`/`set_setting` ke `crate::db` (hapus duplikat di
   qris_settings.rs).
3. Gabung/konsistenkan `BotSettlement` vs `WebSettlement`; ganti
   `UnimplementedSettlement` (todo!()) — bot sudah pakai BotSettlement,
   pastikan web tidak memakai Unimplemented.
4. Perbaiki `main.rs` (supervisor): `tokio::select` kini membuat satu task
   panic (mis. news bot `InvalidToken`) mematikan SELURUH proses — task
   yang mati harus di-restart / minimal tidak membunuh yang lain.
5. Putuskan: `PRAGMA foreign_keys = ON` (A), hash initData non-timing-safe (A),
   `GOPAY_POLL_INTERVAL_MS` belum di `Config` (D baca env langsung),
   subcommand CLI untuk `set_web_app_menu_button`.
6. Bersihkan 72 warning clippy (kebanyakan dead-code — hilang saat wiring
   penuh) + warning dead-code 437 di modul A.
7. Isi gap tes: alur bot teloxide end-to-end (butuh mock Bot API),
   `tick_midtrans_payments` utuh + `create_snap_transaction` (butuh transport
   injeksi di midtrans.rs), route `/api/orders` S28 level HTTP (butuh injeksi
   adapter), `reply_to_ticket`/`notify_*` pengiriman (butuh mock transport).
8. Verifikasi akhir: `cargo check` + `cargo test` + `cargo clippy` bersih,
   lalu jalankan binary dengan .env TES (sandbox saja, JANGAN produksi);
   bot menjawab + web 200.
9. TIDAK deploy ke EC2, TIDAK push GitHub — sampai Dim bilang.

## Gelombang integrasi — SELESAI (2026-11-21 ~18:45 WIB)

Empat worker paralel (A unifikasi, B supervisor, C1 keputusan, D gap tes),
masing-masing diverifikasi koordinator dengan `cargo test` + `cargo clippy`
+ baca kode sebelum diterima; merge + bersih-bersih clippy oleh koordinator.

### 1. Unifikasi supplier (worker A)
- `src/lib/suppliers/types.rs` kanonik: `SupplierOrderStatus` (+ `as_str()`,
  `is_terminal()`), `SupplierBalance`, `SupplierProduct`,
  `SupplierOrderResult`, `CreateOrderInput`, trait `SupplierAdapter`.
- `orders.rs` menghapus duplikat + trait `SupplierPort`; `pub use` dari
  `suppliers::types`. `wholesale_price_idr = Option<f64>` (TS number).
  `fulfill_order` / `check_supplier_funds` terikat ke `SupplierAdapter`.
- 4 metode khusus LahaStore jadi **default method** di `SupplierAdapter`
  (default Err, pesan sama seperti `SupplierPort` lama).
- `AnySupplier` meneruskan metode khusus LahaStore via `as_lahastore()`;
  `from_str` di `SupplierOrderStatus` tidak dibawa (nol pemakai).
- `src/bot/deps.rs` (bridge) DIHAPUS — tinggal `product_banner_file_id` +
  `wib_hour_now`.
- `src/lib/settlement.rs` BARU: `CoreSettlement` (7 metode umum, delegasi ke
  orders/wallet); `BotSettlement` + `WebSettlement` masing-masing memegang
  `core: CoreSettlement` + hanya `spawn_fulfill_and_notify` sendiri.
  `UnimplementedSettlement` DIHAPUS koordinator saat merge (nol pemakai).
- **`get_setting`/`set_setting` disatukan ke `crate::db`** (`db.rs`:
  `get_setting` → `Option<String>`, `set_setting` → `Result`); duplikat +
  TODO dihapus dari `qris_settings.rs` (`get_gopay_static_qris` /
  `set_gopay_static_qris` tetap sebagai wrapper).
- **Perubahan perilaku yang disengaja (didokumentasikan di header
  `lib/settlement.rs`):** `mark_order_paid` baru memakai bentuk
  BotSettlement — `WebSettlement` lama menimpa `paid_via` bahkan untuk
  order yang sudah lunas/processing; bentuk baru tidak (perilaku lama
  adalah bug). **Nuansa race (didokumentasikan):** dua pemanggil yang lolos
  cek status bisa sama-sama dapat `Ok(true)` walau hanya satu yang
  membalik status (klaim atomik di `mark_paid`); praktis mustahil karena
  poller/webhook mengklaim baris pembayaran dulu
  (`mark_midtrans_payment_paid` / `mark_qris_payment_paid`) + cek status,
  dan klaim paid→processing di `fulfill_order` mencegah fulfill ganda.

### 2. Supervisor `main.rs` (worker B)
- `supervise(name, factory, shutdown_rx)` per task (store-bot, news-bot,
  web); `TaskFactory` = boxed FnMut → future ter-pin; `Config`/`Db`
  di-clone baru tiap restart.
- Hasil: `Ok(Ok(()))` → log "selesai tanpa error — restart";
  `Ok(Err(e))` → log error; `Err(join)` → payload panic di-downcast
  (`&str`/`String`), log `PANIC: <msg> — restart`; cancel → keluar diam-diam.
- Backoff `BACKOFF_SECS = [1, 2, 5, 10, 30]` detik, reset ke 1 detik setelah
  sehat >60 detik (`HEALTHY_SECS`). Token kosong → task TIDAK di-spawn
  (satu log warning); web selalu di-spawn.
- Shutdown: `shutdown_signal()` menunggu Ctrl-C (SIGINT) atau SIGTERM;
  kirim `true` lewat channel `watch`; supervisor membatalkan backoff;
  main menunggu maks `SHUTDOWN_TIMEOUT` 5 detik, lalu exit 0.
- Subcommand: `cargo run -- set-menu-button` → `run_set_menu_button()`
  (muat config, panggil `bot::menu_button::set_web_app_menu_button`,
  cetak OK/GAGAL, exit 0/1); bot/web TIDAK jalan dalam mode itu.
  README.md +1 baris mendokumentasikan subcommand (kontrak tidak diubah).

### 3. Keputusan (worker C1)
- **`PRAGMA foreign_keys = ON` DIPERTAHANKAN** (hardening sadar, bukan
  paritas TS): satu-satunya FK adalah `wallets.tg_id REFERENCES
  users(tg_id)`; satu-satunya `INSERT INTO wallets` di src/ ada di
  wallet.rs:182 (`INSERT OR IGNORE`, di dalam `ensure_user`, tepat setelah
  upsert users di bawah lock yang sama); semua mutasi wallet lain adalah
  UPDATE; semua jalur yang menyentuh wallet memanggil `ensure_user` dulu;
  baris dev.db lama sudah memenuhi FK. Komentar ditambahkan di `db.rs`
  `open()` + tes `db::tests::foreign_keys_enforced_for_wallets`.
- **Hash initData → constant-time:** helper baru `constant_time_eq`
  di `telegram_webapp.rs` (akumulasi XOR, false hanya bila panjang beda;
  hash selalu hex 64 karakter jadi early-return panjang tidak membocorkan
  apa-apa); `validate_init_data` memakainya. 4 tes baru. Tanpa dependensi
  baru. Catatan: `midtrans.rs:240` punya `constant_time_eq` privat sendiri
  untuk SHA512 webhook — dedup ke helper bersama adalah pekerjaan
  mendatang (opsional, belum dikerjakan).
- **`GOPAY_POLL_INTERVAL_MS` → `Config`:** field baru
  `pub gopay_poll_interval_ms: u64` (default 30000, min 5000 — cerminan
  `gopayPollIntervalMs()` di TS); `bot/mod.rs` `poll_interval_ms(cfg)`
  membacanya; baca env langsung dihapus.

### 4. Clippy — 103 → 1 warning (koordinator, setelah merge)
- Baseline gelombang 1+2: 72 warning (`cargo clippy`) / 103
  (`--all-targets`). Setelah worker: 103 / 86. Setelah bersih-bersih:
  **1 warning di `cargo clippy` DAN `--all-targets`, 0 error.**
- Satu-satunya warning tersisa: `proc-macro-error2 v2.0.1`
  future-incompat (eksternal, tidak bisa diperbaiki dari sini).
- `special_module_name` (`mod lib;` di main.rs): item-level
  `#[allow]` TIDAK mempan; **crate-level `#![allow(special_module_name)]`
  di baris pertama main.rs mempan**.
- Perbaikan mekanis: `3600_000` → `3_600_000` (2 tempat),
  `matches!`, `.is_multiple_of(3)`, `&&str` → `&str`, `assert!`,
  `.map(json_string)`, CRC16 qris (loop `enumerate` + hapus mask
  `& 0xffff` no-op pada u16), alias tipe `ProductsCache`/`MockHandler`/
  `StatusMap`, `filter_map` → `map`, `trim_end_matches([...])`,
  `as_arc(&self)`, `!status.configured`, 11 baris indentasi doc-list,
  `#[allow(clippy::await_holding_lock)]` pada tes Gap-3 (env lock memang
  harus merentang await — disengaja).
- **DIHAPUS (nol pemakai di mana pun, termasuk tes):**
  `list_active_products_static` (catalog.rs),
  `run_midtrans_poller_loop` + `MIDTRANS_POLL_INTERVAL_SECS`,
  `run_qris_poller_loop` + `QRIS_POLL_INTERVAL_SECS`,
  `as_sondev` + `all_suppliers` + `SUPPLIER_IDS` (suppliers/mod.rs),
  fitur status publik Sondev: `public_call`, `get_public_status`,
  `SondevPublicStatus`, `PUBLIC_TIMEOUT`,
  `MUSE_TOKEN_WHOLESALE_IDR_PER_B`, `SONDEV_DEFAULT_BASE_URL`,
  `list_orders`, `get_logs`, `test_webhook`, `get_dead_letters`,
  `replay_webhooks`, `json_bool` (types.rs),
  varian `NotConfigured` (midtrans_poller.rs + satu match arm di
  bot/mod.rs), `now_iso` (payments/mod.rs — `iso_of` tetap dipakai).
  Catatan perbaikan: penghapusan blok metode Sondev sempat ikut menghapus
  kurung tutup `impl` (error "unclosed delimiter") — diperbaiki dengan
  menambah `}` sebelum `fn map_status`.
- **DISATUKAN:** `MAX_CODE_USES`/`MAX_QTY` yang diduplikasi handlers.rs
  dihapus — handlers kini mengimpor dari `crate::lib::orders`.
- **DIIZINKAN `#[allow(dead_code)]` + komentar alasan** (permukaan API
  port / cerminan baris DB / hanya dipakai tes): `set_order_target`,
  `get_qris_payment`, `mark_qris_payment_cancelled`,
  `find_qris_payment_for_transaction`, `clear_price_override`,
  `list_price_overrides`, `credit_wallet`, 5 fn pricing.rs (allow
  se-modul), `LAHASTORE_DEFAULT_BASE_URL`, metode trait `id` /
  `is_configured`, varian `Target` + `set`/`clear`/`cancel_awaiting`
  (session.rs), `MidtransClient::from_config`, field Config
  `midtrans_client_key` + `referral_reward_idr`, field struct yang tidak
  dibaca (wallet.rs 6 struct, i18n.rs 2 struct, telegram_webapp.rs 2,
  midtrans.rs 2 struct, qris.rs `QrisPaymentRow`, qris_card.rs
  `QrisCardImage`, lahastore.rs 3 struct hasil + field `idr_per_usd` +
  metode `usd_to_idr`/`vnd_to_idr` + `VND_PER_USD`).

### 5. Gap tes terisi (worker D) — 180 → 198 tes
- **+13 tes, semua hermetik** (127.0.0.1, port ephemeral, `:memory:` /
  DB /tmp yang dihapus, nilai dummy). `cargo test` **198 passed,
  0 failed** (dua run stabil).
- **Seam baru (perilaku default produksi identik):** `MidtransClient {
  client, snap_base, api_base }` (`from_config`, `from_client`,
  `with_bases`; override env `MIDTRANS_SNAP_BASE_URL` /
  `MIDTRANS_API_BASE_URL`); `create_snap_transaction_with` +
  `tick_midtrans_payments_with`; `get_transaction_status` menerima
  `&MidtransClient`. `create_snap_transaction` / `tick_midtrans_payments`
  dipertahankan sebagai wrapper tipis karena pemanggil di bot/handlers.rs
  di luar scope D. `NotifyCtx.api_base` (default
  `https://api.telegram.org`) + `send_telegram_html_with_base` /
  `send_telegram_html_with_markup_and_base`.
- **Gap 1 — bot e2e:** `bot::tests::start_command_end_to_end_via_mock_bot_api`
  — dispatcher asli + mock Bot API lokal; `/start` → tepat satu
  `sendMessage` berisi "UNICODES DIGITAL" ke chat 111 + baris user
  `tg:111` ("Budi") dibuat di DB. Gotcha: nama metode Bot API PascalCase
  (`/GetMe`, `/SendMessage`) — mock huruf kecil mengembalikan 404 →
  `Api(InvalidToken)`.
- **Gap 2 — Midtrans tick + Snap:** 8 tes (Snap mock: token + redirect +
  header Basic Auth; 401 → bail membawa `error_messages`; status
  settlement; 404 → pending bukan error; `from_config` sandbox default;
  poller: settlement menandai order lunas + spawn fulfill; 404 membuat
  order tetap pending; payment kedaluwarsa → order failed).
- **Gap 3 — /api/orders HTTP (S28):**
  `web::api::tests::create_order_end_to_end_with_mock_sondev_and_snap`
  — POST /api/orders valid → **201**, `snapToken` `SNAP-TEST-TOKEN`,
  `totalIdr` 2000, order tersimpan `awaiting_payment`; GET /api/orders/{id}
  mengembalikan order yang sama.
- **Gap 4 — notify:** 3 tes (buyer, channel bermasker + dedupe,
  admin) — Order ID termasker `UD-T••••3456`, ID lengkap tidak bocor.
- **Catatan D:** `web_smoke` memakai env global (PORT/DATABASE_PATH)
  yang masih racy secara umum — tes D memakai port/DB unik + Midtrans
  env lock, tapi risiko umumnya tetap ada.

### 6. Verifikasi akhir runtime (koordinator, 2026-11-21 ~18:45 WIB)
- `.env` TES di `/tmp/rust-final-test/.env` (token dummy
  `123456:DUMMYSTORE` / `123456:DUMMYNEWS`, PORT 3225, DB sementara
  `/tmp/rust-final-test/test.db` — skema saja, 0 user / 0 order).
- Web: **homepage 200, /tg 200** (curl, saat kedua bot sedang restart).
- Supervisor: kedua bot panic `InvalidToken` (token dummy) → log
  `PANIC: Couldn't prepare dispatching context: Api(InvalidToken) —
  restart` (10x total) + backoff **1→2→5→10→30 detik** (masing-masing
  2x, satu per bot); **web tetap 200 sepanjang waktu** — satu task panic
  tidak lagi mematikan proses.
- Shutdown: SIGINT ke PID binary → log "Sinyal shutdown diterima…",
  web "shutdown — task dihentikan", kedua bot "shutdown saat backoff —
  batal restart", "Unicodes Digital (Rust) berhenti dengan tenang."
  (main mengembalikan `Ok(())` → exit 0); web setelahnya 000
  (koneksi ditolak). Catatan: SIGINT pertama sempat salah sasaran ke
  shell pembungkus (PID 47833) bukan binary (47834) — pastikan kill ke
  PID binary.
- DB tes: 17 tabel dibuat, `users` 0, `store_orders` 0. Tidak ada token
  asli / DB live yang dipakai. Binary TIDAK di-deploy ke mana pun.

### Gap paritas yang diketahui (tetap)
- **`Config.referral_reward_idr` mati:** wallet.rs:20 punya
  `pub const REFERRAL_REWARD_IDR: i64 = 1500;` sendiri yang dipakai
  `maybe_reward_referrer` — TS membaca env override, Rust
  mengabaikannya (reward tetap 1500 = default). Menghubungkannya butuh
  threading Config lewat fungsi wallet — di luar scope.
- **`midtrans_client_key` tidak dipakai server-side:** TS memakainya
  untuk Snap.js di browser; Rust menyajikan redirect URL Snap
  server-side.
- `constant_time_eq` ganda (telegram_webapp.rs vs midtrans.rs:240) —
  dedup opsional mendatang.
- `web_smoke` env global racy (catatan D di atas).
- `count_code_uses` best-effort di bawah konkurensi (sama seperti TS —
  Sondev menegakkan batas 30 yang sesungguhnya).

### Redesign dashboard admin (2026-11-21 ~18:15–18:30 WIB)
- **Pemicu:** Dim login ke /admin versi TS yang live dan protes tampilannya
  ("dashboard macam apa ini") — bullet list + tabel mentah tanpa desain.
- **Yang diubah (hanya `src/web/views.rs`, presentasi saja):** CSS admin
  baru di `BASE_CSS` (`.stat-card`, `.admin-card`, `.supplier-card`,
  `.badge-*`, `.table-scroll`, `.admin-grid`, `.admin-row`, `.admin-nav`,
  `.login-card`, `.msg-bubble` + media query ≤560px); `main` 800px → 960px.
  - Statistik jadi 5 kartu berjajar (Total Pengguna, Total Order,
    Terkirim, Revenue, Tiket Terbuka — angka besar emas + label kecil).
  - Supplier jadi kartu per supplier (Sondev / LahaStore) + badge
    Aktif/Gagal dicek; semua info TS tetap tampil (reseller, webhook,
    order hari ini, rate limit).
  - Panel QRIS GoPay jadi 4 kartu (Sesi GoBiz, QRIS statis, QR tes,
    Pembayaran terbaru); **semua 28 `id` elemen dipertahankan** —
    `QRIS_PANEL_JS` tidak diubah; input responsif (flex, wrap di HP).
  - Produk & harga retail jadi tabel rapi (edit harga inline, form
    action + `name="price"` + min/max/step tidak berubah); Pengguna dan
    Order terbaru jadi tabel dalam `.table-scroll` (scroll horizontal
    di HP, tidak ada teks kepotong).
  - Daftar/detail tiket: tabel dalam `.table-scroll`, bubble pesan
    pakai class; semua form action + `name="text"` tidak berubah.
  - Nav Dashboard / Tiket Support / Keluar jadi pill links; login jadi
    kartu tengah (form `POST /api/admin/login`, `name="password"` tetap).
- **Tidak diubah:** route, URL, nama field form, nama cookie, API,
  handler `api.rs`, bot, supplier, payment, skema DB. String yang
  diassert tes (`"Total pengguna"`, `"Saldo supplier"`, `"harga retail"`)
  dipertahankan.
- **Verifikasi:** `cargo check` lolos; `cargo test` **198 passed, 0 failed**;
  `cargo clippy --all-targets` 0 error, 1 warning (proc-macro-error2
  eksternal, sudah diketahui). Run binary debug dengan DB tes
  (`/tmp/rust-admin-test/test.db`, disiapkan 2 user + 2 order) + token
  dummy, PORT 3227: `/admin` tanpa cookie → form login; login
  `POST /api/admin/login` → 303 + cookie HttpOnly; `/admin` dengan
  cookie → **200**, 5 kartu statistik, 3 produk + tombol "Ubah harga",
  revenue Rp10.000 (order delivered), semua 28 ID panel QRIS ada,
  **0 tag `<ul`** (bullet list lama hilang); `/admin/tickets` render;
  homepage + `/tg` 200. Server tes dimatikan + direktori tes dihapus.
  Catatan: `pkill -f` dengan string target sempat membunuh shell exec
  sendiri (pelajaran lama) — server tetap mati, diverifikasi port 000.


## Perbaikan keterbacaan dashboard admin — palet netral (2026-11-21 ~18:50 WIB)

Dim protes dashboard hasil redesign pertama: "warna nya juga bikin sakit mata
susah dibaca" — seluruh halaman ungu pekat, teks ungu muda di atas ungu gelap,
kontras rendah. Layout sudah benar, yang salah palet + tipografi. Diperbaiki:

- **Palet lama → baru (admin saja):** bg `#14092b` → `#101014` (hampir hitam
  netral); kartu `#221244`/gradasi ungu → `#1b1e26` (abu gelap netral,
  border `#2e323d`); teks utama `#ede9fe` → `#f4f4f5`; teks sekunder
  `#b3a1e6` → `#a1a1aa`; angka kartu emas `#e8c874` → `#f4f4f5`. Ungu
  `#7c3aed` kini HANYA untuk tombol utama, pill nav aktif, dan garis aksen
  (border-top 3px kartu statistik, border-bottom 2px header). Link admin
  memakai `#a78bfa` (violet terang) — `#7c3aed` sebagai warna teks di atas
  `#101014` kontrasnya hanya ~3,3:1, di bawah ambang baca; `#a78bfa` ~7:1.
- **Badge status dilembutkan:** ok `#059669`→`#166534` + teks `#dcfce7`;
  err `#dc2626`→`#7f1d1d` + teks `#fecaca`; warn `#b45309`→`#78350f` +
  teks `#fde68a` (bg gelap + teks pucat, bukan warna menyala polos).
  Inline `.err`/`.ok` jadi `#fca5a5`/`#86efac`; tombol "Tutup tiket"
  `#4c1d95` → `#7f1d1d`; tombol "Keluar" `#4c4466` → `#3f3f46`.
- **Tipografi:** font dasar 15px, line-height 1.6; judul seksi 19px→21px;
  `.admin-head` 24px→26px; `.admin-card h3` 16px→18px; `.muted`/`.err`/`.ok`
  13px→14px; label kartu 13px→14px; meta supplier 13px→14px; isi pesan
  tiket 14px→15px; teks mikro tabel (tanggal, referensi, error) 12px→13px.
  Padding kartu 16px→18px, gap antar kartu 12px→16px.
- **Cara kerja:** `admin_layout()` baru di `views.rs` me-render `<body
  class="admin">`; semua aturan admin ditulis di bawah `body.admin`
  (override sesudah aturan base) — halaman buyer TETAP memakai `layout`
  dan tema ungu Scaramouche-nya tidak berubah. `pages.rs` hanya mengganti
  pemanggilan `layout(` → `admin_layout(` di 6 call-site admin; tidak ada
  logika/route/field/cookie/API yang berubah.
- **Verifikasi:** `cargo test` **198 passed, 0 failed**; `cargo clippy
  --all-targets` 0 error, 1 warning (proc-macro-error2 eksternal).
  Run binary debug, DB copy (18 user, 6 order) + token dummy, PORT 3230:
  homepage 200, `/admin` tanpa cookie → form login, `/admin` dengan cookie
  → **200**, `body class="admin"`, palet baru hadir (`#101014`, `#1b1e26`,
  `#f4f4f5`, `#a1a1aa`), `#221244`/`#e8c874` = 0 di HTML admin, angka kartu
  18/6/0/Rp0/0 benar, `/admin/tickets` 200, homepage tetap ungu
  (`#14092b` hadir — tema buyer utuh). Sisa 1x `#1a0f38`/`#6d28d9`/`#b3a1e6`
  di HTML admin adalah teks aturan base (buyer) yang sudah di-override
  oleh `body.admin` — tidak ter-render. Semua 27 ID unik panel QRIS
  (15 gopay-*, 12 qris-*) ada tepat 1x — catatan: hitungan sebelumnya
  "28" keliru satu, yang benar 27 dan semuanya utuh. Server tes dimatikan,
  direktori tes (berisi .env tes) dihapus.
- **Belum di-deploy:** butuh rebuild release + redeploy EC2 agar Dim
  melihatnya — versi Rust yang live saat ini masih memakai palet ungu lama.

## Penghapusan panel QRIS GoPay dari /admin (2026-11-21 ~19:00 WIB)

- **Alasan:** Dim sudah dua kali menegaskan panel QRIS GoPay/GoBiz tidak
  perlu lagi karena pembayaran sudah pakai Midtrans; setelah perbaikan warna
  saja panelnya masih tampil dan Dim protes lagi. Kali ini dihapus dari
  tampilan sampai benar-benar hilang.
- **Yang dihapus (hanya tampilan, `src/web/views.rs` + `src/web/pages.rs`):**
  judul seksi "Pembayaran — QRIS GoPay" + `qris_panel()` (kartu Sesi GoBiz,
  QRIS statis GoPay, QR tes, Pembayaran QRIS terbaru), konstanta
  `QRIS_PANEL_JS`, struct `QrisStatusView`, field `qris` di
  `AdminDashboardData`, fn `qris_status()` di `pages.rs` + pemanggilannya,
  dan import `qris_settings` yang terkait. Subtitle dashboard disesuaikan
  ("Ringkasan toko, saldo supplier, produk, dan pengguna."). Catatan "Isi
  saldo sebelum kehabisan" di bawah Saldo supplier TETAP ada.
- **Yang TIDAK dihapus (backend dormant):** route
  `/api/admin/payments/qris/*` dan `/api/admin/payments/gopay/*`, kode
  `qris.rs`/`qris_settings.rs`/`gopay.rs`/`qris_poller.rs`, poller, dan tabel
  DB — tetap ada, hanya tidak terjangkau dari UI. Palet netral hasil
  perbaikan keterbacaan tidak berubah. Tidak ada perubahan route, URL, field
  form, cookie, atau handler lain. Tidak ada tes yang perlu diubah (tidak
  ada tes yang meng-assert seksi QRIS di HTML admin).
- **Verifikasi:** `cargo test` **198 passed, 0 failed**; `cargo clippy
  --all-targets` 0 error, 1 warning (proc-macro-error2 eksternal). Run
  binary debug, DB copy (18 user, 6 order) + token dummy, PORT 3231:
  homepage 200; `/admin` dengan cookie → 200 dengan grep **0** untuk
  "GoBiz", "QRIS GoPay", "QRIS statis", "Minta OTP", "Pembayaran — QRIS";
  kartu statistik 18/6/0/Rp0/0 benar; "Saldo supplier" dan "Isi saldo
  sebelum kehabisan" tetap tampil; `/admin/tickets` 200. Server tes
  dimatikan, direktori tes (berisi .env tes) dihapus.
- **Belum di-deploy:** butuh rebuild release + redeploy EC2 (restart service
  `unicodes-digital-rust`) agar Dim melihatnya — versi Rust yang live saat
  ini masih menampilkan panel QRIS.

## Pilih order di support (2026-11-21 ~19:15 WIB)

- **Permintaan Dim:** saat buka support, user bisa memilih order terakhirnya
  dulu, baru lanjut ke support (tiket).
- **Alur bot (`src/bot/handlers.rs`):** tombol 💬 Support / "Buat Tiket
  Support" / "Buat Tiket Baru" (`support_msg`) kini memanggil
  `start_support()`: kalau user SUDAH punya tiket terbuka -> langsung minta
  pesan (tanpa pilih order, pesan lanjutan masuk ke tiket itu seperti biasa);
  kalau tidak ada tiket terbuka -> layar pilih order (`show_support_pick_order`):
  5 order terakhir + tombol "Tanpa order / Lainnya". Pilih satu order
  (`supord:<id>`, dicek kepemilikannya via `buyer_ref`) -> minta pesan dengan
  menyebut order itu; "Tanpa order" (`supord:none`) -> tiket tanpa order
  (perilaku lama). User tanpa order sama sekali -> langsung minta pesan.
  Pilihan order disimpan di sesi (`AwaitingInput::Ticket { order_id }`) dan
  hanya dipakai saat tiket BARU dibuat; pesan lanjutan mengabaikannya.
- **Baris order (ID + EN):** ikon status + ID order + produk + qty + total Rp.
  Pemetaan status: delivered -> ✅; failed/refunded -> ❌ (status "expired"
  tidak ada sebagai status order — hanya status pembayaran); sisanya
  (awaiting_payment, paid, processing) -> ⏳ "Diproses"/"Processing".
  Refunded dipetakan ke ❌ (terminal, bukan "Diproses") — keputusan sadar.
- **DB (`src/db.rs`):** kolom `order_id TEXT` (nullable) di `support_tickets`
  — ada di CREATE TABLE (DB baru) + migrasi ringan (PRAGMA table_info, ALTER
  TABLE hanya kalau belum ada; pola sama seperti migrasi store_orders).
- **Lib (`src/lib/support_tickets.rs`):** `SupportTicket.order_id`, semua
  SELECT menyertakan order_id, `create_ticket(..., order_id)`; pesan lanjutan
  tidak mengubah order_id tiket lama.
- **Admin web:** detail tiket menampilkan kartu "📦 Order terkait" (ID, nama
  produk, qty, status, total Rp) kalau ada; daftar tiket punya kolom "Order";
  JSON `/api/admin/tickets*` menyertakan `"orderId"`. Admin web tetap
  berbahasa Indonesia saja (tidak ada toggle EN) — label "Order terkait"
  hanya ID. Tidak ada route/URL yang berubah; palet netral tidak berubah.
- **i18n (`src/i18n.rs`):** `support_pick_order_title`,
  `support_pick_no_order`, `support_order_processing`,
  `support_order_row`, `support_ask_with_order` — ID + EN.
- **Tes baru (203 total, 0 gagal):** `create_ticket_with_bound_order`,
  `create_ticket_without_order_has_none`, `follow_up_keeps_bound_order`
  (support_tickets.rs); `support_tickets_order_id_migration_from_old_schema`
  (db.rs, skema lama -> migrasi -> idempotent);
  `support_order_rows_show_status_icons_and_format` (handlers.rs — 5 status
  x ID/EN, format baris + callback data `supord:<id>`).
- **Verifikasi runtime:** binary debug, DB tes seed (2 order + 2 tiket,
  satu terikat UD-TESTDEL) + token dummy, PORT 3232: `/admin/tickets`
  menampilkan kolom Order (UD-TESTDEL 1x); detail TKT-ABC123 menampilkan
  kartu "Order terkait" (UD-TESTDEL, Muse AI Token 1B, Terkirim, Rp10.000);
  detail TKT-DEF456 tanpa kartu; JSON `orderId` benar. Server tes dimatikan,
  direktori tes (berisi .env tes) dihapus.
- **Belum di-deploy:** butuh rebuild release + redeploy EC2 (restart service
  `unicodes-digital-rust`) agar Dim melihatnya.
