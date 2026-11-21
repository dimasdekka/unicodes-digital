# Kelompok E — Web (axum + maud) — Status

Terakhir diperbarui: 2026-11-21 ~16:35 WIB. Port penuh web layer Unicodes Digital
(TS `~/workspace/reseller-web`) ke Rust. Stack: axum 0.8 + maud 0.27 (askama
DIBUANG koordinator 16:08 WIB — askama_axum 0.5 deprecated, askama 0.14 tanpa
fitur axum). Halaman dikembalikan via `axum::response::Html`.

## Modul selesai — `src/web/`

| File | Isi |
|---|---|
| `mod.rs` | `AppState { cfg, db, http: reqwest::Client, settlement: Arc<WebSettlement>, gopay: Arc<GopayClient> }`, `serve()` (bind `0.0.0.0:{port}`), semua route, + `#[cfg(test)] mod tests` berisi smoke test `web_smoke`. |
| `auth.rs` | Port `admin-auth.ts` + `admin-api.ts`: cookie `unicodes_admin` = sha256(password), httpOnly, 7 hari, fails closed, password dari env `ADMIN_PASSWORD`. |
| `settlement.rs` | `WebSettlement` — implementasi nyata `PaymentSettlement` (kelompok C) untuk web: `credit_topup` via `wallet::mark_topup_paid`, `mark_order_paid` via klaim atomik `UPDATE ... WHERE status='awaiting_payment'`, `spawn_fulfill_and_notify` (tokio::spawn → `fulfill_order` + `notify_buyer`). |
| `util.rs` | `url_decode`/`url_encode`, `ParsedBody::{Json, Form}` + `parse_body` (JSON-atau-form sesuai Content-Type, meniru route TS yang menerima keduanya), `wib_datetime`/`wib_date` (offset +7 jam manual, tanpa chrono-tz). |
| `views.rs` | Semua template maud: `layout`, `product_json`, `storefront` + `product_card` + `BUY_JS`, `tg_page` + `TG_JS`, `admin_nav`, `admin_login`, `admin_dashboard`, `qris_panel` + `QRIS_PANEL_JS`, `tickets_list`, `ticket_detail`, `tickets_login_prompt`, `ticket_not_found`. |
| `pages.rs` | Handler halaman: storefront (`/`), Mini App (`/tg`), admin (`/admin`), daftar tiket, detail tiket. |
| `api.rs` | Semua JSON API + webhook (lihat bawah). |

## Route yang di-port

- `GET /` — storefront katalog (filter `?cat=`, default "Semua").
- `GET /tg` — Mini App Telegram (initData, katalog, checkout, polling status).
- `GET /admin` — dashboard (total user, total order, terkirim, revenue, tiket open) + saldo supplier + panel QRIS + produk & harga retail + daftar pengguna + order terbaru. Login view bila belum auth.
- `GET /admin/tickets`, `GET /admin/tickets/{id}` — daftar (open dulu) + detail thread.
- `GET /api/products` — katalog JSON (3 produk).
- `POST /api/orders` — buat order; qty default 1, qty > 30 → 400 `"Jumlah maksimal 30 per order."`; validasi initData → buyerRef `tg:{id}`; cek dana supplier (fail-open); order gratis → langsung `mark_paid` + fulfill di background; berbayar → Snap Midtrans (30 menit expiry).
- `GET /api/orders/{id}` — detail order (404 bila tidak ada).
- `POST /api/orders/{id}/fulfill` — fulfill manual (TIDAK dilindungi auth, sama seperti TS — ada TODO PRODUKSI di kodenya).
- `POST /api/webhooks/midtrans` — verifikasi SHA512, lalu SATU fungsi transisi `processMidtransResult` (kelompok C) dengan `WebSettlement`; selalu 200 `{ok:true}` bila sukses.
- `POST /api/webhooks/sondev` — verifikasi HMAC-SHA256 (`timestamp + "\n" + raw body`, ±300 detik, prefix `sha256=` opsional), dedupe via `record_webhook_delivery("sondev", X-Sondev-Delivery)` → `{ok:true, duplicate:true}`; terminal completed → `mark_order_delivered` (items + `code` digabung, URL pertama jadi `result_link`); failed/REFUNDED/PARTIAL_DELIVERED → `mark_order_failed`.
- `POST /api/admin/login` (Form) — salah → redirect `/admin?error=1`; benar → Set-Cookie + redirect `/admin`; `ADMIN_PASSWORD` kosong → 500 JSON.
- `GET /api/admin/logout` — hapus cookie + redirect.
- `GET /api/admin/tickets`, `GET /api/admin/tickets/{id}` — list (limit clamp 1..200, default 50) + detail.
- `POST /api/admin/tickets/{id}/reply|close|reopen` — balas (kegagalan Telegram → 502 JSON, balasan tetap tersimpan), tutup/buka lagi.
- `POST /api/admin/products/{id}/price` — ubah harga retail → `product_price_overrides`, berlaku langsung; harga 500..10.000.000; 404 bila produk tidak dikenal.
- `GET+POST /api/admin/payments/qris` — status QRIS + simpan static QRIS.
- `GET /api/admin/payments/qris/payments`, `GET /api/admin/payments/qris/test` — daftar pembayaran + test QR (PNG base64, `ADMIN-TEST-{millis}`).
- `GET /api/admin/payments/gopay/session`, `POST .../otp|verify|poll`, `GET .../merchant`, `POST .../logout` — panel GoPay, port apa adanya (jalur DORMANT, OTP diblokir server Gojek — TIDAK diperbaiki).

Semua route `/api/admin/*` (kecuali login/logout) mengembalikan 401 tanpa cookie admin yang valid.

## Keputusan perilaku (bila TS ambigu / sengaja menyimpang)

- **Perbandingan cookie admin constant-time** (bukan `===` seperti TS) — penyimpangan sadar, lebih aman.
- **Web checkout MASIH urutan lama qty → target → bayar** (TS `tg/page.tsx` + `buy-button.tsx`), berbeda dari bot (kode dulu baru jumlah). Dicatat sebagai gap, TIDAK disamakan diam-diam.
- **Redirect login/logout memakai 303** (`Redirect::to` axum), bukan 307 seperti TS — perilaku browser untuk POST→GET sama, lebih idiomatik di axum.
- **`?error=1` pada reply-form gagal**: TS mengeset pesan error tapi halaman detail tidak pernah membacanya — dipertahankan apa adanya (tidak ditambah tampilan error).
- **Tema Scaramouche**: bg `#14092b`, kartu `#221244`, aksen violet `#7c3aed`, teks lavender `#ede9fe`, harga emas `#e8c874`. Halaman buyer = KATALOG SAJA (tanpa nama supplier/saldo/deposit).
- **Maud meng-escape `&` menjadi `&amp;`** — teks seperti "Produk & harga retail" muncul sebagai `&amp;` di HTML (diperhitungkan di smoke test).

## Bug yang ditemukan & diperbaiki saat smoke test

1. **axum 0.8 menolak sintaks route `:id`** — panic saat startup: `Path segments must not start with ':'`. Semua route diubah ke `{id}` (`/admin/tickets/{id}`, `/api/orders/{id}`, `/api/orders/{id}/fulfill`, `/api/admin/tickets/{id}`, `.../reply|close|reopen`, `/api/admin/products/{id}/price`). Extractor `Path<String>` tidak berubah.
2. **Smoke test via binary penuh TIDAK BISA dengan token dummy**: `main.rs` memakai `tokio::select` atas bot toko + bot news + web — bot news panic `InvalidToken` (token dummy) dan seluruh proses ikut mati, web ikut mati. Karena itu smoke test ditulis sebagai **unit test di `src/web/mod.rs`** (`#[tokio::test] web_smoke`): menjalankan `serve()` saja di port 3223 dengan DB `/tmp/web-smoke-test.db` (dihapus sebelum/sesudah), lalu cek via reqwest.
3. **reqwest mengikuti redirect secara default** — `Set-Cookie` pada respons 303 login hilang dari respons akhir; test memakai client kedua dengan `redirect::Policy::none()` untuk langkah login.
4. **JANGAN `pkill -f` dengan string yang juga ada di command line sendiri** (pelajaran lama dari AGENTS.md, terulang di sesi ini): `pkill -f "target/debug/unicodes-digital"` membunuh shell exec sendiri (SIGTERM) karena pola cocok dengan command line-nya. Gunakan PID eksplisit.

## Verifikasi

- `cargo check` **LOLOS** (2026-11-21 ~16:35 WIB): `Finished dev profile`, 61 warnings (dead code/unused imports — semuanya dari modul kelompok lain, **nol dari `src/web/`**), plus warning future-incompat `proc-macro-error2 v2.0.1` yang sudah dikenal.
- `cargo test --bin unicodes-digital web_smoke` **LOLOS** (13,5 detik). Yang dicek:
  - `GET /` 200: ada "Muse AI Token 1B", "Rp2.000", badge "Segera Hadir" (Gemini), TIDAK ada kata "Sondev".
  - `GET /api/products`: `ok:true`, 3 produk, `muse-token-1b` harga 2000.
  - `GET /tg` 200, memuat `telegram-web-app.js`.
  - `GET /admin` tanpa login: form login ("Password admin").
  - `GET /api/admin/tickets` tanpa cookie → 401.
  - Login salah → 303 `Location: /admin?error=1`; login benar (`test123`) → 303 + `Set-Cookie: unicodes_admin=...; HttpOnly`.
  - Dengan cookie: dashboard berisi "Total pengguna", "Saldo supplier", "harga retail"; `GET /api/admin/tickets` → `ok:true`, 0 tiket.
  - Tiket tidak ada → API 404 + halaman "Tiket tidak ditemukan".
  - `POST /api/orders` produk salah → 400; qty 31 → 400 `"Jumlah maksimal 30 per order."`; `GET /api/orders/UD-TIDAKADA` → 404.
  - Webhook Midtrans tanpa key → 500; webhook Sondev tanpa secret → 500.
  - QRIS status → `configured:false`; GoPay session → `ok:true, authenticated:false`.
- **Belum dites end-to-end dengan Midtrans/Sondev sungguhan** (butuh key + saldo; sama seperti batasan TS). Fulfill sungguhan, Snap, dan webhook dengan signature valid belum dieksekusi di runtime.

## Perbaikan pada file kelompok lain (minimal — wajib dilaporkan ke koordinator)

- **E0597 borrow (7 titik, fix sesuai saran compiler: bind `match stmt.query_map(...)` ke `let rows = ...; rows`)**: `lib/orders.rs` (2: `list_store_orders`, `list_orders_by_buyer`), `lib/support_tickets.rs` (3: `list_tickets`, `list_tickets_by_user`, `get_ticket_messages`), `lib/wallet.rs` (2: `list_promo_codes`, `top_referrers`).
- **`src/web/supplier_port.rs` dibuat lalu DIHAPUS**: bentrok E0119 dengan bridge identik kelompok bot di `src/bot/deps.rs:50` (`impl SupplierPort for AnySupplier`). Bridge bot sekarang kanonik; web memakainya via `check_supplier_funds`/`fulfill_order`.
- **Delegasi Xiaomi/SheerID ditambahkan ke bridge bot di `src/bot/deps.rs`** (`create_xiaomi_order`, `get_xiaomi_order`, `create_sheerid_verification`, `get_sheerid_job` via `as_lahastore()`, default `LahaCurrency::Usd`, `discount_code: None`). Alasan: `fulfill_order` (orders.rs) memanggil method port itu dan default trait selalu Err — fulfillment Gemini Xiaomi/SheerID akan gagal untuk bot DAN web. Produk masih `coming_soon`, jadi laten.

## Daftar kebutuhan / temuan integrasi (untuk koordinator)

- **Sudah terpenuhi saat kerja**: semua signature yang dibutuhkan dari A/B/C sudah ada saat web ditulis (catalog, telegram_webapp, support_tickets, product_prices, suppliers/AnySupplier, midtrans + poller, qris + qris_card + qris_settings, gopay, orders, wallet, notify, i18n). Tidak ada `todo!()` tersisa di `src/web/`.
- **Duplikasi `SupplierPort` (orders.rs) vs `SupplierAdapter` (suppliers/types.rs)**: keduanya mendefinisikan ulang tipe yang sama (`SupplierBalance`, `SupplierProduct`, `SupplierOrderResult`, `CreateOrderInput`, `SupplierOrderStatus`) sebagai tipe berbeda — satu-satunya perbedaan nyata: `wholesale_price_idr` adalah `Option<i64>` di orders.rs vs `Option<f64>` di types.rs. Bridge di `bot/deps.rs` mengonversi field-per-field. Kelompok A/B sebaiknya menyatukan ke satu definisi kanonik.
- **Duplikasi settlement**: kelompok bot punya `BotSettlement` (`src/bot/settlement.rs`, `Arc<BotState>`), web punya `WebSettlement` (`src/web/settlement.rs`) — dua implementasi `PaymentSettlement`, satu per peran proses. Bisa disatukan nanti, bukan blocker.
- **Tidak ada perubahan** pada `main.rs`, `config.rs`, `Cargo.toml` oleh kelompok E.
- **Catatan**: satu prompt approval tool sempat timeout di tengah sesi (status `timed_out`) — tidak ada aksi yang terblokir; output perintah tetap diterima dan pekerjaan berlanjut.
