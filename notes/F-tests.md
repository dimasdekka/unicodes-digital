# Kelompok F — Porting harness TS (197 checks) → `cargo test`

Worker: subagent F, 2026-11-21. Baseline sebelum F: **41 tes lolos, 0 gagal**
(`cargo test --bin unicodes-digital`); modul kelompok A (db, catalog, i18n,
pricing, wallet, orders, session, notify, support_tickets, product_prices,
telegram_webapp) semuanya **0 tes**.

Prinsip: tes di dalam modul (`#[cfg(test)]`, DB temporer `:memory:` via
`db::open` + `ensure_tables` asli — TIDAK PERNAH sentuh `dev.db`, TIDAK
PERNAH buat tabel skema parsial). HTTP di-mock (trait `GopayHttp` / fake
`SupplierPort` / fake `PaymentSettlement`); tidak ada panggilan API
Sondev/Midtrans/Telegram sungguhan; nilai dummy semua.

## Hasil akhir

- Sebelum F: **41 tes lolos, 0 gagal**. Sesudah F: **180 tes lolos, 0 gagal**
  (`cargo test --bin unicodes-digital`, 2026-11-21 ~18:10 WIB).
  **+139 tes baru.** `cargo check` bersih (hanya warning future-incompat
  proc-macro-error2 yang sudah dikenal + warning dead-code lama).
- `main.rs` / `config.rs` / `Cargo.toml` TIDAK diubah. Tidak ada file di
  luar `~/workspace/reseller-rust/` yang disentuh. Tidak ada push GitHub,
  tidak ada sentuhan EC2 / `reseller-web`.

## Peta section harness → modul Rust → status

| Section harness | Isi | Modul Rust | Status |
|---|---|---|---|
| S1 | /start kaya + setMyCommands | `i18n::welcome`, `bot::handlers::BOT_COMMANDS` | ✅ tes unit (teks + konstanta); alur kirim via teloxide tidak ter-cover (butuh mock Bot API) |
| S2 | referral via /start ref_ | `lib::wallet::record_referral`, `get_referral_stats` | ✅ |
| S3 | menu + katalog + detail | `catalog`, `i18n::product_detail` | ✅ |
| S4 | checkout kode → qty → ringkasan | `bot::extract`, `i18n::qty_title`/`pay_title`, `orders::count_code_uses` | ✅ (logika); alur callback bot tidak ter-cover |
| S4b | satu kode maks 30x | `orders::count_code_uses` | ✅ (30x + link + escape `_`) |
| S5 | dompet + top-up | `lib::wallet` | ✅ |
| S6 | Midtrans top-up lunas, idempotent | `wallet::mark_topup_paid`, `midtrans_poller::process_midtrans_result` | ✅ |
| S7 | bayar saldo → delivered + notifikasi | `orders::fulfill_order` (fake supplier), `wallet::debit_wallet` | ✅ (fulfill); `notify_buyer` kirim Telegram tidak ter-cover |
| S7b | klaim atomik markPaid/fulfill ganda | `orders::mark_paid`, `fulfill_order` | ✅ (termasuk 2 thread bersamaan) |
| S8 | reward referral sekali | `wallet::maybe_reward_referrer` | ✅ |
| S9 | promo | `wallet::{create,validate,calc,redeem}_promo` | ✅ |
| S10 | tiket support | `lib::support_tickets` | ✅ (DB); `reply_to_ticket`/`close_ticket` kirim Telegram tidak ter-cover (butuh mock HTTP Bot API) |
| S11 | pesanan saya + detail | `orders::{list_orders_by_buyer,get_store_order}`, `i18n::order_detail` | ✅ |
| S12 | bahasa ID/EN | `wallet::set_user_lang`, `i18n::t` | ✅ |
| S13 | kebijakan + reseller | `i18n` (policy/reseller) | ✅ (teks) |
| S14 | legacy /products /order /fulfill | `orders::{create,get}_store_order`, `fulfill_order` | ✅ (logika); parsing command `/fulfill` admin di handler tidak ter-cover |
| S15 | validasi initData WebApp | `lib::telegram_webapp::validate_init_data` | ✅ (valid/tamper/expired/tanpa hash/token salah/tanpa auth_date/user rusak) |
| S16 | render /tg tanpa env Telegram | `web` smoke test (sudah ada) | ✅ sudah ada |
| S17 | menu button Web App | `bot::menu_button` | ✅ (+ `set_web_app_menu_button` menolak saat localhost) |
| S18 | banner di /start & /menu | handler bot (foto) | ❌ tidak ter-cover — butuh mock Bot API teloxide |
| S19 | kategori + stok | `catalog`, `i18n::out_of_stock` | ✅ |
| S20 | kode invite, qty custom, saldo kurang, Snap | `extract`, `wallet::MIN_TOPUP_IDR`, `payments::midtrans` (row + signature) | ✅ (logika); validasi qty custom inline di handler — tidak ter-cover |
| S21 | top-up custom | `wallet::{create_topup, MIN_TOPUP_IDR}` | ✅ |
| S22 | promo tidak ditemukan/kedaluwarsa/habis | `wallet::validate_promo` | ✅ |
| S23 | order tidak ada, blokir admin | `orders::get_store_order` (None) | ⚠️ sebagian — blokir admin di handler tidak ter-cover |
| S24 | referral diri sendiri + EN | `wallet::record_referral`, `i18n` EN welcome | ✅ |
| S25 | route admin GoBiz (session/merchant/poll) | `web::api::{gopay_session,gopay_merchant,gopay_poll}` + fake `GopayHttp` | ✅ (handler langsung, AppState rakitan) |
| S26 | feed duplikat → fulfill sekali | `payments::qris_poller::tick_qris_payments` + fake HTTP | ✅ |
| S27 | fulfillment gagal → refund | `orders::fulfill_order` (fake supplier gagal; wallet refund vs QRIS manual) | ✅ |
| S28 | cek saldo supplier via route order | `orders::check_supplier_funds` (unit) | ⚠️ sebagian — route `/api/orders` membangun adapter Sondev asli dari cfg, tidak bisa di-mock tanpa injeksi adapter |
| S29 | sesi /cancel, callback asing, tombol back | `lib::session::cancel_awaiting` | ⚠️ sebagian — /cancel + callback asing di handler tidak ter-cover |
| S30 | poller: lunas, sesi habis, QR kedaluwarsa | `payments::qris_poller::tick_qris_payments` | ✅ (match 1, auth_required, expiry sweep) |
| S31 | GoPay OTP via route admin | `web::api::{gopay_otp,gopay_verify,gopay_logout}` + fake `GopayHttp` | ✅ (handler langsung) |
| S32 | tombol "Saya Sudah Bayar" (cek manual Midtrans) | handler bot + `midtrans::get_transaction_status` (HTTP live) | ❌ tidak ter-cover — cek manual + throttle 10 dtk hidup di handler bot; `get_transaction_status` tidak punya transport injeksi |
| S33 | wajib-join channel + feed NEW ORDER | `lib::notify::mask_middle`, early-return `notify_channel_order` | ⚠️ sebagian — kirim ke channel + `getChatMember` butuh Bot API |
| S34 | bayar via Midtrans Snap | `payments::midtrans` (row/signature/status), `web::api::midtrans_webhook` | ✅ (webhook: signature benar → 200, salah → 401); `create_snap_transaction` HTTP live tidak ter-cover |
| S35 | bot news | `bot::news::info_text` | ✅ (teks; long-polling tidak ter-cover) |

## Tes yang ditambah (per modul)

| Modul | Tes baru | Isi |
|---|---|---|
| `src/db.rs` | 6 | semua tabel ada; kolom migrasi target/discount_idr/promo_code/paid_via; settings round-trip; webhook dedupe |
| `src/catalog.rs` | 6 | Muse Rp2.000; 3 produk; kategori; per-kategori; harga efektif + override |
| `src/i18n.rs` | 10 | rupiah; wib_time; welcome ID (S1) + EN (S24); product_detail; out_of_stock; code_used_up; qty_title; pay_title; promo_invalid; status_label |
| `src/lib/pricing.rs` | 5 | vnd_to_idr; usd_to_idr; retail_price ceil-500 (contoh dok 8500→12000); default |
| `src/lib/wallet.rs` | 18 | user/lang; credit/debit/insufficient; txn + total; topup min + mark_topup_paid idempotent; promo (buat/validasi/diskon/redeem sekali); referral (catat/self/dup/reward sekali); tg_id_from_buyer_ref; kredit konkuren 8 thread |
| `src/lib/orders.rs` | 17 | create total/diskon clamp; count_code_uses (polos/link/`_` escape/30x/0); mark_paid konkuren 2 thread + idempotent; cancel expired; mark delivered/failed quirk; fulfill sukses/gagal-wallet-refund/gagal-QRIS-manual/fulfill ganda 1x supplier; check_supplier_funds kurang + fail-open |
| `src/lib/session.rs` | 4 | get/set/clear/cancel_awaiting/update |
| `src/lib/notify.rs` | 5 | mask_middle (pendek/panjang, S33 `UD-T••••3456`); notify_channel_order early-return |
| `src/lib/support_tickets.rs` | 8 | format TKT-; buat + pesan pertama; pesan kedua gabung; tutup → tiket baru; list/count; escape_html; reopen |
| `src/lib/product_prices.rs` | 3 | set/get/clear/list + harga efektif |
| `src/lib/telegram_webapp.rs` | 7 | S15 lengkap |
| `src/bot/extract.rs` | 1 | kasus harness baru: kode `_`/`-` (+ `hi`/spasi ditambah ke tes lama) |
| `src/bot/handlers.rs` | 3 | BOT_COMMANDS (5, tanpa menu); QTY_PACKS; parse_command |
| `src/bot/menu_button.rs` | 1 | set_web_app_menu_button menolak saat localhost (S17) |
| `src/bot/news.rs` | 2 | info_text (S35: News & Info, link toko, sensor, channel opsional) |
| `src/lib/payments/qris.rs` | 4 | dynamic builder payload rusak; checksum invalid; allocator habis; alokasi konkuren 8 thread (nominal unik semua) |
| `src/lib/payments/midtrans.rs` | 4 | map deny/cancel/expire; signature key salah; CRUD + expiry sweep |
| `src/lib/payments/midtrans_poller.rs` | 6 | process_midtrans_result: topup Paid, order Paid, Pending, deny Failed, idempotent kedua → Ignored |
| `src/lib/payments/qris_poller.rs` | 4 | tick: tanpa pending → 0; feed cocok → match 1; feed duplikat → 0; sesi habis → auth_required; QR kedaluwarsa → sweep + order failed |
| `src/web/auth.rs` | 5 | hash/cookie valid; cookie salah; tanpa password; parse cookie; header login/logout |
| `src/web/api.rs` | 13 | webhook Midtrans (S34: tanpa config → 500, body tak lengkap → 400, sig benar → 200 + topup/order lunas, sig salah → 401); gopay_session (401 tanpa cookie; tanpa/dengan sesi); gopay_merchant (profil + QRIS cocok); gopay_poll (auth_required; match feed 1); gopay_otp (ok, otp_token tidak bocor; 401 tanpa cookie); gopay_verify (format salah 400, sukses → sesi login + logout) |
| `src/lib/suppliers/sondev.rs` | 3 | to_order_result bentuk harness DELIVERED + FAILED; percent_encode |
| `src/lib/payments/midtrans.rs` | 5 | signature key salah; status unknown → Failed; midtrans_configured; duplikat order_id; expiry sweep |
| `src/lib/payments/qris.rs` | 5 | dynamic builder payload rusak; checksum invalid; allocator habis; alokasi konkuren 8 thread; match_feed status/waktu |
| **Total baru** | **139** | |

## Bug yang ditemukan

- **Bug produksi: TIDAK ADA.** Semua 180 tes lolos terhadap perilaku yang
  ada; tidak ada logika produksi yang diubah.
- **Bug di kode TES saya sendiri (diperbaiki saat penulisan):**
  1. Deadlock `Mutex` di 2 helper tes (`db::ensure_tables_is_idempotent`,
     `notify::delivered_order`): guard `db.lock()` dipegang sambil memanggil
     fungsi yang me-lock mutex yang sama (Mutex tidak re-entrant) — 3 tes
     menggantung >60 detik. Perbaikan: batasi scope guard dengan blok `{}`.
  2. Ekspektasi `mask_middle("UD-ABCDEFGHIJ")` salah hitung (tail 4 karakter
     = "GHIJ", bukan "HIJ") — kode produksi benar, tes diperbaiki.
  3. Nilai sha256 karangan di tes `auth::hash_is_sha256_hex_deterministic`
     diganti nilai asli (`bbbdb91b…`, dicek via `sha256sum`).
  4. Nama fungsi tes menabrak fungsi produksi (`usd_to_idr`, `escape_html`,
     `tg_id_from_buyer_ref`, `refund_wallet_for_order`) sehingga memanggil
     dirinya sendiri — di-rename.
- **Insiden operasional:** sekali `pkill -f` membunuh shell sendiri (pola
  yang sama seperti pelajaran di `AGENTS.md` — pakai PID, bukan pola);
  sekali `touch src/lib.rs` tidak sengaja saat cek warning — file langsung
  dihapus, suite tetap 180 lolos setelahnya.

## Catatan / deviasi yang ditemukan saat menulis tes

- `qris.rs` (tes lama kelompok C) memakai `test_db()` dengan skema parsial
  (hanya tabel `qris_payments`) — dibiarkan apa adanya (tes lama, bukan
  buatan F); tes baru F selalu memakai `db::open(":memory:")` + schema penuh.
- `db::open(":memory:")` berfungsi untuk tes (WAL jadi no-op di memori).
- `reply_to_ticket` / `close_ticket` menyimpan pesan ke DB walau kirim
  Telegram gagal, tapi tetap melakukan HTTP sungguhan ke Bot API — tidak
  dites (butuh mock transport; kandidat: `send_telegram_html` pakai client
  injeksi — kerja integrasi).
- `get_transaction_status` (Midtrans) memakai `reqwest::Client` langsung
  tanpa trait transport — `tick_midtrans_payments` utuh tidak bisa dites
  tanpa jaringan; hanya `process_midtrans_result` yang ter-cover.
- Route `/api/orders` membangun adapter Sondev dari `Config` di dalam
  handler — S28 (saldo supplier kurang → 400; fail-open) hanya ter-cover
  di level `check_supplier_funds`, bukan di level HTTP.

## Sisa yang belum ter-cover

- Alur bot teloxide end-to-end (S1 sebagian, S3/S4/S18/S20/S29/S32/S33
  sebagian): butuh harness mock Bot API ala `harness.ts` (stub `Bot` /
  transport HTTP teloxide) — belum ada di crate.
- `notify_buyer` / `notify_admin` / `notify_channel_order` pengiriman
  (hanya early-return + mask yang dites).
- `reply_to_ticket` / `close_ticket` pengiriman Telegram.
- `tick_midtrans_payments` utuh (butuh transport injeksi di midtrans.rs).
- `create_snap_transaction` (HTTP Midtrans live).
- Route `/api/orders` S28 di level HTTP.
- `verify_otp` saat refresh token ditolak / `get_merchant_profile` gagal
  (sebagian kecil jalur error GoPay).
