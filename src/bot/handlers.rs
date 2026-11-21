//! Bot toko — port `src/bot/bot.ts` (1969 baris) ke teloxide 0.17.
//!
//! Alur SAMA dengan TS: /start -> menu -> katalog -> detail -> kode invite
//! -> jumlah -> promo (opsional) -> bayar (saldo / QRIS Midtrans / QRIS
//! GoBiz fallback) -> fulfill -> hasil dikirim ke chat pembeli (notify).
//! Sesi checkout in-memory (`crate::lib::session::SessionStore`);
//! restart = draft hilang, order di DB aman.
//!
//! Perbedaan dari TS yang disengaja:
//! - `fixedFetch` TIDAK di-port (workaround Node 24 + grammY; teloxide
//!   memakai reqwest native).
//! - Paralel antar-chat + sekuensial dalam satu chat = distribution
//!   function default Dispatcher teloxide (chat id) — lihat `mod.rs`.
//! - QR foto: `render_qris_png` polos (kelompok C belum menulis komposit
//!   kartu standee di `qris_card.rs`).

use std::sync::Arc;
use std::time::{Duration, Instant};

use teloxide::prelude::*;
use teloxide::types::{
    ChatId, ChatMemberStatus, FileId, InlineKeyboardButton, InlineKeyboardMarkup,
    InputFile, InputMedia, InputMediaPhoto, MaybeInaccessibleMessage, MessageId,
    ParseMode, Recipient, User, WebAppInfo,
};

use crate::bot::deps;
use crate::bot::extract::extract_invite_code;
use crate::bot::menu_button::get_web_app_url;
use crate::bot::settlement::BotSettlement;
use crate::bot::BotState;
use crate::catalog::{self, StockState, SUPPLIER_SONDEV};
use crate::i18n::{
    t, OrderDetailParams, PayTitleParams, ProductDetailParams, QtyTitleParams, Strings,
    TxnLineParams, WelcomeParams,
};
use crate::lib::notify::notify_buyer;
use crate::lib::orders::{
    count_code_uses, create_store_order, fulfill_order, get_buyer_stats,
    get_store_order, get_store_stats, list_orders_by_buyer, mark_paid, new_order_id,
    set_order_paid_via, CreateStoreOrderInput, StoreOrder, StoreOrderStatus,
    MAX_CODE_USES, MAX_QTY,
};
use crate::lib::payments::midtrans::{
    create_midtrans_payment, create_snap_transaction, get_midtrans_payment,
    midtrans_configured,
};
use crate::lib::payments::midtrans_poller::tick_midtrans_payments;
use crate::lib::payments::qris::{
    create_qris_payment, get_qris_payment_by_reference,
};
use crate::lib::payments::qris_card::render_qris_card_or_plain;
use crate::lib::payments::qris_poller::{tick_qris_payments, QrisPollCode};
use crate::lib::payments::qris_settings::get_gopay_static_qris;
use crate::lib::session::{AwaitingInput, CheckoutDraft, Session};
use crate::lib::support_tickets::{
    add_user_message, create_ticket, get_open_ticket_by_user, list_tickets_by_user,
};
use crate::lib::suppliers::{get_supplier, SupplierAdapter, SupplierId};
use crate::lib::wallet::{
    calc_promo_discount, create_promo_code, create_topup, debit_wallet,
    ensure_user, get_referral_stats, get_topup, get_user, get_wallet, list_promo_codes,
    list_wallet_txns, maybe_reward_referrer, record_referral, referral_code,
    referral_link, redeem_promo, set_user_lang, top_referrers, validate_promo,
    CreatePromoInput, EnsureUserInput, Lang, PromoValidation, MIN_TOPUP_IDR,
    TOPUP_AMOUNTS,
};

pub const QTY_PACKS: [i64; 5] = [1, 3, 5, 10, 30];
/// Throttle tombol "Saya Sudah Bayar" per referensi.
const MANUAL_CHECK_MIN: Duration = Duration::from_secs(10);
/// Cache hasil cek wajib-join channel.
const CHANNEL_JOIN_CACHE: Duration = Duration::from_secs(10 * 60);

pub const BOT_COMMANDS: [(&str, &str); 5] = [
    ("start", "Menu utama toko"),
    ("products", "Lihat katalog produk"),
    ("order", "Lacak pesanan"),
    ("support", "Buka tiket support"),
    ("cancel", "Batalkan aksi"),
];

// ---------------------------------------------------------------------------
// Konteks satu update.
// ---------------------------------------------------------------------------

/// Satu update (pesan / callback) + semua yang dibutuhkan handler.
/// `msg_id` terisi hanya untuk callback query (untuk edit pesan); pesan
/// biasa selalu dibalas baru.
pub struct Ctx {
    pub bot: Bot,
    pub state: Arc<BotState>,
    pub chat_id: ChatId,
    pub user: User,
    pub msg_id: Option<MessageId>,
    pub is_photo: bool,
    pub is_callback: bool,
}

impl Ctx {
    pub fn uid(&self) -> i64 {
        self.user.id.0 as i64
    }

    pub fn lang(&self) -> Lang {
        get_user(&self.state.db, self.uid())
            .map(|u| u.lang)
            .unwrap_or(Lang::Id)
    }

    pub fn strs(&self) -> &'static Strings {
        t(self.lang())
    }

    pub fn is_admin(&self) -> bool {
        self.state.cfg.telegram_admin_ids.contains(&self.uid())
    }

    pub fn session(&self) -> Session {
        self.state.sessions.get(self.uid())
    }

    pub fn update_session(&self, f: impl FnOnce(&mut Session)) {
        self.state.sessions.update(self.uid(), f);
    }

    /// Tampilkan layar: edit pesan (kalau dari tombol) atau kirim baru.
    /// Pesan FOTO tidak bisa diedit teksnya pakai editMessageText —
    /// pakai editMessageCaption. Kalau edit gagal (terlalu lama / isi
    /// sama), kirim pesan baru — sama seperti TS.
    pub async fn render(&self, text: &str, kb: Option<InlineKeyboardMarkup>) {
        if let Some(msg_id) = self.msg_id {
            let ok = if self.is_photo {
                let mut req = self
                    .bot
                    .edit_message_caption(self.chat_id, msg_id)
                    .caption(text.to_owned())
                    .parse_mode(ParseMode::Html);
                if let Some(k) = kb.clone() {
                    req = req.reply_markup(k);
                }
                req.await.is_ok()
            } else {
                let mut req = self
                    .bot
                    .edit_message_text(self.chat_id, msg_id, text.to_owned())
                    .parse_mode(ParseMode::Html);
                if let Some(k) = kb.clone() {
                    req = req.reply_markup(k);
                }
                req.await.is_ok()
            };
            if ok {
                return;
            }
        }
        self.reply(text, kb).await;
    }

    pub async fn reply(&self, text: &str, kb: Option<InlineKeyboardMarkup>) {
        let mut req = self
            .bot
            .send_message(self.chat_id, text.to_owned())
            .parse_mode(ParseMode::Html);
        if let Some(k) = kb {
            req = req.reply_markup(k);
        }
        if let Err(e) = req.await {
            tracing::warn!("[bot] reply gagal: {e}");
        }
    }

    pub async fn reply_photo(
        &self,
        file_id: &str,
        caption: &str,
        kb: Option<InlineKeyboardMarkup>,
    ) {
        let mut req = self
            .bot
            .send_photo(self.chat_id, InputFile::file_id(FileId(file_id.to_owned())))
            .caption(caption.to_owned())
            .parse_mode(ParseMode::Html);
        if let Some(k) = kb {
            req = req.reply_markup(k);
        }
        if let Err(e) = req.await {
            tracing::error!("[bot] kirim foto gagal: {e}");
        }
    }

    pub async fn answer_callback(&self, query_id: &teloxide::types::CallbackQueryId) {
        if let Err(e) = self.bot.answer_callback_query(query_id.clone()).await {
            tracing::debug!("[bot] answerCallbackQuery gagal: {e}");
        }
    }
}

// ---------------------------------------------------------------------------
// Keyboard.
// ---------------------------------------------------------------------------

fn cb(text: &str, data: &str) -> InlineKeyboardButton {
    InlineKeyboardButton::callback(text.to_owned(), data.to_owned())
}

fn url_btn(text: &str, url: &str) -> Option<InlineKeyboardButton> {
    reqwest::Url::parse(url)
        .ok()
        .map(|u| InlineKeyboardButton::url(text.to_owned(), u))
}

fn menu_keyboard(s: &Strings, state: &BotState) -> InlineKeyboardMarkup {
    let mut rows: Vec<Vec<InlineKeyboardButton>> = Vec::new();
    if let Some(web_url) = get_web_app_url(&state.cfg.app_url) {
        if let Ok(url) = reqwest::Url::parse(&web_url) {
            rows.push(vec![InlineKeyboardButton::web_app(
                "🌐 Buka Toko".to_owned(),
                WebAppInfo { url },
            )]);
        }
    }
    rows.push(vec![cb(s.catalog, "cat")]);
    rows.push(vec![cb(s.my_balance, "wallet"), cb(s.my_orders, "orders")]);
    rows.push(vec![cb(s.language, "lang"), cb(s.policy, "policy")]);
    rows.push(vec![cb(s.support, "support")]);
    let channel_url = state.cfg.telegram_channel_url.trim();
    if !channel_url.is_empty() {
        if let Some(b) = url_btn(s.follow_channel, channel_url) {
            rows.push(vec![b]);
        }
    }
    InlineKeyboardMarkup::new(rows)
}

fn back_row(s: &Strings) -> InlineKeyboardMarkup {
    InlineKeyboardMarkup::new(vec![vec![cb(s.back_menu, "menu")]])
}

fn back_to_catalog_row(s: &Strings) -> InlineKeyboardMarkup {
    InlineKeyboardMarkup::new(vec![vec![
        cb("⬅️ Katalog", "cat"),
        cb(s.back_menu, "menu"),
    ]])
}

// ---------------------------------------------------------------------------
// Wajib-join channel.
// ---------------------------------------------------------------------------

async fn is_channel_member(ctx: &Ctx, tg_id: i64, force: bool) -> bool {
    let channel_id = ctx.state.cfg.telegram_channel_id.trim().to_string();
    if channel_id.is_empty() {
        return true; // channel belum dikonfigurasi = gate mati (fail open)
    }
    if !force {
        if let Ok(cache) = ctx.state.join_cache.lock() {
            if let Some((joined, at)) = cache.get(&tg_id) {
                if at.elapsed() < CHANNEL_JOIN_CACHE {
                    return *joined;
                }
            }
        }
    }
    let mut joined = true; // fail open saat API error
    let recipient = match channel_id.parse::<i64>() {
        Ok(n) => Recipient::Id(ChatId(n)),
        Err(_) => Recipient::ChannelUsername(
            channel_id.trim_start_matches('@').to_string(),
        ),
    };
    match ctx.bot.get_chat_member(recipient, ctx.user.id).await {
        Ok(member) => {
            joined = matches!(
                member.kind.status(),
                ChatMemberStatus::Owner
                    | ChatMemberStatus::Administrator
                    | ChatMemberStatus::Member
            );
        }
        Err(e) => {
            tracing::error!("[join-gate] getChatMember gagal (fail open): {e}");
        }
    }
    if let Ok(mut cache) = ctx.state.join_cache.lock() {
        cache.insert(tg_id, (joined, Instant::now()));
    }
    joined
}

async fn show_join_screen(ctx: &Ctx, not_yet: bool) {
    let s = ctx.strs();
    let url = ctx.state.cfg.telegram_channel_url.trim().to_string();
    let mut row = vec![];
    if !url.is_empty() {
        if let Some(b) = url_btn(s.join_channel_button, &url) {
            row.push(b);
        }
    }
    let kb = InlineKeyboardMarkup::new(vec![
        row,
        vec![cb(s.join_channel_check_button, "check_joined")],
    ]);
    let text = if not_yet {
        format!("{}\n\n{}", s.join_channel_text, s.join_channel_not_yet)
    } else {
        s.join_channel_text.to_string()
    };
    ctx.render(&text, Some(kb)).await;
}

/// Gate: true = boleh lanjut; false = layar join sudah ditampilkan.
async fn require_joined(ctx: &Ctx) -> bool {
    if is_channel_member(ctx, ctx.uid(), false).await {
        return true;
    }
    show_join_screen(ctx, false).await;
    false
}

// ---------------------------------------------------------------------------
// Menu / katalog / detail produk.
// ---------------------------------------------------------------------------

pub async fn show_menu(ctx: &Ctx) -> anyhow::Result<()> {
    if !require_joined(ctx).await {
        return Ok(());
    }
    let s = ctx.strs();
    let id = ctx.uid();
    let (_count, total_spent) = get_buyer_stats(&ctx.state.db, &format!("tg:{id}"));
    let wallet = get_wallet(&ctx.state.db, id);
    let (total_users, delivered_orders) = get_store_stats(&ctx.state.db);
    let text = (s.welcome)(&WelcomeParams {
        name: ctx.user.first_name.clone(),
        tg_id: id,
        username: ctx.user.username.clone(),
        total_spent_idr: total_spent,
        balance_idr: wallet.balance_idr,
        total_users,
        delivered_orders,
        channel_url: ctx.state.cfg.telegram_channel_url.trim().to_string(),
        hour_wib: deps::wib_hour_now(),
    });
    let kb = menu_keyboard(s, &ctx.state);
    // Home dari command (/start, /menu): kirim banner sebagai foto.
    // Dari tombol (callback) tetap edit teks — supaya chat tidak dipenuhi
    // foto setiap kali kembali ke menu. Kalau env kosong / foto gagal,
    // fallback ke balasan teks biasa.
    if !ctx.is_callback {
        let file_id = ctx.state.cfg.bot_banner_file_id.trim().to_string();
        if !file_id.is_empty() {
            let req = ctx
                .bot
                .send_photo(ctx.chat_id, InputFile::file_id(FileId(file_id)))
                .caption(text.clone())
                .parse_mode(ParseMode::Html)
                .reply_markup(kb.clone());
            match req.await {
                Ok(_) => return Ok(()),
                Err(e) => {
                    tracing::error!("[bot] kirim banner gagal, fallback teks: {e}");
                }
            }
        }
    }
    ctx.render(&text, Some(kb)).await;
    Ok(())
}

pub async fn show_catalog(ctx: &Ctx, category: Option<&str>) -> anyhow::Result<()> {
    let s = ctx.strs();
    let db = &ctx.state.db;
    let sondev_cats: Vec<String> = {
        let mut seen: Vec<String> = Vec::new();
        for p in catalog::list_active_products(db) {
            if p.supplier == SUPPLIER_SONDEV && !seen.contains(&p.category.to_string()) {
                seen.push(p.category.to_string());
            }
        }
        seen
    };
    // Tanpa kategori = langsung daftar produk kalau cuma ada 1 kategori.
    let category: Option<String> = match category {
        Some(c) => Some(c.to_string()),
        None if sondev_cats.len() == 1 => Some(sondev_cats[0].clone()),
        None => None,
    };
    let Some(category) = category else {
        let mut rows: Vec<Vec<InlineKeyboardButton>> = sondev_cats
            .iter()
            .map(|c| vec![cb(c, &format!("cat:{c}"))])
            .collect();
        rows.push(vec![cb(s.category_all, "cat:all")]);
        rows.push(vec![cb(s.back_menu, "menu")]);
        ctx.render(
            s.catalog_pick_category,
            Some(InlineKeyboardMarkup::new(rows)),
        )
        .await;
        return Ok(());
    };
    let category = category.as_str();
    let mut rows: Vec<Vec<InlineKeyboardButton>> = Vec::new();
    for p in catalog::list_active_products_by_category(db, category) {
        if p.supplier != SUPPLIER_SONDEV {
            continue;
        }
        let emoji = match p.stock_state {
            StockState::Available => "✅",
            StockState::OutOfStock => "🔴",
            StockState::ComingSoon => "⏳",
        };
        let suffix = if p.stock_state == StockState::Available {
            crate::i18n::rupiah(p.retail_price_idr)
        } else {
            (s.stock_label)(p.stock_state)
        };
        rows.push(vec![cb(
            &format!("{emoji} {} — {suffix}", p.name),
            &format!("prod:{}", p.id),
        )]);
    }
    rows.push(vec![
        cb("⬅️ Kategori", "cat"),
        cb(s.back_menu, "menu"),
    ]);
    ctx.render(s.catalog_title, Some(InlineKeyboardMarkup::new(rows)))
        .await;
    Ok(())
}

pub async fn show_product(ctx: &Ctx, product_id: &str) -> anyhow::Result<()> {
    let s = ctx.strs();
    let db = &ctx.state.db;
    let product = catalog::get_catalog_product(db, product_id)?;
    let mut rows: Vec<Vec<InlineKeyboardButton>> = Vec::new();
    if product.stock_state != StockState::Available {
        // Format stok habis sudah membawa nama produk sendiri — jangan dobel.
        if product.stock_state == StockState::OutOfStock {
            ctx.render(
                &(s.out_of_stock)(product.name),
                Some(back_to_catalog_row(s)),
            )
            .await;
            return Ok(());
        }
        ctx.render(
            &format!("{}\n\n{}", product.name, s.coming_soon),
            Some(back_to_catalog_row(s)),
        )
        .await;
        return Ok(());
    }
    if product.id == "muse-token-1b" {
        // Ala Sondev: kode invite DULU, baru pilih jumlah — paket qty tidak
        // ditampilkan di detail; muncul setelah kode diterima.
        rows.push(vec![cb(
            "⚡ Masukkan Kode Invite",
            &format!("entercode:{}", product.id),
        )]);
    } else {
        rows.push(vec![cb(
            &format!(
                "🛒 Beli 1 — {}",
                crate::i18n::rupiah(product.retail_price_idr)
            ),
            &format!("qty:{}:1", product.id),
        )]);
    }
    rows.push(vec![
        cb("⬅️ Katalog", "cat"),
        cb(s.back_menu, "menu"),
    ]);
    let kb = InlineKeyboardMarkup::new(rows);
    // targetNote di TS hardcode Bahasa Indonesia untuk semua bahasa —
    // dipertahankan persis (kandidat perbaikan i18n, bukan perubahan diam-diam).
    let caption = (s.product_detail)(&ProductDetailParams {
        name: product.name.to_string(),
        desc: product.description.to_string(),
        price: product.retail_price_idr,
        unit: product.unit.map(|u| u.to_string()),
        badge: product.badge.map(|b| b.to_string()),
        features: product.features.iter().map(|f| f.to_string()).collect(),
        stock: (s.stock_label)(product.stock_state),
        target_note: if product.requires_target {
            Some("Token dikirim ke akun Muse kamu — masukkan kode invite dulu (buka aplikasi Muse → Invite → salin kode/linknya).".to_string())
        } else {
            None
        },
    });
    // Detail produk ala Lhie: banner foto produk + caption (tetap 1 pesan).
    // Pesan saat ini foto -> ganti medianya; pesan teks -> hapus lalu kirim
    // foto baru (editMessageMedia pada pesan teks DITOLAK Telegram).
    // Kalau banner tidak ada / gagal -> fallback teks biasa.
    let banner_file_id = deps::product_banner_file_id(&ctx.state.cfg, product.id);
    if !banner_file_id.is_empty() {
        if let Some(msg_id) = ctx.msg_id {
            if ctx.is_photo {
                let media = InputMedia::Photo(
                    InputMediaPhoto::new(InputFile::file_id(FileId(banner_file_id.clone())))
                        .caption(caption.clone())
                        .parse_mode(ParseMode::Html),
                );
                let mut req = ctx.bot.edit_message_media(ctx.chat_id, msg_id, media);
                req = req.reply_markup(kb.clone());
                if req.await.is_ok() {
                    return Ok(());
                }
                tracing::error!("[bot] banner produk gagal, fallback teks");
            } else {
                let _ = ctx.bot.delete_message(ctx.chat_id, msg_id).await;
                let req = ctx
                    .bot
                    .send_photo(ctx.chat_id, InputFile::file_id(FileId(banner_file_id)))
                    .caption(caption.clone())
                    .parse_mode(ParseMode::Html)
                    .reply_markup(kb.clone());
                if req.await.is_ok() {
                    return Ok(());
                }
                tracing::error!("[bot] banner produk gagal, fallback teks");
            }
        } else {
            ctx.reply_photo(&banner_file_id, &caption, Some(kb.clone()))
                .await;
            return Ok(());
        }
    }
    ctx.render(&caption, Some(kb)).await;
    Ok(())
}

// ---------------------------------------------------------------------------
// Checkout.
// ---------------------------------------------------------------------------

async fn ask_code(ctx: &Ctx, product_id: &str) {
    let s = ctx.strs();
    ctx.update_session(|sess| {
        sess.awaiting = Some(AwaitingInput::Code {
            product_id: product_id.to_string(),
        });
    });
    ctx.render(s.code_prompt, Some(back_row(s))).await;
}

/// Kode diterima: tampilkan pilihan jumlah ala Sondev (harga dasar + stok
/// live dari /stock; gagal baca stok = "Tersedia", jangan blokir checkout).
async fn show_qty_after_code(
    ctx: &Ctx,
    product_id: &str,
    code: &str,
) -> anyhow::Result<()> {
    let s = ctx.strs();
    let db = &ctx.state.db;
    let product = catalog::get_catalog_product(db, product_id)?;
    ctx.update_session(|sess| {
        sess.draft = Some(CheckoutDraft {
            product_id: product_id.to_string(),
            qty: 0,
            target: Some(code.to_string()),
            ..Default::default()
        });
    });
    let mut stock_label = (s.stock_label)(StockState::Available);
    match get_supplier(
        SupplierId::from_str(product.supplier).unwrap_or(SupplierId::Sondev),
        &ctx.state.cfg,
    ) {
        Ok(supplier) => {
            if let Ok(Some(stock)) = supplier
                .get_stock(product.supplier_product_code)
                .await
            {
                stock_label = format!("{stock}B");
            }
        }
        Err(e) => {
            tracing::warn!("[bot] get_supplier gagal (stok jadi Tersedia): {e}");
        }
    }
    let mut rows: Vec<Vec<InlineKeyboardButton>> = Vec::new();
    for chunk in QTY_PACKS.chunks(2) {
        if chunk.len() == 2 {
            let (a, b) = (chunk[0], chunk[1]);
            rows.push(vec![
                cb(
                    &format!("{a}B ({})", crate::i18n::rupiah(product.retail_price_idr * a)),
                    &format!("qty:{}:{a}", product.id),
                ),
                cb(
                    &format!("{b}B ({})", crate::i18n::rupiah(product.retail_price_idr * b)),
                    &format!("qty:{}:{b}", product.id),
                ),
            ]);
        } else {
            let a = chunk[0];
            rows.push(vec![cb(
                &format!("{a}B (Maks) ({})", crate::i18n::rupiah(product.retail_price_idr * a)),
                &format!("qty:{}:{a}", product.id),
            )]);
        }
    }
    rows.push(vec![cb("✏️ Jumlah lain…", &format!("qtycustom:{}", product.id))]);
    rows.push(vec![
        cb("⬅️ Katalog", "cat"),
        cb(s.back_menu, "menu"),
    ]);
    ctx.render(
        &(s.qty_title)(&QtyTitleParams {
            code: code.to_string(),
            price: product.retail_price_idr,
            stock: stock_label,
        }),
        Some(InlineKeyboardMarkup::new(rows)),
    )
    .await;
    Ok(())
}

fn draft_totals(
    db: &crate::db::Db,
    draft: &CheckoutDraft,
) -> anyhow::Result<(i64, i64)> {
    let product = catalog::get_catalog_product(db, &draft.product_id)?;
    let subtotal = product.retail_price_idr * draft.qty;
    Ok((subtotal, (subtotal - draft.discount_idr).max(0)))
}

async fn ask_promo(ctx: &Ctx, draft: &CheckoutDraft) {
    let s = ctx.strs();
    let kb = InlineKeyboardMarkup::new(vec![
        vec![cb("🎟️ Masukkan Kode Promo", "promo_enter")],
        vec![cb("➡️ Lewati", "promo_skip")],
        vec![cb(s.back_menu, "menu")],
    ]);
    let _ = draft;
    ctx.render(s.promo_ask, Some(kb)).await;
}

async fn show_pay(ctx: &Ctx, draft: &CheckoutDraft, notice: Option<&str>) {
    let s = ctx.strs();
    let id = ctx.uid();
    let product = match catalog::get_catalog_product(&ctx.state.db, &draft.product_id) {
        Ok(p) => p,
        Err(e) => {
            tracing::error!("[bot] show_pay: produk tidak ditemukan: {e}");
            return;
        }
    };
    let (subtotal, total) = match draft_totals(&ctx.state.db, draft) {
        Ok(t) => t,
        Err(e) => {
            tracing::error!("[bot] show_pay: {e}");
            return;
        }
    };
    let wallet = get_wallet(&ctx.state.db, id);
    let mut rows: Vec<Vec<InlineKeyboardButton>> = Vec::new();
    if total == 0 {
        rows.push(vec![cb("✅ Proses Order (Gratis)", "pay_wallet")]);
    } else {
        rows.push(vec![cb(
            &format!("{} ({})", s.pay_wallet, crate::i18n::rupiah(total)),
            "pay_wallet",
        )]);
        rows.push(vec![cb(
            &format!("{} ({})", s.pay_midtrans, crate::i18n::rupiah(total)),
            "pay_midtrans",
        )]);
    }
    rows.push(vec![cb("🎟️ Pakai Promo", "promo_enter")]);
    rows.push(vec![
        cb("❌ Batal", "cancel_checkout"),
        cb(s.back_menu, "menu"),
    ]);
    let mut text = String::new();
    if let Some(n) = notice {
        text.push_str(n);
        text.push_str("\n\n");
    }
    text.push_str(&(s.pay_title)(&PayTitleParams {
        name: product.name.to_string(),
        qty: draft.qty,
        subtotal,
        discount: draft.discount_idr,
        total,
        balance: wallet.balance_idr,
        code: draft.target.clone(),
    }));
    ctx.render(&text, Some(InlineKeyboardMarkup::new(rows))).await;
}

/// Mulai checkout: jumlah sudah dipilih. Target dulu bila wajib, lalu promo.
async fn begin_checkout(ctx: &Ctx, product_id: &str, qty: i64) -> anyhow::Result<()> {
    let s = ctx.strs();
    let product = catalog::get_catalog_product(&ctx.state.db, product_id)?;
    if product.stock_state != StockState::Available {
        if product.stock_state == StockState::OutOfStock {
            ctx.render(
                &(s.out_of_stock)(product.name),
                Some(back_to_catalog_row(s)),
            )
            .await;
            return Ok(());
        }
        ctx.render(
            &format!("{}\n\n{}", product.name, s.coming_soon),
            Some(back_to_catalog_row(s)),
        )
        .await;
        return Ok(());
    }
    let draft = CheckoutDraft {
        product_id: product_id.to_string(),
        qty,
        ..Default::default()
    };
    ctx.update_session(|sess| {
        sess.draft = Some(draft.clone());
    });
    if product.requires_target {
        // Ala Sondev: kode invite dulu (langkahnya dijelaskan di prompt).
        ask_code(ctx, product_id).await;
        return Ok(());
    }
    ask_promo(ctx, &draft).await;
    Ok(())
}

/// Pastikan order DB ada untuk draft ini (dipakai ulang saat ganti cara
/// bayar / retry). Promo ditukar SEBELUM order dibuat, dengan ID order
/// yang sama.
async fn ensure_draft_order(
    ctx: &Ctx,
    draft: &mut CheckoutDraft,
) -> Result<StoreOrder, String> {
    let id = ctx.uid();
    let s = ctx.strs();
    if let Some(order_id) = &draft.order_id {
        if let Some(existing) = get_store_order(&ctx.state.db, order_id) {
            return Ok(existing);
        }
    }
    // Cek ulang batas 30x sebelum order dibuat — kode bisa mencapai 30x di
    // antara input kode dan bayar. Dicek sebelum promo ditukar supaya promo
    // tidak hangus sia-sia.
    if let Some(target) = &draft.target {
        if count_code_uses(&ctx.state.db, target) >= MAX_CODE_USES {
            return Err((s.code_used_up)(target));
        }
    }
    let order_id = new_order_id();
    if draft.promo_code.is_some() && !draft.promo_redeemed {
        let code = draft.promo_code.clone().unwrap();
        let redeemed = redeem_promo(
            &ctx.state.db,
            &code,
            id,
            &order_id,
            draft.discount_idr,
        );
        if !redeemed {
            draft.promo_code = None;
            draft.discount_idr = 0;
            draft.promo_redeemed = false;
            return Err((s.promo_invalid)("already_used"));
        }
        draft.promo_redeemed = true;
    }
    let order = create_store_order(
        &ctx.state.db,
        CreateStoreOrderInput {
            id: Some(order_id),
            catalog_product_id: draft.product_id.clone(),
            quantity: Some(draft.qty),
            buyer_ref: Some(format!("tg:{id}")),
            xiaomi_accounts: None,
            sheerid_url: None,
            target: draft.target.clone(),
            discount_idr: Some(draft.discount_idr),
            promo_code: draft.promo_code.clone(),
        },
    )
    .map_err(|e| e.to_string())?;
    draft.order_id = Some(order.id.clone());
    Ok(order)
}

/// Jalankan fulfillment di background + kabari pembeli saat selesai.
pub(crate) fn fulfill_in_background(state: &Arc<BotState>, order_id: String) {
    let state = Arc::clone(state);
    tokio::spawn(async move {
        match fulfill_order_now(&state, &order_id).await {
            Ok(done) => notify_buyer(&state.notify_ctx, &state.db, &done).await,
            Err(e) => tracing::error!("[bot] fulfill {order_id} gagal: {e}"),
        }
    });
}

async fn fulfill_order_now(
    state: &BotState,
    order_id: &str,
) -> anyhow::Result<StoreOrder> {
    let order = get_store_order(&state.db, order_id)
        .ok_or_else(|| anyhow::anyhow!("Order tidak ditemukan: {order_id}"))?;
    let supplier = get_supplier(
        SupplierId::from_str(&order.supplier).unwrap_or(SupplierId::Sondev),
        &state.cfg,
    )?;
    fulfill_order(
        &state.db,
        Some(&state.notify_ctx),
        &supplier,
        order_id,
    )
    .await
}

fn order_detail_text(s: &Strings, order: &StoreOrder) -> String {
    (s.order_detail)(&OrderDetailParams {
        id: order.id.clone(),
        product: order.catalog_product_id.clone(),
        qty: order.quantity,
        total: order.retail_total_idr,
        status: (s.status_label)(order.status.as_str()),
        items: order.delivered_items.clone(),
        link: order.result_link.clone(),
        error: order.error.clone(),
    })
}

async fn pay_with_wallet(ctx: &Ctx) {
    let s = ctx.strs();
    let id = ctx.uid();
    let mut draft = match ctx.session().draft {
        Some(d) => d,
        None => {
            let _ = show_catalog(ctx, None).await;
            return;
        }
    };
    match ensure_draft_order(ctx, &mut draft).await {
        Ok(order) => {
            ctx.update_session(|sess| sess.draft = Some(draft.clone()));
            if order.status.as_str() != "awaiting_payment" {
                ctx.render(
                    &order_detail_text(s, &order),
                    Some(back_row(s)),
                )
                .await;
                return;
            }
            if let Err(e) = set_order_paid_via(&ctx.state.db, &order.id, "wallet") {
                tracing::error!("[bot] setOrderPaidVia gagal: {e}");
            }
            if order.retail_total_idr > 0 {
                let product_name =
                    catalog::get_catalog_product(&ctx.state.db, &order.catalog_product_id)
                        .map(|p| p.name.to_string())
                        .unwrap_or_else(|_| order.catalog_product_id.clone());
                match debit_wallet(
                    &ctx.state.db,
                    id,
                    order.retail_total_idr,
                    "purchase",
                    Some(&order.id),
                    Some(&product_name),
                ) {
                    Ok(Some(_)) => {}
                    Ok(None) => {
                        show_pay(ctx, &draft, Some(s.wallet_insufficient)).await;
                        return;
                    }
                    Err(e) => {
                        tracing::error!("[bot] debitWallet gagal: {e}");
                        show_pay(ctx, &draft, Some(s.wallet_insufficient)).await;
                        return;
                    }
                }
            }
            if let Err(e) = mark_paid(&ctx.state.db, &order.id) {
                tracing::error!("[bot] markPaid gagal: {e}");
                return;
            }
            if !maybe_reward_referrer(&ctx.state.db, id) {
                // Bukan error — hanya berarti tidak ada referrer yang berhak.
            }
            ctx.update_session(|sess| {
                sess.draft = None;
                sess.awaiting = None;
            });
            let text = if order.retail_total_idr == 0 {
                (s.free_order_done)(&order.id)
            } else {
                (s.wallet_paid)(&order.id)
            };
            ctx.render(&text, Some(back_row(s))).await;
            fulfill_in_background(&ctx.state, order.id);
        }
        Err(err) => {
            ctx.update_session(|sess| sess.draft = Some(draft.clone()));
            show_pay(ctx, &draft, Some(&format!("⚠️ {err}"))).await;
        }
    }
}

/// Foto QR: kartu standee Dim + QR dinamis di tengah. Referensi (id order
/// UD-… / id top-up TOPUP-…) menempelkan tombol "Saya Sudah Bayar" di bawah
/// foto — pembeli tap setelah bayar, bot langsung cek saat itu juga (di
/// samping poller otomatis tiap 30 detik).
async fn send_qris_photo(
    ctx: &Ctx,
    qr_string: &str,
    caption: &str,
    reference: Option<&str>,
) {
    let s = ctx.strs();
    // Gambar QR untuk pembeli: kartu komposit standee kalau template ada,
    // QR polos kalau tidak (render_qris_card_or_plain, kelompok C).
    let png = match render_qris_card_or_plain(qr_string) {
        Ok(img) => img.png,
        Err(e) => {
            tracing::error!("[bot] render QR gagal: {e}");
            ctx.reply(&(s.qris_failed)(None), Some(back_row(s))).await;
            return;
        }
    };
    let mut rows: Vec<Vec<InlineKeyboardButton>> = Vec::new();
    if let Some(reference) = reference {
        rows.push(vec![cb(
            s.qris_check_button,
            &format!("qris_check:{reference}"),
        )]);
    }
    rows.push(vec![cb(s.back_menu, "menu")]);
    let req = ctx
        .bot
        .send_photo(ctx.chat_id, InputFile::memory(png))
        .caption(caption.to_owned())
        .parse_mode(ParseMode::Html)
        .reply_markup(InlineKeyboardMarkup::new(rows));
    if let Err(e) = req.await {
        tracing::error!("[bot] kirim foto QR gagal: {e}");
    }
}

/// Cek manual pembayaran QRIS (tombol "Saya Sudah Bayar" di foto QR):
/// jalankan satu tick poller sekarang, lalu laporkan status referensi itu.
/// Throttle 10 detik per referensi supaya feed GoBiz tidak dihajar spam tap.
async fn check_qris_payment(ctx: &Ctx, reference: &str) {
    let s = ctx.strs();
    let check_kb = InlineKeyboardMarkup::new(vec![
        vec![cb(
            s.qris_check_button,
            &format!("qris_check:{reference}"),
        )],
        vec![cb(s.back_menu, "menu")],
    ]);
    let now = Instant::now();
    let too_soon = {
        let mut map = ctx.state.manual_check.lock().unwrap();
        match map.get(reference) {
            Some(last) if now.duration_since(*last) < MANUAL_CHECK_MIN => true,
            _ => {
                map.insert(reference.to_string(), now);
                false
            }
        }
    };
    if too_soon {
        ctx.reply(s.qris_check_too_soon, Some(check_kb)).await;
        return;
    }
    let settlement = BotSettlement::new(Arc::clone(&ctx.state));
    let tick = tick_qris_payments(
        &ctx.state.db,
        &ctx.state.gopay_client,
        &settlement,
    )
    .await;
    if !tick.ok {
        let msg = if matches!(tick.code, Some(QrisPollCode::AuthRequired)) {
            s.qris_check_no_session
        } else {
            s.qris_check_failed
        };
        ctx.reply(msg, Some(check_kb)).await;
        return;
    }
    let payment = get_qris_payment_by_reference(&ctx.state.db, reference)
        .ok()
        .flatten();
    if reference.starts_with("UD-") {
        let order = get_store_order(&ctx.state.db, reference);
        let Some(order) = order else {
            ctx.reply(s.qris_check_failed, Some(check_kb)).await;
            return;
        };
        if matches!(
            order.status.as_str(),
            "paid" | "processing" | "delivered"
        ) {
            ctx.reply(
                &(s.qris_check_found_order)(&order.id),
                Some(back_row(s)),
            )
            .await;
            return;
        }
        let expired = payment
            .as_ref()
            .map(|p| p.status.as_str() == "expired")
            .unwrap_or(false)
            || (order.status.as_str() == "failed"
                && order.error.as_deref().unwrap_or("").contains("kedaluwarsa"));
        if expired {
            ctx.reply(s.qris_check_expired, Some(back_row(s))).await;
            return;
        }
        ctx.reply(s.qris_check_pending, Some(check_kb)).await;
        return;
    }
    if reference.starts_with("TOPUP-") {
        let topup = get_topup(&ctx.state.db, reference);
        let Some(topup) = topup else {
            ctx.reply(s.qris_check_failed, Some(check_kb)).await;
            return;
        };
        if topup.status == "paid" {
            ctx.reply(
                &(s.qris_check_found_topup)(topup.amount_idr),
                Some(back_row(s)),
            )
            .await;
            return;
        }
        if payment
            .as_ref()
            .map(|p| p.status.as_str() == "expired")
            .unwrap_or(false)
        {
            ctx.reply(s.qris_check_expired, Some(back_row(s))).await;
            return;
        }
        ctx.reply(s.qris_check_pending, Some(check_kb)).await;
        return;
    }
    ctx.reply(s.qris_check_failed, Some(check_kb)).await;
}

/// Cek manual pembayaran Midtrans (tombol "Saya Sudah Bayar" di pesan link
/// bayar) — cerminan checkQrisPayment untuk jalur Midtrans.
async fn check_midtrans_payment(ctx: &Ctx, reference: &str) {
    let s = ctx.strs();
    let check_kb = InlineKeyboardMarkup::new(vec![
        vec![cb(
            s.qris_check_button,
            &format!("midtrans_check:{reference}"),
        )],
        vec![cb(s.back_menu, "menu")],
    ]);
    let now = Instant::now();
    let too_soon = {
        let mut map = ctx.state.manual_check.lock().unwrap();
        match map.get(reference) {
            Some(last) if now.duration_since(*last) < MANUAL_CHECK_MIN => true,
            _ => {
                map.insert(reference.to_string(), now);
                false
            }
        }
    };
    if too_soon {
        ctx.reply(s.qris_check_too_soon, Some(check_kb)).await;
        return;
    }
    if !midtrans_configured(&ctx.state.cfg) {
        ctx.reply(s.midtrans_check_no_config, Some(check_kb))
            .await;
        return;
    }
    let settlement = BotSettlement::new(Arc::clone(&ctx.state));
    let tick = tick_midtrans_payments(
        &ctx.state.http_client,
        &ctx.state.cfg,
        &ctx.state.db,
        &settlement,
    )
    .await;
    if !tick.ok {
        ctx.reply(s.qris_check_failed, Some(check_kb)).await;
        return;
    }
    let payment = get_midtrans_payment(&ctx.state.db, reference)
        .ok()
        .flatten();
    if reference.starts_with("UD-") {
        let order = get_store_order(&ctx.state.db, reference);
        let Some(order) = order else {
            ctx.reply(s.qris_check_failed, Some(check_kb)).await;
            return;
        };
        if matches!(
            order.status.as_str(),
            "paid" | "processing" | "delivered"
        ) {
            ctx.reply(
                &(s.qris_check_found_order)(&order.id),
                Some(back_row(s)),
            )
            .await;
            return;
        }
        let expired = payment
            .as_ref()
            .map(|p| p.status.as_str() == "expired")
            .unwrap_or(false)
            || (order.status.as_str() == "failed"
                && order.error.as_deref().unwrap_or("").contains("kedaluwarsa"));
        if expired {
            ctx.reply(s.qris_check_expired, Some(back_row(s))).await;
            return;
        }
        ctx.reply(s.qris_check_pending, Some(check_kb)).await;
        return;
    }
    if reference.starts_with("TOPUP-") {
        let topup = get_topup(&ctx.state.db, reference);
        let Some(topup) = topup else {
            ctx.reply(s.qris_check_failed, Some(check_kb)).await;
            return;
        };
        if topup.status == "paid" {
            ctx.reply(
                &(s.qris_check_found_topup)(topup.amount_idr),
                Some(back_row(s)),
            )
            .await;
            return;
        }
        if payment
            .as_ref()
            .map(|p| p.status.as_str() == "expired")
            .unwrap_or(false)
        {
            ctx.reply(s.qris_check_expired, Some(back_row(s))).await;
            return;
        }
        ctx.reply(s.qris_check_pending, Some(check_kb)).await;
        return;
    }
    ctx.reply(s.qris_check_failed, Some(check_kb)).await;
}

/// Bayar order via QRIS dinamis GoBiz (jalur LAMA).
/// TIDAK ditawarkan di menu sejak Midtrans jadi jalur aktif (2026-11-21);
/// dipertahankan sebagai fallback + dipakai tes QRIS.
async fn pay_with_qris(ctx: &Ctx) {
    let s = ctx.strs();
    let id = ctx.uid();
    let mut draft = match ctx.session().draft {
        Some(d) => d,
        None => {
            let _ = show_catalog(ctx, None).await;
            return;
        }
    };
    match ensure_draft_order(ctx, &mut draft).await {
        Ok(order) => {
            ctx.update_session(|sess| sess.draft = Some(draft.clone()));
            if order.status.as_str() != "awaiting_payment" {
                ctx.render(&order_detail_text(s, &order), Some(back_row(s)))
                    .await;
                return;
            }
            if let Err(e) = set_order_paid_via(&ctx.state.db, &order.id, "qris") {
                tracing::error!("[bot] setOrderPaidVia gagal: {e}");
            }
            if order.retail_total_idr == 0 {
                // Gratis berkat promo — tanpa QR.
                if let Err(e) = mark_paid(&ctx.state.db, &order.id) {
                    tracing::error!("[bot] markPaid gagal: {e}");
                    return;
                }
                let _ = maybe_reward_referrer(&ctx.state.db, id);
                ctx.update_session(|sess| {
                    sess.draft = None;
                    sess.awaiting = None;
                });
                ctx.render(&(s.free_order_done)(&order.id), Some(back_row(s)))
                    .await;
                fulfill_in_background(&ctx.state, order.id);
                return;
            }
            match get_gopay_static_qris(&ctx.state.db) {
                Ok(Some(static_payload)) => {
                    match create_qris_payment(
                        &ctx.state.db,
                        &order.id,
                        order.retail_total_idr,
                        &static_payload,
                        None,
                    ) {
                        Ok(created) => {
                            ctx.update_session(|sess| {
                                sess.draft = None;
                                sess.awaiting = None;
                            });
                            send_qris_photo(
                                ctx,
                                &created.qr_string,
                                &(s.qris_link)(
                                    &order.id,
                                    order.retail_total_idr,
                                    created.payment.unique_amount_idr,
                                    &created.payment.expires_at,
                                ),
                                Some(&order.id),
                            )
                            .await;
                        }
                        Err(e) => {
                            tracing::error!("[bot] QRIS gagal: {e}");
                            ctx.render(
                                &(s.qris_failed)(Some(&e.to_string())),
                                Some(back_row(s)),
                            )
                            .await;
                        }
                    }
                }
                Ok(None) => {
                    ctx.render(
                        &(s.qris_failed)(Some(
                            "QRIS statis belum dipasang di panel admin (/admin).",
                        )),
                        Some(back_row(s)),
                    )
                    .await;
                }
                Err(e) => {
                    tracing::error!("[bot] baca QRIS statis gagal: {e}");
                    ctx.render(
                        &(s.qris_failed)(Some(&e.to_string())),
                        Some(back_row(s)),
                    )
                    .await;
                }
            }
        }
        Err(err) => {
            ctx.update_session(|sess| sess.draft = Some(draft.clone()));
            show_pay(ctx, &draft, Some(&format!("⚠️ {err}"))).await;
        }
    }
}

/// Bayar order via Midtrans Snap (QRIS) — jalur pembayaran aktif.
/// Kalau order ini sudah punya link Midtrans pending yang belum kedaluwarsa,
/// pakai lagi link itu (jangan buat transaksi baru dengan order_id sama).
async fn pay_with_midtrans(ctx: &Ctx) {
    let s = ctx.strs();
    let id = ctx.uid();
    let mut draft = match ctx.session().draft {
        Some(d) => d,
        None => {
            let _ = show_catalog(ctx, None).await;
            return;
        }
    };
    match ensure_draft_order(ctx, &mut draft).await {
        Ok(order) => {
            ctx.update_session(|sess| sess.draft = Some(draft.clone()));
            if order.status.as_str() != "awaiting_payment" {
                ctx.render(&order_detail_text(s, &order), Some(back_row(s)))
                    .await;
                return;
            }
            if let Err(e) = set_order_paid_via(&ctx.state.db, &order.id, "midtrans") {
                tracing::error!("[bot] setOrderPaidVia gagal: {e}");
            }
            if order.retail_total_idr == 0 {
                // Gratis berkat promo — tanpa bayar.
                if let Err(e) = mark_paid(&ctx.state.db, &order.id) {
                    tracing::error!("[bot] markPaid gagal: {e}");
                    return;
                }
                let _ = maybe_reward_referrer(&ctx.state.db, id);
                ctx.update_session(|sess| {
                    sess.draft = None;
                    sess.awaiting = None;
                });
                ctx.render(&(s.free_order_done)(&order.id), Some(back_row(s)))
                    .await;
                fulfill_in_background(&ctx.state, order.id);
                return;
            }
            let existing = get_midtrans_payment(&ctx.state.db, &order.id)
                .ok()
                .flatten();
            let reuse = match &existing {
                Some(p) => {
                    p.status.as_str() == "pending"
                        && chrono::DateTime::parse_from_rfc3339(&p.expires_at)
                            .map(|dt| dt > chrono::Utc::now())
                            .unwrap_or(false)
                }
                None => false,
            };
            let (redirect_url, expires_at) = if reuse {
                let p = existing.unwrap();
                (p.redirect_url, p.expires_at)
            } else {
                let product =
                    match catalog::get_catalog_product(&ctx.state.db, &order.catalog_product_id)
                    {
                        Ok(p) => p,
                        Err(e) => {
                            tracing::error!("[bot] Midtrans gagal: {e}");
                            ctx.render(
                                &(s.midtrans_failed)(Some(&e.to_string())),
                                Some(back_row(s)),
                            )
                            .await;
                            return;
                        }
                    };
                match create_snap_transaction(
                    &ctx.state.http_client,
                    &ctx.state.cfg,
                    &order.id,
                    order.retail_total_idr,
                    product.name,
                )
                .await
                {
                    Ok(snap) => {
                        match create_midtrans_payment(
                            &ctx.state.db,
                            &order.id,
                            order.retail_total_idr,
                            &snap.token,
                            &snap.redirect_url,
                            None,
                        ) {
                            Ok(payment) => (payment.redirect_url, payment.expires_at),
                            Err(e) => {
                                tracing::error!("[bot] Midtrans gagal: {e}");
                                ctx.render(
                                    &(s.midtrans_failed)(Some(&e.to_string())),
                                    Some(back_row(s)),
                                )
                                .await;
                                return;
                            }
                        }
                    }
                    Err(e) => {
                        tracing::error!("[bot] Midtrans gagal: {e}");
                        ctx.render(
                            &(s.midtrans_failed)(Some(&e.to_string())),
                            Some(back_row(s)),
                        )
                        .await;
                        return;
                    }
                }
            };
            ctx.update_session(|sess| {
                sess.draft = None;
                sess.awaiting = None;
            });
            let mut rows: Vec<Vec<InlineKeyboardButton>> = Vec::new();
            if let Some(b) = url_btn(s.midtrans_open_button, &redirect_url) {
                rows.push(vec![b]);
            }
            rows.push(vec![cb(
                s.qris_check_button,
                &format!("midtrans_check:{}", order.id),
            )]);
            rows.push(vec![cb(s.back_menu, "menu")]);
            ctx.render(
                &(s.midtrans_link)(&order.id, order.retail_total_idr, &expires_at),
                Some(InlineKeyboardMarkup::new(rows)),
            )
            .await;
        }
        Err(err) => {
            ctx.update_session(|sess| sess.draft = Some(draft.clone()));
            show_pay(ctx, &draft, Some(&format!("⚠️ {err}"))).await;
        }
    }
}

// ---------------------------------------------------------------------------
// Dompet.
// ---------------------------------------------------------------------------

async fn show_wallet(ctx: &Ctx) {
    let s = ctx.strs();
    let w = get_wallet(&ctx.state.db, ctx.uid());
    let kb = InlineKeyboardMarkup::new(vec![
        vec![cb("➕ Isi Saldo", "topup"), cb("📜 Riwayat", "txns")],
        vec![cb(s.back_menu, "menu")],
    ]);
    ctx.render(
        &(s.wallet_title)(w.balance_idr, w.total_topup_idr, w.total_spent_idr),
        Some(kb),
    )
    .await;
}

async fn show_topup(ctx: &Ctx) {
    let s = ctx.strs();
    let mut rows: Vec<Vec<InlineKeyboardButton>> = Vec::new();
    for chunk in TOPUP_AMOUNTS.chunks(2) {
        rows.push(
            chunk
                .iter()
                .map(|a| {
                    cb(
                        &crate::i18n::rupiah(*a),
                        &format!("topup:{a}"),
                    )
                })
                .collect(),
        );
    }
    rows.push(vec![cb("✏️ Nominal lain…", "topup_custom")]);
    rows.push(vec![cb("⬅️ Saldo", "wallet"), cb(s.back_menu, "menu")]);
    ctx.render(s.topup_title, Some(InlineKeyboardMarkup::new(rows)))
        .await;
}

async fn start_topup(ctx: &Ctx, amount_idr: i64) {
    let s = ctx.strs();
    let id = ctx.uid();
    match create_topup(&ctx.state.db, id, amount_idr) {
        Ok(topup) => {
            match create_snap_transaction(
                &ctx.state.http_client,
                &ctx.state.cfg,
                &topup.id,
                topup.amount_idr,
                &format!(
                    "Isi saldo Unicodes Digital {}",
                    crate::i18n::rupiah(topup.amount_idr)
                ),
            )
            .await
            {
                Ok(snap) => {
                    match create_midtrans_payment(
                        &ctx.state.db,
                        &topup.id,
                        topup.amount_idr,
                        &snap.token,
                        &snap.redirect_url,
                        None,
                    ) {
                        Ok(payment) => {
                            let mut rows: Vec<Vec<InlineKeyboardButton>> = Vec::new();
                            if let Some(b) =
                                url_btn(s.midtrans_open_button, &snap.redirect_url)
                            {
                                rows.push(vec![b]);
                            }
                            rows.push(vec![cb(
                                s.qris_check_button,
                                &format!("midtrans_check:{}", topup.id),
                            )]);
                            rows.push(vec![cb(s.back_menu, "menu")]);
                            ctx.render(
                                &(s.midtrans_topup_created)(
                                    topup.amount_idr,
                                    &payment.expires_at,
                                ),
                                Some(InlineKeyboardMarkup::new(rows)),
                            )
                            .await;
                        }
                        Err(e) => {
                            tracing::error!("[bot] top-up Midtrans gagal: {e}");
                            ctx.render(
                                &(s.midtrans_failed)(Some(&e.to_string())),
                                Some(back_row(s)),
                            )
                            .await;
                        }
                    }
                }
                Err(e) => {
                    tracing::error!("[bot] top-up Midtrans gagal: {e}");
                    ctx.render(
                        &(s.midtrans_failed)(Some(&e.to_string())),
                        Some(back_row(s)),
                    )
                    .await;
                }
            }
        }
        Err(e) => {
            tracing::error!("[bot] top-up gagal: {e}");
            ctx.render(
                &(s.midtrans_failed)(Some(&e.to_string())),
                Some(back_row(s)),
            )
            .await;
        }
    }
}

async fn show_txns(ctx: &Ctx) {
    let s = ctx.strs();
    let txns = list_wallet_txns(&ctx.state.db, ctx.uid(), 10);
    let text = if txns.is_empty() {
        s.txns_empty.to_string()
    } else {
        let lines: Vec<String> = txns
            .iter()
            .map(|txn| {
                (s.txn_line)(&TxnLineParams {
                    kind: txn.kind.clone(),
                    amount: txn.amount_idr,
                    date: txn.created_at.chars().take(10).collect(),
                    note: txn.note.clone(),
                })
            })
            .collect();
        format!("{}\n\n{}", s.txns_title, lines.join("\n"))
    };
    let kb = InlineKeyboardMarkup::new(vec![vec![
        cb("⬅️ Saldo", "wallet"),
        cb(s.back_menu, "menu"),
    ]]);
    ctx.render(&text, Some(kb)).await;
}

// ---------------------------------------------------------------------------
// Pesanan.
// ---------------------------------------------------------------------------

async fn show_my_orders(ctx: &Ctx) {
    let s = ctx.strs();
    let orders = list_orders_by_buyer(&ctx.state.db, &format!("tg:{}", ctx.uid()), 10);
    if orders.is_empty() {
        let kb = InlineKeyboardMarkup::new(vec![
            vec![cb(s.catalog, "cat")],
            vec![cb(s.back_menu, "menu")],
        ]);
        ctx.render(s.orders_empty, Some(kb)).await;
        return;
    }
    let mut rows: Vec<Vec<InlineKeyboardButton>> = Vec::new();
    for o in &orders {
        let emoji = match o.status.as_str() {
            "delivered" => "✅",
            "failed" => "❌",
            "refunded" => "↩️",
            _ => "⏳",
        };
        rows.push(vec![cb(
            &format!(
                "{emoji} {} — {}",
                o.id,
                crate::i18n::rupiah(o.retail_total_idr)
            ),
            &format!("ord:{}", o.id),
        )]);
    }
    rows.push(vec![cb(s.back_menu, "menu")]);
    ctx.render(s.orders_title, Some(InlineKeyboardMarkup::new(rows)))
        .await;
}

async fn show_order_detail(ctx: &Ctx, order_id: &str) {
    let s = ctx.strs();
    let id = ctx.uid();
    let order = get_store_order(&ctx.state.db, &order_id.to_uppercase());
    let ok = match &order {
        Some(o) => o.buyer_ref.as_deref() == Some(&format!("tg:{id}")),
        None => false,
    };
    if !ok {
        ctx.render(s.order_not_found, Some(back_row(s))).await;
        return;
    }
    let order = order.unwrap();
    let kb = InlineKeyboardMarkup::new(vec![vec![
        cb("⬅️ Pesanan Saya", "orders"),
        cb(s.back_menu, "menu"),
    ]]);
    ctx.render(&order_detail_text(s, &order), Some(kb)).await;
}

// ---------------------------------------------------------------------------
// Referral / promo / support / bahasa.
// ---------------------------------------------------------------------------

fn percent_encode(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

async fn show_referral(ctx: &Ctx) {
    let s = ctx.strs();
    let id = ctx.uid();
    let stats = get_referral_stats(&ctx.state.db, id);
    let link = referral_link(id);
    let share_url = format!(
        "https://t.me/share/url?url={}&text={}",
        percent_encode(&link),
        percent_encode("Belanja token Muse murah di Unicodes Digital!"),
    );
    let mut rows: Vec<Vec<InlineKeyboardButton>> = vec![vec![cb(
        "🏆 Top Referral",
        "reftop",
    )]];
    if let Some(b) = url_btn("📤 Bagikan Link", &share_url) {
        rows[0].push(b);
    }
    rows.push(vec![cb(s.back_menu, "menu")]);
    ctx.render(
        &(s.referral_title)(
            &referral_code(id),
            &link,
            stats.invited,
            stats.completed,
            stats.credit_earned,
        ),
        Some(InlineKeyboardMarkup::new(rows)),
    )
    .await;
}

async fn show_top_referrers(ctx: &Ctx) {
    let s = ctx.strs();
    let top = top_referrers(&ctx.state.db, 5);
    let text = if top.is_empty() {
        s.top_referrers_empty.to_string()
    } else {
        let lines: Vec<String> = top
            .iter()
            .enumerate()
            .map(|(i, (tg_id, invited, rewarded))| {
                (s.top_referrer_line)(i as i64 + 1, *tg_id, *invited, *rewarded)
            })
            .collect();
        format!("{}\n\n{}", s.top_referrers_title, lines.join("\n"))
    };
    let kb = InlineKeyboardMarkup::new(vec![vec![
        cb("⬅️ Referral", "ref"),
        cb(s.back_menu, "menu"),
    ]]);
    ctx.render(&text, Some(kb)).await;
}

async fn show_promo(ctx: &Ctx) {
    let s = ctx.strs();
    let kb = InlineKeyboardMarkup::new(vec![
        vec![cb("🎟️ Masukkan Kode", "promo_enter")],
        vec![cb(s.back_menu, "menu")],
    ]);
    ctx.render(s.promo_title, Some(kb)).await;
}

async fn show_support(ctx: &Ctx) {
    let s = ctx.strs();
    let kb = InlineKeyboardMarkup::new(vec![
        vec![cb("✉️ Buat Tiket Support", "support_msg")],
        vec![cb("🎫 Tiket Saya", "my_tickets")],
        vec![cb(s.back_menu, "menu")],
    ]);
    ctx.render(s.support_title, Some(kb)).await;
}

/// Awalan status untuk satu baris pilih order:
/// selesai -> ✅, gagal/refund -> ❌, sisanya -> ⏳ Diproses/Processing.
/// "Expired" tidak ada sebagai status order (hanya status pembayaran).
fn support_order_status_prefix(s: &Strings, status: StoreOrderStatus) -> String {
    match status {
        StoreOrderStatus::Delivered => "✅".to_string(),
        StoreOrderStatus::Failed | StoreOrderStatus::Refunded => "❌".to_string(),
        _ => format!("⏳ {}", s.support_order_processing),
    }
}

/// Satu baris tombol pilih order: (teks tombol, callback data).
/// Format: ikon status + ID order + produk + qty + total Rp.
fn support_order_row(s: &Strings, order: &StoreOrder, product_name: &str) -> (String, String) {
    let prefix = support_order_status_prefix(s, order.status);
    let text = (s.support_order_row)(
        &prefix,
        &order.id,
        product_name,
        order.quantity,
        order.retail_total_idr,
    );
    (text, format!("supord:{}", order.id))
}

/// Langkah pilih order sebelum buat tiket baru.
/// - Punya order -> tampilkan 5 order terakhir + "Tanpa order".
/// - Tidak punya order -> langsung minta pesan (perilaku lama).
async fn show_support_pick_order(ctx: &Ctx) {
    let s = ctx.strs();
    let orders = list_orders_by_buyer(&ctx.state.db, &format!("tg:{}", ctx.uid()), 5);
    if orders.is_empty() {
        ctx.update_session(|sess| {
            sess.awaiting = Some(AwaitingInput::Ticket { order_id: None });
        });
        ctx.render(s.support_ask, Some(back_row(s))).await;
        return;
    }
    let mut rows: Vec<Vec<InlineKeyboardButton>> = Vec::new();
    for o in &orders {
        let product_name = catalog::get_catalog_product(&ctx.state.db, &o.catalog_product_id)
            .map(|p| p.name.to_string())
            .unwrap_or_else(|_| o.catalog_product_id.clone());
        let (text, data) = support_order_row(s, o, &product_name);
        rows.push(vec![cb(&text, &data)]);
    }
    rows.push(vec![cb(s.support_pick_no_order, "supord:none")]);
    rows.push(vec![cb("⬅️ Support", "support"), cb(s.back_menu, "menu")]);
    ctx.render(
        s.support_pick_order_title,
        Some(InlineKeyboardMarkup::new(rows)),
    )
    .await;
}

/// Mulai alur support (tombol "Buat Tiket Support" / "Buat Tiket Baru").
/// Tiket terbuka -> langsung tulis pesan lanjutan (tanpa pilih order lagi);
/// kalau tidak ada tiket terbuka -> pilih order dulu, baru tulis pesan.
async fn start_support(ctx: &Ctx) {
    if get_open_ticket_by_user(&ctx.state.db, ctx.uid()).is_some() {
        ctx.update_session(|sess| {
            sess.awaiting = Some(AwaitingInput::Ticket { order_id: None });
        });
        let s = ctx.strs();
        ctx.render(s.support_ask, Some(back_row(s))).await;
        return;
    }
    show_support_pick_order(ctx).await;
}

/// Daftar tiket milik user (id + status + cuplikan pesan pertama).
async fn show_my_tickets(ctx: &Ctx) {
    let s = ctx.strs();
    let tickets = list_tickets_by_user(&ctx.state.db, ctx.uid(), 5);
    let kb = InlineKeyboardMarkup::new(vec![
        vec![cb("✉️ Buat Tiket Baru", "support_msg")],
        vec![cb(s.back_menu, "menu")],
    ]);
    if tickets.is_empty() {
        ctx.render(
            &format!("{}{}", s.my_tickets_title, s.my_tickets_empty),
            Some(kb),
        )
        .await;
        return;
    }
    // Label status di TS hardcode Bahasa Indonesia — dipertahankan persis.
    let lines: Vec<String> = tickets
        .iter()
        .map(|tk| {
            let status = match tk.status.as_str() {
                "open" => "🟢 Terbuka",
                "pending" => "🟡 Dibalas admin",
                _ => "🔒 Ditutup",
            };
            (s.my_ticket_line)(&tk.id, &tk.subject, status)
        })
        .collect();
    ctx.render(
        &format!("{}{}", s.my_tickets_title, lines.join("\n\n")),
        Some(kb),
    )
    .await;
}

async fn show_lang(ctx: &Ctx) {
    let s = ctx.strs();
    let kb = InlineKeyboardMarkup::new(vec![
        vec![
            cb("🇮🇩 Bahasa Indonesia", "lang:id"),
            cb("🇬🇧 English", "lang:en"),
        ],
        vec![cb(s.back_menu, "menu")],
    ]);
    ctx.render(s.lang_title, Some(kb)).await;
}

// ---------------------------------------------------------------------------
// Command.
// ---------------------------------------------------------------------------

/// Parse "/cmd@bot arg1 arg2" -> ("cmd", "arg1 arg2").
fn parse_command(text: &str) -> (String, String) {
    let without_slash = text.strip_prefix('/').unwrap_or(text);
    let mut parts = without_slash.splitn(2, char::is_whitespace);
    let first = parts.next().unwrap_or("");
    let name = first.split('@').next().unwrap_or("").to_string();
    let args = parts.next().unwrap_or("").trim().to_string();
    (name, args)
}

async fn handle_command(ctx: &Ctx, text: &str) -> anyhow::Result<()> {
    let (name, args) = parse_command(text);
    let s = ctx.strs();
    let id = ctx.uid();
    match name.as_str() {
        "start" => {
            // Referral: /start ref_UD-<id pengundang>
            if let Some(rest) = args.strip_prefix("ref_UD-") {
                if let Ok(referrer) = rest.trim().parse::<i64>() {
                    if referrer != id {
                        ensure_user(
                            &ctx.state.db,
                            EnsureUserInput {
                                tg_id: id,
                                first_name: Some(ctx.user.first_name.clone()),
                                username: ctx.user.username.clone(),
                                referrer_tg_id: Some(referrer),
                            },
                        );
                        record_referral(&ctx.state.db, referrer, id);
                    }
                }
            }
            show_menu(ctx).await?;
        }
        "menu" => {
            show_menu(ctx).await?;
        }
        "products" => {
            if !require_joined(ctx).await {
                return Ok(());
            }
            show_catalog(ctx, None).await?;
        }
        "support" => {
            show_support(ctx).await;
        }
        "cancel" => {
            ctx.update_session(|sess| {
                sess.awaiting = None;
                sess.draft = None;
            });
            ctx.reply(s.cancelled, None).await;
        }
        "order" => {
            if args.is_empty() {
                show_my_orders(ctx).await;
            } else {
                show_order_detail(ctx, &args).await;
            }
        }
        "fulfill" => {
            // Khusus admin — jalankan fulfillment setelah bayar terkonfirmasi.
            if !ctx.is_admin() {
                ctx.reply(s.admin_only, None).await;
                return Ok(());
            }
            let order_id = args.trim().to_uppercase();
            if order_id.is_empty() {
                ctx.reply(s.fulfill_usage, None).await;
                return Ok(());
            }
            ctx.reply(&(s.fulfill_processing)(&order_id), None).await;
            let fulfilled = match mark_paid(&ctx.state.db, &order_id) {
                Ok(_) => fulfill_order_now(&ctx.state, &order_id).await,
                Err(e) => Err(e),
            };
            match fulfilled {
                Ok(done) => {
                    ctx.reply(&order_detail_text(s, &done), None).await;
                    notify_buyer(&ctx.state.notify_ctx, &ctx.state.db, &done).await;
                }
                Err(e) => {
                    ctx.reply(&format!("Gagal: {e}"), None).await;
                }
            }
        }
        "promo_add" => {
            // /promo_add <KODE> <percent|fixed> <nilai> [max_uses] [days_valid]
            if !ctx.is_admin() {
                ctx.reply(s.admin_only, None).await;
                return Ok(());
            }
            let parts: Vec<&str> = args.split_whitespace().collect();
            let (code, kind, value_raw) = (
                parts.first().copied(),
                parts.get(1).copied(),
                parts.get(2).copied(),
            );
            let value: Option<i64> = value_raw.and_then(|v| v.parse().ok());
            let valid = matches!((code, kind, value), (Some(_), Some(k), Some(v)) if (k == "percent" || k == "fixed") && v > 0);
            if !valid {
                ctx.reply(s.promo_add_usage, None).await;
                return Ok(());
            }
            let code = code.unwrap();
            let kind = kind.unwrap();
            let value = value.unwrap();
            let max_uses = parts
                .get(3)
                .and_then(|v| v.parse::<i64>().ok())
                .map(|v| v.max(0));
            let valid_days = parts
                .get(4)
                .and_then(|v| v.parse::<i64>().ok())
                .map(|v| v.max(0));
            match create_promo_code(
                &ctx.state.db,
                CreatePromoInput {
                    code: code.to_string(),
                    kind: kind.to_string(),
                    value,
                    max_uses,
                    valid_days,
                    created_by: Some(format!("tg:{id}")),
                },
            ) {
                Ok(promo) => {
                    ctx.reply(&(s.promo_added)(&promo.code, &promo.kind, promo.value), None)
                        .await;
                }
                Err(e) => {
                    ctx.reply(&format!("Gagal membuat promo: {e}"), None).await;
                }
            }
        }
        "promo_list" => {
            if !ctx.is_admin() {
                ctx.reply(s.admin_only, None).await;
                return Ok(());
            }
            let promos = list_promo_codes(&ctx.state.db);
            if promos.is_empty() {
                ctx.reply(s.promo_list_empty, None).await;
                return Ok(());
            }
            let lines: Vec<String> = promos
                .iter()
                .map(|p| {
                    let disc = if p.kind == "percent" {
                        format!("{}%", p.value)
                    } else {
                        crate::i18n::rupiah(p.value)
                    };
                    let uses = match p.max_uses {
                        Some(m) => format!("{}/{}", p.uses_count, m),
                        None => format!("{}/∞", p.uses_count),
                    };
                    // Kata validitas di TS hardcode Bahasa Indonesia.
                    let valid = match &p.valid_until {
                        Some(until) => {
                            let expired =
                                chrono::DateTime::parse_from_rfc3339(until)
                                    .map(|dt| dt < chrono::Utc::now())
                                    .unwrap_or(false);
                            if expired {
                                "kedaluwarsa".to_string()
                            } else {
                                format!("s.d. {}", &until[..until.len().min(10)])
                            }
                        }
                        None => "tanpa batas".to_string(),
                    };
                    format!("<code>{}</code> — {disc} • dipakai {uses} • {valid}", p.code)
                })
                .collect();
            ctx.reply(
                &format!("{}\n\n{}", s.promo_list_title, lines.join("\n")),
                None,
            )
            .await;
        }
        _ => {
            ctx.reply(s.unknown_command, None).await;
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Callback query.
// ---------------------------------------------------------------------------

async fn route_callback(ctx: &Ctx, data: &str) -> anyhow::Result<()> {
    let s = ctx.strs();
    let id = ctx.uid();
    // Prefix yang paling panjang dicek dulu; pola persis regex TS.
    if data == "menu" {
        show_menu(ctx).await?;
    } else if data == "check_joined" {
        if is_channel_member(ctx, id, true).await {
            show_menu(ctx).await?;
        } else {
            show_join_screen(ctx, true).await;
        }
    } else if data == "cat" {
        if !require_joined(ctx).await {
            return Ok(());
        }
        show_catalog(ctx, None).await?;
    } else if let Some(cat) = data.strip_prefix("cat:") {
        if !require_joined(ctx).await {
            return Ok(());
        }
        let cat = if cat == "all" { "Semua" } else { cat };
        show_catalog(ctx, Some(cat)).await?;
    } else if let Some(pid) = data.strip_prefix("prod:") {
        show_product(ctx, pid).await?;
    } else if let Some(pid) = data.strip_prefix("buy:") {
        // Kompatibilitas: tombol Beli lama (buy:) -> detail produk.
        show_product(ctx, pid).await?;
    } else if let Some(rest) = data.strip_prefix("qty:") {
        // qty:<product>:<n>
        if let Some((pid, n)) = rest.rsplit_once(':') {
            let qty = n.parse::<i64>().unwrap_or(1).clamp(1, MAX_QTY);
            let draft = ctx.session().draft;
            let product = catalog::get_catalog_product(&ctx.state.db, pid)?;
            if let Some(mut d) = draft {
                if d.product_id == pid
                    && d.target.is_some()
                    && product.stock_state == StockState::Available
                {
                    d.qty = qty;
                    ctx.update_session(|sess| sess.draft = Some(d.clone()));
                    show_pay(ctx, &d, None).await;
                    return Ok(());
                }
            }
            begin_checkout(ctx, pid, qty).await?;
        }
    } else if let Some(pid) = data.strip_prefix("entercode:") {
        ask_code(ctx, pid).await;
    } else if let Some(pid) = data.strip_prefix("qtycustom:") {
        ctx.update_session(|sess| {
            sess.awaiting = Some(AwaitingInput::CustomQty {
                product_id: pid.to_string(),
            });
        });
        ctx.render(s.qty_custom_prompt, Some(back_to_catalog_row(s)))
            .await;
    } else if data == "promo_enter" {
        let draft = ctx.session().draft;
        ctx.update_session(|sess| {
            sess.awaiting = match &draft {
                Some(d) => Some(AwaitingInput::Promo { draft: d.clone() }),
                None => Some(AwaitingInput::PromoCheck),
            };
        });
        ctx.render(s.promo_enter_prompt, Some(back_row(s))).await;
    } else if data == "promo_skip" {
        match ctx.session().draft {
            Some(d) => show_pay(ctx, &d, None).await,
            None => {
                show_catalog(ctx, None).await?;
            }
        }
    } else if data == "pay_wallet" {
        if !require_joined(ctx).await {
            return Ok(());
        }
        pay_with_wallet(ctx).await;
    } else if let Some(reference) = data.strip_prefix("qris_check:") {
        check_qris_payment(ctx, reference).await;
    } else if let Some(reference) = data.strip_prefix("midtrans_check:") {
        check_midtrans_payment(ctx, reference).await;
    } else if data == "pay_qris" {
        if !require_joined(ctx).await {
            return Ok(());
        }
        pay_with_qris(ctx).await;
    } else if data == "pay_midtrans" {
        if !require_joined(ctx).await {
            return Ok(());
        }
        pay_with_midtrans(ctx).await;
    } else if data == "cancel_checkout" {
        ctx.update_session(|sess| {
            sess.draft = None;
            sess.awaiting = None;
        });
        show_menu(ctx).await?;
    } else if data == "wallet" {
        show_wallet(ctx).await;
    } else if data == "topup" {
        show_topup(ctx).await;
    } else if let Some(n) = data.strip_prefix("topup:") {
        if let Ok(amount) = n.parse::<i64>() {
            start_topup(ctx, amount).await;
        }
    } else if data == "topup_custom" {
        ctx.update_session(|sess| sess.awaiting = Some(AwaitingInput::CustomTopup));
        ctx.render(s.topup_custom_prompt, Some(back_row(s))).await;
    } else if data == "txns" {
        show_txns(ctx).await;
    } else if data == "orders" {
        show_my_orders(ctx).await;
    } else if let Some(order_id) = data.strip_prefix("ord:") {
        show_order_detail(ctx, order_id).await;
    } else if data == "ref" {
        show_referral(ctx).await;
    } else if data == "reftop" {
        show_top_referrers(ctx).await;
    } else if data == "promo" {
        show_promo(ctx).await;
    } else if data == "support" {
        show_support(ctx).await;
    } else if data == "support_msg" {
        start_support(ctx).await;
    } else if data == "supord:none" {
        ctx.update_session(|sess| {
            sess.awaiting = Some(AwaitingInput::Ticket { order_id: None });
        });
        ctx.render(s.support_ask, Some(back_row(s))).await;
    } else if let Some(order_id) = data.strip_prefix("supord:") {
        // Order harus milik user yang menekan tombol — kalau tidak,
        // fail open ke tiket tanpa order (perilaku lama).
        let id = ctx.uid();
        let oid = order_id.to_uppercase();
        let ok = get_store_order(&ctx.state.db, &oid)
            .map(|o| o.buyer_ref.as_deref() == Some(&format!("tg:{id}")))
            .unwrap_or(false);
        if ok {
            ctx.update_session(|sess| {
                sess.awaiting = Some(AwaitingInput::Ticket {
                    order_id: Some(oid.clone()),
                });
            });
            ctx.render(&(s.support_ask_with_order)(&oid), Some(back_row(s)))
                .await;
        } else {
            ctx.update_session(|sess| {
                sess.awaiting = Some(AwaitingInput::Ticket { order_id: None });
            });
            ctx.render(s.support_ask, Some(back_row(s))).await;
        }
    } else if data == "my_tickets" {
        show_my_tickets(ctx).await;
    } else if data == "policy" {
        ctx.render(s.policy_title, Some(back_row(s))).await;
    } else if data == "reseller" {
        ctx.render(s.reseller_title, Some(back_row(s))).await;
    } else if data == "lang" {
        show_lang(ctx).await;
    } else if let Some(lang) = data.strip_prefix("lang:") {
        let lang = Lang::from_str(lang);
        if let Err(e) = set_user_lang(&ctx.state.db, id, lang) {
            tracing::error!("[bot] setUserLang gagal: {e}");
        }
        let s2 = t(lang);
        ctx.reply(s2.lang_set, None).await;
        show_menu(ctx).await?;
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Pesan teks.
// Urutan penting: handler ini TERAKHIR supaya command tetap jalan saat ada
// input yang ditunggu.
// ---------------------------------------------------------------------------

async fn handle_text(ctx: &Ctx, raw_text: &str) -> anyhow::Result<()> {
    let s = ctx.strs();
    let id = ctx.uid();
    let text = raw_text.trim();

    // Perintah (/) sudah ditangani handler command — abaikan di sini.
    if text.starts_with('/') {
        ctx.reply(s.unknown_command, None).await;
        return Ok(());
    }

    let session = ctx.session();
    let Some(awaiting) = session.awaiting.clone() else {
        return Ok(()); // teks bebas tanpa konteks — abaikan
    };

    // --- Kode invite Muse (alur Sondev) ---
    if let AwaitingInput::Code { product_id } = &awaiting {
        let product_id = product_id.clone();
        let Some(code) = extract_invite_code(text) else {
            ctx.reply(s.code_not_found, None).await;
            return Ok(());
        };
        // Satu kode maks 30x penebusan. Dihitung dari order berbayar di bot
        // ini — kode yang dipakai di luar bot tidak terlihat.
        if count_code_uses(&ctx.state.db, &code) >= MAX_CODE_USES {
            ctx.reply(&(s.code_used_up)(&code), None).await;
            return Ok(()); // tetap menunggu kode lain
        }
        ctx.update_session(|sess| sess.awaiting = None);
        show_qty_after_code(ctx, &product_id, &code).await?;
        return Ok(());
    }

    // --- Target produk on-demand ---
    if let AwaitingInput::Target { draft } = &awaiting {
        let mut draft = draft.clone();
        if text.is_empty() {
            // Pesan kosong bukan target yang valid — minta ulang.
            let product = catalog::get_catalog_product(&ctx.state.db, &draft.product_id)?;
            ctx.reply(
                &(s.target_prompt)(product.target_label.unwrap_or("Target")),
                None,
            )
            .await;
            return Ok(());
        }
        ctx.update_session(|sess| sess.awaiting = None);
        // Target disimpan di draft; order dibuat saat bayar.
        draft.target = Some(text.to_string());
        ctx.update_session(|sess| sess.draft = Some(draft.clone()));
        ask_promo(ctx, &draft).await;
        return Ok(());
    }

    // --- Jumlah custom ---
    if let AwaitingInput::CustomQty { product_id } = &awaiting {
        let product_id = product_id.clone();
        let Ok(n) = text.parse::<f64>() else {
            ctx.reply(s.qty_invalid, None).await;
            return Ok(());
        };
        if !n.is_finite() || n < 1.0 || n > MAX_QTY as f64 {
            ctx.reply(s.qty_invalid, None).await;
            return Ok(());
        }
        let n = n.floor() as i64;
        ctx.update_session(|sess| sess.awaiting = None);
        // Kode sudah dimasukkan (alur Sondev) -> jumlah langsung ke ringkasan.
        let draft = ctx.session().draft;
        let product = catalog::get_catalog_product(&ctx.state.db, &product_id)?;
        if let Some(mut d) = draft {
            if d.product_id == product_id
                && d.target.is_some()
                && product.stock_state == StockState::Available
            {
                d.qty = n;
                ctx.update_session(|sess| sess.draft = Some(d.clone()));
                show_pay(ctx, &d, None).await;
                return Ok(());
            }
        }
        begin_checkout(ctx, &product_id, n).await?;
        return Ok(());
    }

    // --- Kode promo ---
    if matches!(
        awaiting,
        AwaitingInput::Promo { .. } | AwaitingInput::PromoCheck
    ) {
        let code = text
            .to_uppercase()
            .chars()
            .filter(|c| !c.is_whitespace())
            .collect::<String>();
        let is_promo = matches!(awaiting, AwaitingInput::Promo { .. });
        let draft = match &awaiting {
            AwaitingInput::Promo { draft } => Some(draft.clone()),
            _ => None,
        };
        ctx.update_session(|sess| sess.awaiting = None);
        match validate_promo(&ctx.state.db, &code, id) {
            PromoValidation::Ok(promo) => {
                if is_promo {
                    let mut draft = draft.unwrap();
                    let (subtotal, _) = draft_totals(&ctx.state.db, &draft)?;
                    draft.promo_code = Some(promo.code.clone());
                    draft.discount_idr = calc_promo_discount(&promo, subtotal);
                    let notice =
                        (s.promo_applied)(&promo.code, draft.discount_idr);
                    ctx.update_session(|sess| sess.draft = Some(draft.clone()));
                    show_pay(ctx, &draft, Some(&notice)).await;
                } else {
                    let disc = if promo.kind == "percent" {
                        format!("{}%", promo.value)
                    } else {
                        crate::i18n::rupiah(promo.value)
                    };
                    let msg = if ctx.lang() == Lang::Id {
                        format!("✅ Kode <code>{}</code> valid ({disc}). Masukkan saat checkout.", promo.code)
                    } else {
                        format!("✅ Code <code>{}</code> is valid ({disc}). Enter it at checkout.", promo.code)
                    };
                    ctx.reply(&msg, None).await;
                }
            }
            other => {
                let reason = other.reason().unwrap_or("already_used");
                ctx.reply(&(s.promo_invalid)(reason), None).await;
                if is_promo {
                    if let Some(d) = draft {
                        show_pay(ctx, &d, None).await;
                    }
                }
            }
        }
        return Ok(());
    }

    // --- Nominal isi saldo custom ---
    if matches!(awaiting, AwaitingInput::CustomTopup) {
        let cleaned: String = text
            .chars()
            .filter(|c| *c != '.' && *c != ',' && !c.is_whitespace())
            .collect();
        let n: i64 = cleaned.parse().unwrap_or(0);
        if n < MIN_TOPUP_IDR {
            ctx.reply(s.topup_invalid, None).await;
            return Ok(());
        }
        ctx.update_session(|sess| sess.awaiting = None);
        start_topup(ctx, n).await;
        return Ok(());
    }

    // --- Pesan untuk tiket support — TIDAK diteruskan ke admin.
    //  Tiket baru kalau belum ada yang terbuka; kalau ada tiket terbuka,
    //  pesan masuk ke tiket itu (status kembali 'open' = butuh admin).
    //  Order yang dipilih di langkah pilih order hanya dipakai saat tiket
    //  BARU dibuat; pesan lanjutan mengabaikannya.
    if let AwaitingInput::Ticket { order_id } = awaiting {
        ctx.update_session(|sess| sess.awaiting = None);
        match get_open_ticket_by_user(&ctx.state.db, id) {
            Some(open) => {
                add_user_message(&ctx.state.db, &open.id, text)?;
                ctx.reply(&(s.ticket_appended)(&open.id), None).await;
            }
            None => {
                let ticket = create_ticket(
                    &ctx.state.db,
                    id,
                    ctx.user.username.as_deref(),
                    Some(&ctx.user.first_name),
                    text,
                    order_id.as_deref(),
                )?;
                ctx.reply(&(s.ticket_created)(&ticket.id), None).await;
            }
        }
        return Ok(());
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Entry point dispatcher.
// ---------------------------------------------------------------------------

/// Daftarkan pengguna di DB setiap ada interaksi (upsert murah) —
//  middleware TS `bot.use(...)`. Kegagalan dicatat, tidak menggagalkan update.
fn ensure_user_for(ctx: &Ctx) {
    ensure_user(
        &ctx.state.db,
        EnsureUserInput {
            tg_id: ctx.uid(),
            first_name: Some(ctx.user.first_name.clone()),
            username: ctx.user.username.clone(),
            referrer_tg_id: None,
        },
    );
}

pub async fn handle_message(
    bot: Bot,
    msg: Message,
    state: Arc<BotState>,
) -> anyhow::Result<()> {
    let Some(user) = msg.from.clone() else {
        return Ok(());
    };
    let ctx = Ctx {
        bot,
        state,
        chat_id: msg.chat.id,
        user,
        msg_id: None,
        is_photo: false,
        is_callback: false,
    };
    ensure_user_for(&ctx);
    let Some(text) = msg.text().map(|t| t.to_string()) else {
        return Ok(());
    };
    if text.trim_start().starts_with('/') {
        handle_command(&ctx, text.trim()).await?;
    } else {
        handle_text(&ctx, &text).await?;
    }
    Ok(())
}

pub async fn handle_callback(
    bot: Bot,
    q: CallbackQuery,
    state: Arc<BotState>,
) -> anyhow::Result<()> {
    let teloxide::types::CallbackQuery {
        id,
        from: user,
        message,
        data,
        ..
    } = q;
    let (chat_id, msg_id, is_photo) = match &message {
        Some(MaybeInaccessibleMessage::Regular(m)) => {
            (m.chat.id, Some(m.id), m.photo().is_some())
        }
        Some(MaybeInaccessibleMessage::Inaccessible(m)) => {
            (m.chat.id, Some(m.message_id), false)
        }
        None => return Ok(()),
    };
    let ctx = Ctx {
        bot,
        state,
        chat_id,
        user,
        msg_id,
        is_photo,
        is_callback: true,
    };
    ctx.answer_callback(&id).await;
    ensure_user_for(&ctx);
    if let Some(data) = data {
        route_callback(&ctx, &data).await?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bot_commands_are_five_without_menu() {
        // Harness S1: setMyCommands saat startup — start/products/order/
        // support/cancel, tanpa /menu.
        assert_eq!(BOT_COMMANDS.len(), 5);
        let names: Vec<&str> = BOT_COMMANDS.iter().map(|(n, _)| *n).collect();
        assert_eq!(names, vec!["start", "products", "order", "support", "cancel"]);
        assert!(!names.contains(&"menu"));
    }

    #[test]
    fn qty_packs_match_sondev() {
        // S4: paket 1/3/5/10/30B ala Sondev; maks 30.
        assert_eq!(QTY_PACKS, [1, 3, 5, 10, 30]);
        assert_eq!(MAX_QTY, 30);
        assert_eq!(MAX_CODE_USES, 30);
    }

    #[test]
    fn support_order_rows_show_status_icons_and_format() {
        // Baris pilih order: ikon status + ID + produk + qty + total Rp.
        // delivered -> ✅; failed/refunded -> ❌; sisanya -> ⏳ Diproses.
        let db = crate::db::open(":memory:").expect("db tes");
        let seed = |id: &str, status: &str| {
            create_store_order(
                &db,
                CreateStoreOrderInput {
                    id: Some(id.to_string()),
                    catalog_product_id: "muse-token-1b".to_string(),
                    quantity: Some(5),
                    buyer_ref: Some("tg:111".to_string()),
                    xiaomi_accounts: None,
                    sheerid_url: None,
                    target: Some("UVI0QD".to_string()),
                    discount_idr: None,
                    promo_code: None,
                },
            )
            .unwrap();
            db.lock()
                .unwrap()
                .execute(
                    "UPDATE store_orders SET status = ?1 WHERE id = ?2",
                    rusqlite::params![status, id],
                )
                .unwrap();
        };
        seed("UD-DELIV1", "delivered");
        seed("UD-FAIL11", "failed");
        seed("UD-WAIT11", "awaiting_payment");
        seed("UD-PAID11", "paid");
        seed("UD-REFU11", "refunded");
        let orders = list_orders_by_buyer(&db, "tg:111", 5);
        assert_eq!(orders.len(), 5);
        let s = t(Lang::Id);
        let by_id = |id: &str| orders.iter().find(|o| o.id == id).unwrap().clone();
        let (text, data) = support_order_row(s, &by_id("UD-DELIV1"), "Muse AI Token 1B");
        assert!(text.starts_with("✅ UD-DELIV1"), "{text}");
        assert!(text.contains("Muse AI Token 1B ×5 — Rp10.000"), "{text}");
        assert_eq!(data, "supord:UD-DELIV1");
        let (text, _) = support_order_row(s, &by_id("UD-FAIL11"), "Muse AI Token 1B");
        assert!(text.starts_with("❌ UD-FAIL11"), "{text}");
        let (text, _) = support_order_row(s, &by_id("UD-WAIT11"), "Muse AI Token 1B");
        assert!(text.starts_with("⏳ Diproses UD-WAIT11"), "{text}");
        let (text, _) = support_order_row(s, &by_id("UD-PAID11"), "Muse AI Token 1B");
        assert!(text.starts_with("⏳ Diproses UD-PAID11"), "{text}");
        let (text, _) = support_order_row(s, &by_id("UD-REFU11"), "Muse AI Token 1B");
        assert!(text.starts_with("❌ UD-REFU11"), "{text}");
        // EN: "Diproses" -> "Processing".
        let s_en = t(Lang::En);
        let (text, _) = support_order_row(s_en, &by_id("UD-WAIT11"), "Muse AI Token 1B");
        assert!(text.starts_with("⏳ Processing UD-WAIT11"), "{text}");
        let (text, _) = support_order_row(s_en, &by_id("UD-DELIV1"), "Muse AI Token 1B");
        assert!(text.starts_with("✅ UD-DELIV1"), "{text}");
    }

    #[test]
    fn parse_command_splits_name_and_args() {
        assert_eq!(parse_command("/start"), ("start".to_string(), String::new()));
        assert_eq!(
            parse_command("/start ref_UD-222"),
            ("start".to_string(), "ref_UD-222".to_string())
        );
        assert_eq!(
            parse_command("/order UD-ABC123"),
            ("order".to_string(), "UD-ABC123".to_string())
        );
        // Mention @bot diabaikan; spasi ganda dipangkas.
        assert_eq!(
            parse_command("/start@unicodesdigital_bot ref_UD-222"),
            ("start".to_string(), "ref_UD-222".to_string())
        );
        assert_eq!(
            parse_command("/promo_add  TEST10 percent 10 5 30"),
            ("promo_add".to_string(), "TEST10 percent 10 5 30".to_string())
        );
        assert_eq!(parse_command("/cancel"), ("cancel".to_string(), String::new()));
    }
}
