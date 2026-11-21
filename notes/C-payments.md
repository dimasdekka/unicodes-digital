# Kelompok C — Payments (status 2026-11-21)

Port penuh `src/lib/payments/` TS → Rust. Kerja hanya di `~/workspace/reseller-rust/`;
`~/workspace/reseller-web`, EC2, dan GitHub TIDAK disentuh.

## Modul selesai

| File Rust | Sumber TS | Status |
|---|---|---|
| `src/lib/payments/midtrans.rs` | `midtrans.ts` | ✅ selesai |
| `src/lib/payments/midtrans_poller.rs` | `midtrans-poller.ts` | ✅ selesai (settlement via trait, lihat bawah) |
| `src/lib/payments/qris.rs` | `qris.ts` | ✅ selesai |
| `src/lib/payments/qris_card.rs` | `qris-card.ts` | ✅ selesai |
| `src/lib/payments/qris_settings.rs` | `qris-settings.ts` | ✅ selesai |
| `src/lib/payments/qris_poller.rs` | `qris-poller.ts` | ✅ selesai (baru; `pub mod qris_poller;` didaftarkan di `mod.rs`) |
| `src/lib/payments/gopay.rs` | `gopay.ts` | ✅ selesai (sekunder, jalur DORMANT) |

`cargo check` lolos (hanya warning `dead_code`/`unused` bawaan skeleton +
fungsi pub yang belum dipanggil bot/web — hilang begitu di-wire).
Tes unit di dalam tiap modul (`cargo test`): cerminan `midtrans.test.ts`,
`qris.test.ts`, `qris-card.test.ts`, `qris-admin.test.ts`, `gopay.test.ts`
(vektor sintetis EMVCo yang sama; CRC diverifikasi vs implementasi bitwise
independen; HTTP di-mock lewat trait transport).

**Hasil tes (2026-11-21 ~09:25 WIB): `cargo test --bin unicodes-digital payments`
→ 26 passed, 0 failed. Full crate: 39/40 — 1 gagal di
`bot::extract::tests::share_text_needs_invite_context` (milik kelompok bot,
bukan payments).** Selama pengerjaan, tree sempat merah sesaat karena
kelompok lain mengedit bersamaan (konflik impl `SupplierPort` bot-vs-web,
`supplier_port.rs` hilang sesaat); saat laporan ini ditulis `cargo check`
sudah bersih lagi. Tes payments sempat diverifikasi di scratch crate
terpisah (`~/workspace/.scratch-payments-check`, sudah dihapus) saat tree
utama masih churn — hasilnya sama: 26/26.

## Fungsi penting yang di-port

- **midtrans.rs**: `create_snap_transaction` (Snap QRIS-only,
  `enabled_payments: ["qris"]`, expiry 30 mnt, Basic auth `server_key:`),
  `get_transaction_status`, `map_midtrans_status`,
  `verify_notification_signature` (SHA512 + constant-time compare),
  store `midtrans_payments`: create/get/list pending/list expired/
  mark paid/failed/expire (klaim atomik `UPDATE ... WHERE status='pending'`).
- **midtrans_poller.rs**: `process_midtrans_result` — SATU fungsi transisi
  tunggal dipakai poller DAN webhook; `tick_midtrans_payments` (30 dtk);
  `run_midtrans_poller_loop`.
- **qris.rs**: `crc16_ccitt_false` (table-driven), `parse_qris_payload`,
  `serialize_qris_payload`, `is_valid_qris_checksum`, `build_dynamic_qris`,
  `render_qris_png`, `allocate_unique_amount`, store `qris_payments`
  (`create_qris_payment` — alokasi + insert dalam `BEGIN IMMEDIATE`),
  `match_feed_transaction` / `find_qris_payment_for_transaction` /
  `reconcile_feed_transactions`.
- **qris_card.rs**: `compose_qris_card` (+ varian `compose_qris_card_from_path`
  untuk tes), `render_qris_card_or_plain`; konstanta bbox QR
  x 359..885, y 502..1027 pada 1252×1252.
- **qris_settings.rs**: `get_gopay_static_qris` / `set_gopay_static_qris`,
  `extract_merchant_info`, `validate_static_qris`, `list_recent_qris_payments`.
  Juga `get_setting` / `set_setting` (port `db.ts`, sementara di sini).
- **qris_poller.rs**: `tick_qris_payments`, `run_qris_poller_loop`.
- **gopay.rs**: `GopayClient` (`request_otp`, `verify_otp`,
  `refresh_session`, `ensure_fresh_session`, `get_merchant_profile`,
  `list_feed_transactions` + paginasi), `normalize_phone`,
  `GopayError` + kode (`auth_required` dst), `is_auth_required`.

## Keputusan perilaku (TS ambigu / perlu dicatat)

1. **Midtrans 404 = pending** — `get_transaction_status` memetakan HTTP 404
   maupun `status_code` "404"/404 di body (HTTP 200) menjadi
   `transaction_status: "pending"`. TIDAK PERNAH ditandai FAILED oleh 404;
   expiry sweep 30 menit yang menutupnya.
2. **Quirk TS yang di-port apa adanya — `markOrderFailed` no-op dari
   `awaiting_payment`**: di TS, `markOrderFailed` hanya mengubah order dari
   status `processing`/`paid`; poller Midtrans memanggilnya untuk order
   `awaiting_payment` dan hasilnya tidak berubah. Jadi pembayaran Midtrans
   yang `deny/cancel/expire` hanya menandai baris `midtrans_payments` jadi
   `failed`; order baru benar-benar gagal saat expiry sweep membatalkannya.
   Trait `PaymentSettlement::mark_order_failed` WAJIB meniru semantik ini.
3. **Satu transisi tunggal**: `process_midtrans_result` dipakai poller dan
   webhook (route webhook tinggal memanggilnya setelah verifikasi signature).
4. **QRIS `render_qris_png`**: TS `margin: 2`; crate `qrcode` memakai quiet
   zone standar 4 modul. QR tetap valid/terbaca; geometri piksel tidak
   dijamin identik dengan TS.
5. **QRIS card**: TS menulis encoder PNG sendiri; Rust memakai crate `image`
   (PNG lossless juga). Template dibaca dari `src/bot/assets/qris-standee.jpg`
   atau env `QRIS_STANDEE_PATH`; template hilang/rusak/ukuran beda →
   `compose_qris_card` = None, fallback QR polos (tidak pernah crash).
6. **GoPay**: jalur DORMANT, port apa adanya — OTP TIDAK "diperbaiki".
   Endpoint GoID/GoBiz tetap hipotesis paygateme (belum terverifikasi live).
   Refresh 401: retry sekali dengan body yang sama. `call` di-refactor jadi
   loop 2-attempt + `call_once` (bukan rekursi) karena async fn rekursif
   ditolak rustc (E0733); `refresh_session` memakai `call_once` langsung.
7. **Waktu**: semua tabel menyimpan ISO 8601 UTC milidetik + `Z` seperti
   `toISOString()` TS (`now_iso` / `iso_of` / `parse_iso` di `payments/mod.rs`).
8. **Tanda tangan fungsi DB**: semua fungsi store menerima `&Db`
   (`Arc<Mutex<Connection>>`) sebagai argumen pertama — tidak ada singleton
   global seperti `getDb()` TS.
9. **HTTP**: fungsi Midtrans menerima `&reqwest::Client`; GoPay memakai
   trait transport `GopayHttp` (default `ReqwestGopayHttp`) supaya bisa
   di-mock di tes — cerminan `fetchImpl` TS.

## Signature yang disediakan untuk kelompok lain

### `PaymentSettlement` — `midtrans_poller.rs` (dipakai kedua poller)
```rust
pub trait PaymentSettlement: Send + Sync {
    fn credit_topup(&self, db: &Db, topup_id: &str) -> anyhow::Result<bool>;
    // Ok(true) = saldo masuk; Ok(false) = sudah paid; Err = top-up tak ada.
    fn mark_order_paid(&self, db: &Db, order_id: &str, paid_via: &str) -> anyhow::Result<bool>;
    // port setOrderPaidVia + markPaid; klaim atomik; Ok(false) = sudah bukan awaiting_payment.
    fn mark_order_failed(&self, db: &Db, order_id: &str, reason: &str) -> anyhow::Result<()>;
    // port markOrderFailed — HANYA dari processing/paid (lihat quirk #2).
    fn cancel_order_expired(&self, db: &Db, order_id: &str, reason: &str) -> anyhow::Result<()>;
    // port cancelOrderForExpiredPayment — hanya dari awaiting_payment.
    fn order_status(&self, db: &Db, order_id: &str) -> anyhow::Result<Option<String>>;
    fn order_buyer_tg_id(&self, db: &Db, order_id: &str) -> anyhow::Result<Option<i64>>;
    // port tgIdFromBuyerRef (buyer_ref "tg:<id>").
    fn maybe_reward_referrer(&self, db: &Db, referred_tg_id: i64) -> anyhow::Result<bool>;
    fn spawn_fulfill_and_notify(&self, order_id: String);
    // fulfillOrder + notifyBuyer di background; JANGAN tahan tick.
}
```
- `UnimplementedSettlement` (di file yang sama): implementasi `todo!()`
  sementara supaya `cargo check` lolos. **Wajib diganti implementasi nyata
  (kelompok orders/wallet/notify) sebelum bot live** — tick akan panic saat
  pembayaran lunas kalau masih memakai ini.
- `process_midtrans_result(db, settlement, order_id, tx_status)` — dipakai
  poller DAN route webhook Midtrans (`POST /api/webhooks/midtrans`, kelompok web).
- `tick_midtrans_payments(client, cfg, db, settlement)`,
  `run_midtrans_poller_loop(client, cfg, db, settlement)` (interval 30 dtk).
- `tick_qris_payments(db, client: &GopayClient, settlement)`,
  `run_qris_poller_loop(db, client, settlement)` (interval 30 dtk).
- `GopayClient::new(db, cfg)` — dipakai poller QRIS + route admin GoPay.
- `create_snap_transaction(client, cfg, order_id, amount_idr, item_name)`,
  `create_midtrans_payment(db, ...)`, `get_midtrans_payment(db, order_id)` —
  dipakai pembuat order/top-up (kelompok bot/web).
- `create_qris_payment(db, reference, amount_idr, static_payload, now)` →
  `{ payment, qr_string }`; `render_qris_card_or_plain(qr_string)` →
  `{ png, composited }` — dipakai pembuat QR order/top-up.
- `get_gopay_static_qris(db)` — dipakai `GopayClient::get_merchant_profile`
  dan route admin QRIS; `validate_static_qris(payload)` — dipakai route admin
  saat Dim menempel string QRIS statis (kembalikan `QrisMerchantInfo`, JANGAN
  echo payload lengkap ke client).

## Yang belum / butuh kelompok lain

- **Implementasi nyata `PaymentSettlement`** (orders/wallet/notify) — blocker
  sebelum pembayaran benar-benar menyelesaikan order di Rust.
- **`ensure_tables` di `db.rs` masih no-op** (koordinator): tabel
  `midtrans_payments`, `qris_payments`, `settings` dll belum dibuat oleh
  `db::open`. Modul payments mengasumsikan tabel sudah ada (skema IDENTIK
  `db.ts`); tes unit membuat tabelnya sendiri di DB in-memory.
- **`get_setting`/`set_setting`** masih didefinisikan di `qris_settings.rs`
  (TODO di file) — pindah ke `crate::db` begitu kelompok DB mem-portnya.
- **Route webhook Midtrans + route admin** (kelompok web): tinggal panggil
  `verify_notification_signature` + `process_midtrans_result` /
  `tick_qris_payments`.
- **Bot/web belum memanggil poller loop** (`run_midtrans_poller_loop` /
  `run_qris_poller_loop`) — `main.rs` tidak diubah.

## Perubahan di luar `src/lib/payments/` (dicatat sesuai kontrak)

- `src/config.rs`: tambah field `gobiz_phone: String` (env `GOBIZ_PHONE`,
  default `""`) — dibutuhkan `GopayClient::request_otp` sebagai nomor default,
  sama seperti `gobizPhone()` di `config.ts`. Tidak ada secret yang ditulis.
- `Cargo.toml`, `main.rs`: TIDAK diubah.
- Tidak ada secret yang ditulis ke file/repo/laporan mana pun; `.env.local`
  tidak pernah dibaca/disalin.
