# Kelompok A — Inti (porting Rust) — status 2026-11-21 ~16:40 WIB

Semua modul kelompok A selesai diport dari TS. `cargo check` LOLOS
(0 error; 437 warning dead-code — wajar, bot/web belum memanggil
fungsi-fungsi ini; 4 warning unused-dependency manifest: axum, maud,
regex, teloxide — dipakai kelompok lain nanti).

## Modul selesai

| File Rust | Sumber TS | Isi |
|---|---|---|
| `src/db.rs` | `src/lib/db.ts` (+ support-tickets, product-prices) | `open()` (WAL + busy_timeout=5000, sudah ada), `ensure_tables()` — SEMUA tabel: store_orders (+ migrasi kolom target/discount_idr/promo_code/paid_via via PRAGMA read-only), webhook_deliveries, users, wallets, wallet_txns, wallet_topups, promo_codes, promo_redemptions, referrals, qris_payments, settings, channel_posts, midtrans_payments, support_tickets, support_messages, product_price_overrides. Plus `now_iso()`, `record_webhook_delivery()`, `get_setting()`/`set_setting()`. |
| `src/catalog.rs` | `src/lib/catalog.ts` | `CATALOG` (3 produk; Muse 1B Rp2.000), `CatalogProduct`, `Fulfillment`, `StockState`, `get_catalog_product(db, id)`, `list_active_products(db)`, `list_categories(db)`, `list_active_products_by_category(db, cat)`, `list_active_products_static()`. Harga efektif pakai override admin. |
| `src/i18n.rs` | `src/bot/i18n.ts` | `Strings` (semua field ID+EN, teks disalin persis), `t(lang) -> &'static Strings`, `STR_ID`/`STR_EN`, `rupiah(n)` (pemisah "."), `wib_time(iso)` (HH:mm WIB), param struct `WelcomeParams`, `ProductDetailParams`, `QtyTitleParams`, `PayTitleParams`, `TxnLineParams`, `OrderDetailParams`. |
| `src/lib/pricing.rs` | `src/lib/pricing.ts` | `VND_PER_USD=26500`, `IDR_PER_USD=16500`, `vnd_to_idr`, `usd_to_idr`, `retail_from_wholesale`, `retail_price(wholesale, pct, flat)` (ceil ke 500), `retail_price_default` (25% + Rp1.000). |
| `src/lib/wallet.rs` | `src/lib/wallet.ts` | `Lang`, `BotUser`, `Wallet`, `WalletTxn`, `WalletTopup`, `PromoCode`, `ReferralStats`, `with_tx` (BEGIN IMMEDIATE), `ensure_user`, `get_user`, `set_user_lang`, `get_wallet`, `credit_wallet`, `debit_wallet` (None kalau saldo kurang), `list_wallet_txns`, `create_topup`/`get_topup`/`mark_topup_paid` (idempotent), promo: `create_promo_code`, `list_promo_codes`, `get_promo_code`, `has_redeemed_promo`, `validate_promo` (reason not_found/expired/exhausted/already_used), `calc_promo_discount`, `redeem_promo`, referral: `referral_code`/`referral_link`/`record_referral`/`get_referral_stats`/`top_referrers`/`maybe_reward_referrer` (Rp1.500, sekali, order pertama berbayar), `refund_wallet_for_order`, `tg_id_from_buyer_ref`, `base36`. |
| `src/lib/orders.rs` | `src/lib/orders.ts` | `StoreOrder`, `StoreOrderStatus`, `create_store_order`, `new_order_id`, `get_store_order`, `list_store_orders`, `list_orders_by_buyer`, `get_buyer_stats`, `count_code_uses` (kode polos ATAU `.../invite/KODE`, escape LIKE), `get_store_stats`, `set_order_target`, `set_order_paid_via`, `mark_paid`, `cancel_order_for_expired_payment`, `mark_order_delivered`, `mark_order_failed`, `check_supplier_funds` (FAIL OPEN), `fulfill_order` (klaim atomik paid→processing, ladder Sondev 5/10/20/30 dtk max 10 mnt, Xiaomi/SheerID poll 8 dtk max 5 mnt, refund dompet otomatis / notifikasi refund manual QRIS). Konstanta `MAX_QTY=30`, `MAX_CODE_USES=30`. |
| `src/lib/session.rs` | `src/bot/session.ts` | `CheckoutDraft`, `AwaitingInput` (code/target/custom_qty/promo/promo_check/custom_topup/ticket), `Session`, `SessionStore` (Mutex<HashMap>, `get`/`set`/`update`/`clear`/`cancel_awaiting`). |
| `src/lib/notify.rs` | `src/lib/notify.ts` | `NotifyCtx` (+ `from_config(&Config)`), `send_telegram_html` (+ `_with_markup`), `mask_middle`, `notify_channel_order` (hanya delivered, dedupe channel_posts, buyer+ID disamarkan, TIDAK PERNAH item hasil/target), `notify_buyer`, `notify_admin`. |
| `src/lib/support_tickets.rs` | `src/lib/support-tickets.ts` | `TicketStatus`, `SupportTicket`, `SupportMessage`, `get_open_ticket_by_user`, `create_ticket` (ID `TKT-XXXXXX`, alfabet tanpa 0/O/1/I, retry 5x), `add_user_message`, `list_tickets`, `list_tickets_by_user`, `get_ticket`, `get_ticket_messages`, `count_open_tickets`, `set_ticket_status`, `escape_html`, `reply_to_ticket` (async, simpan walau kirim gagal), `close_ticket`, `reopen_ticket`. |
| `src/lib/product_prices.rs` | `src/lib/product-prices.ts` | `get_price_override`, `set_price_override`, `clear_price_override`, `get_effective_retail_price(db, product_id, base)`, `list_price_overrides`. Tabel dibuat di `db::ensure_tables` (bukan lazy). |
| `src/lib/telegram_webapp.rs` | `src/lib/telegram-webapp.ts` | `TelegramWebAppUser`, `InitDataValidation` (reason bad_init_data/missing_hash/missing_auth_date/expired/bad_hash/bad_user), `validate_init_data(init_data, bot_token)` (HMAC-SHA256 WebAppData, auth_date ≤ 86400 dtk, form-decode sendiri). |

## Keputusan perilaku (TS ambigu / adaptasi Rust)

- **Tidak ada singleton DB global** seperti `getDb()` TS. Semua fungsi
  publik menerima `&Db` (`Arc<Mutex<Connection>>`) dan mengunci sekali;
  helper internal menerima `&Connection` supaya tidak deadlock (Mutex
  tidak re-entrant). `with_tx(db, f)` = `BEGIN IMMEDIATE … COMMIT/ROLLBACK`.
- **Klaim atomik dipertahankan persis**: `mark_paid`,
  `cancel_order_for_expired_payment`, `fulfill_order` (paid→processing),
  `mark_topup_paid`, `maybe_reward_referrer`, `redeem_promo` semuanya
  pakai `UPDATE … WHERE status=…` / cek-dalam-transaksi.
- **`validate_promo`** mengembalikan enum `PromoValidation` (bukan
  Result) dengan `.reason()` = string TS; `validate_init_data` sama
  polanya (`InitDataValidation::reason()`).
- **Perbandingan hash initData**: TS pakai `timingSafeEqual`; di sini
  perbandingan byte `==` setelah cek panjang (crate `subtle` bukan
  dependensi) — dicatat sebagai penyimpangan kecil, bukan secret.
- **i18n EN `welcome`**: sapaan dihitung 10–18 = "Good afternoon",
  ikon terpisah (☀️ s.d. 15, 🌇 15–18) persis TS.
- **`rupiah`**: format manual `Rp2.000` (tanpa ICU di Rust).
- **`notify`**: TS baca token dari `config.ts` global; Rust pakai
  `NotifyCtx` eksplisit (dibangun sekali dari `Config`).
- **`session`**: TS `Map` global + referensi mutable; Rust
  `SessionStore` (pemiliknya bot) dengan method `get/set/update`.
- **ID order/topup**: `UD-<millis base36>-<6 acak>` uppercase, meniru
  `Date.now().toString(36)` + `Math.random().toString(36)` TS.

## Signature untuk kelompok lain

- **Bot/web (kelompok B/D)**: `db::open(path) -> Db`; `catalog::get_catalog_product(&Db, &str)`;
  `i18n::t(Lang)`; `wallet::{ensure_user, get_wallet, credit_wallet, debit_wallet, create_topup, mark_topup_paid, validate_promo, redeem_promo, record_referral, maybe_reward_referrer, get_referral_stats, referral_code/link, tg_id_from_buyer_ref}`;
  `orders::{create_store_order, get_store_order, list_orders_by_buyer, mark_paid, cancel_order_for_expired_payment, count_code_uses, MAX_QTY, MAX_CODE_USES}`;
  `session::SessionStore::new()`; `support_tickets::{get_open_ticket_by_user, create_ticket, add_user_message, list_tickets_by_user, reply_to_ticket, close_ticket, reopen_ticket}`;
  `notify::NotifyCtx::from_config(&Config)` + `notify_buyer`/`notify_admin`/`notify_channel_order`;
  `telegram_webapp::validate_init_data(&str, &str)`;
  `product_prices::{get/set/clear_price_override}`.
- **Supplier (kelompok B)** — `orders::SupplierPort` (trait, async fn
  bawaan Rust): `get_balance`, `list_products`, `create_order`,
  `get_order`, plus default "tidak didukung" untuk
  `create_xiaomi_order` / `get_xiaomi_order` / `create_sheerid_verification`
  / `get_sheerid_job` (hanya LahaStore yang override).
  `fulfill_order(db, notify: Option<&NotifyCtx>, supplier: &S, order_id)`
  generik atas `S: SupplierPort`. Tipe `SupplierBalance`,
  `SupplierProduct`, `SupplierOrderResult`, `SupplierOrderStatus`,
  `CreateOrderInput`, `SheeridJob` didefinisikan di `orders.rs` sebagai
  cerminan `suppliers/types.ts` — **kelompok B sebaiknya me-re-export
  tipe ini dari `suppliers/types.rs`, bukan mendefinisikan ulang**
  (saat ini `suppliers/types.rs` punya definisi sendiri yang duplikatif:
  `SupplierAdapter` + tipe yang sama namanya — perlu disatukan sebelum
  `fulfill_order` dipakai; adapter Sondev/LahaStore harus mengimplementasikan
  `orders::SupplierPort` atau dibuatkan bridge tipis).
- **Payments (kelompok C)** — `mark_topup_paid` (idempotent, dipakai
  poller QRIS/Midtrans), `mark_paid`, `cancel_order_for_expired_payment`,
  `check_supplier_funds` (fail-open), `db::record_webhook_delivery`,
  `db::{get_setting, set_setting}` (dipakai `qris_settings` dkk).
- **Web admin** — `support_tickets::list_tickets`/`get_ticket_messages`/
  `set_ticket_status`, `product_prices::list_price_overrides`,
  `orders::list_store_orders`, `wallet::list_promo_codes`.

## Yang belum / catatan

- `main.rs` / `config.rs` / `Cargo.toml` TIDAK diubah.
- Tidak ada secret yang ditulis ke file/repo/laporan; `.env.local`
  tidak pernah dibaca/disalin; tes hanya pakai DB temp (tidak ada tes
  yang menyentuh `dev.db` asli — belum ada tes sama sekali untuk modul A).
- **Edit paralel**: kelompok lain bekerja di repo yang sama bersamaan
  (payments/mod.rs, suppliers/*, Cargo.toml berubah saat saya bekerja).
  Dua insiden: (1) stub `gopay.rs`/`qris_poller.rs` yang saya buat
  supaya `cargo check` lolos sudah diganti kelompok C dengan implementasi
  penuh — stub saya tidak lagi relevan; (2) pola `match stmt.query_map`
  di 7 fungsi saya sudah diubah pihak lain menjadi `let rows = match …`
  sebelum saya sempat memperbaikinya sendiri; script perbaikan saya sempat
  merusak `list_wallet_txns` (sisa `out` nyasar) — sudah saya perbaiki
  kembali dan `cargo check` lolos sesudahnya. (3) Saya sempat `touch
  src/lib.rs` kosong saat menghitung warning — sudah dihapus; warning
  "found module declaration for lib.rs" yang muncul sesudahnya hilang
  sendiri pada check berikutnya.
- **E0733 gopay.rs sempat muncul** (rekursi async call→refresh_session→call)
  saat kelompok C menulis file itu; pada check terakhir sudah hilang —
  kelompok C memperbaikinya sendiri. Saya TIDAK mengubah file kelompok C.
- Warning 437 dead-code akan hilang bertahap saat bot/web memanggil
  fungsi-fungsi ini; bukan error.
