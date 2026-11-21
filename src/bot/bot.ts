// Bot Telegram Unicodes Digital — bot toko proper (v2).
// Etalase + checkout + dompet + referral + promo + support, mencontoh
// Sondev Store Bot & Gemini-ChatGPT Bot (lihat screenshot referensi Dim).
//
// Alur order SAMA dengan web (src/lib) — bot di sini cuma antarmuka chat.
//   /start -> menu -> katalog -> detail -> jumlah -> target (bila wajib)
//   -> promo (opsional) -> bayar (saldo / QRIS dinamis) -> fulfill ->
//   hasil dikirim bot ke chat pembeli (src/lib/notify.ts).
//
// Jalankan: npm run bot (long polling; TELEGRAM_BOT_TOKEN di .env.local)
// Token: @BotFather -> /newbot. Admin IDs: /getid atau @userinfobot.
import { sequentialize } from "@grammyjs/runner";
import { Bot, InlineKeyboard, InputFile, type Context } from "grammy";
import {
  getCatalogProduct,
  listActiveProducts,
  listActiveProductsByCategory,
} from "../lib/catalog";
import {
  countCodeUses,
  createStoreOrder,
  fulfillOrder,
  getBuyerStats,
  getStoreOrder,
  getStoreStats,
  listOrdersByBuyer,
  markPaid,
  newOrderId,
  setOrderPaidVia,
  type StoreOrder,
} from "../lib/orders";
import {
  addUserMessage,
  createTicket,
  getOpenTicketByUser,
  listTicketsByUser,
} from "../lib/support-tickets";
import { createQrisPayment, getQrisPaymentByReference } from "../lib/payments/qris";
import { tickQrisPayments } from "../lib/payments/qris-poller";
import { renderQrisCardOrPlain } from "../lib/payments/qris-card";
import { getGopayStaticQris } from "../lib/payments/qris-settings";
import { notifyBuyer } from "../lib/notify";
import {
  MIN_TOPUP_IDR,
  TOPUP_AMOUNTS,
  calcPromoDiscount,
  createPromoCode,
  createTopup,
  debitWallet,
  ensureUser,
  getReferralStats,
  getTopup,
  getUser,
  getWallet,
  listPromoCodes,
  listWalletTxns,
  maybeRewardReferrer,
  recordReferral,
  redeemPromo,
  referralCode,
  referralLink,
  setUserLang,
  topReferrers,
  validatePromo,
  type Lang,
} from "../lib/wallet";
import { rupiah, t, type Strings } from "./i18n";
import { fixedFetch } from "./fixed-fetch";
import { cancelAwaiting, getSession, type CheckoutDraft } from "./session";
import { getSupplier } from "../lib/suppliers";
import { getWebAppUrl } from "./menu-button";
import { telegramAdminIds, telegramBotToken, telegramChannelId, telegramChannelUrl, midtransConfigured, productBannerFileId } from "../lib/config";
import {
  createMidtransPayment,
  createSnapTransaction,
  getMidtransPayment,
} from "../lib/payments/midtrans";
import { tickMidtransPayments } from "../lib/payments/midtrans-poller";

const QTY_PACKS = [1, 3, 5, 10, 30];

/** Support lewat TIKET (2026-11-21): forward pesan langsung ke Telegram
 *  admin DIMATIKAN permanen — nomor pribadi Dim tidak boleh tampil ke user.
 *  User buat tiket dari bot, admin menjawab dari web admin (/admin/tickets),
 *  balasan diteruskan ke user via Bot API. Lihat src/lib/support-tickets.ts. */

/** Shortcut command di menu "/" Telegram. /menu TIDAK didaftarkan (dobel
 *  dengan /start) — handler /menu di bawah tetap ada buat yang mengetik
 *  manual. Didaftarkan sekali saat startup (lihat createBot). */
const BOT_COMMANDS: { command: string; description: string }[] = [
  { command: "start", description: "Menu utama toko" },
  { command: "products", description: "Lihat katalog produk" },
  { command: "order", description: "Lacak pesanan" },
  { command: "support", description: "Buka tiket support" },
  { command: "cancel", description: "Batalkan aksi" },
];

function adminIds(): string[] {
  return telegramAdminIds().map(String);
}

function isAdmin(ctx: Context): boolean {
  return !!ctx.from && adminIds().includes(String(ctx.from.id));
}

function uid(ctx: Context): number | null {
  return ctx.from?.id ?? null;
}

function langOf(tgId: number): Lang {
  return getUser(tgId)?.lang ?? "id";
}

function sOf(tgId: number): Strings {
  return t(langOf(tgId));
}

// ------------------------------------------------------------ keyboard ---

function menuKeyboard(lang: Lang): InlineKeyboard {
  const s = t(lang);
  const kb = new InlineKeyboard();
  const webAppUrl = getWebAppUrl();
  if (webAppUrl) kb.webApp("🌐 Buka Toko", webAppUrl).row();
  // Menu ala Lhie (2026-11-21): Lihat Produk penuh, Saldo | Pesanan,
  // Bahasa | Kebijakan, lalu 💬 Support (tiket — bukan forward ke admin).
  // API (Reseller & API) DISEMBUNYIKAN dulu (handler-nya tetap ada).
  // Tombol Ikuti Channel hanya muncul kalau channel sudah dikonfigurasi.
  kb.text(s.catalog, "cat").row();
  kb.text(s.myBalance, "wallet").text(s.myOrders, "orders").row();
  kb.text(s.language, "lang").text(s.policy, "policy").row();
  kb.text(s.support, "support").row();
  const channelUrl = telegramChannelUrl();
  if (channelUrl) kb.url(s.followChannel, channelUrl).row();
  return kb;
}

function backRow(lang: Lang): InlineKeyboard {
  return new InlineKeyboard().text(t(lang).backMenu, "menu");
}

function backToCatalogRow(lang: Lang): InlineKeyboard {
  const s = t(lang);
  return new InlineKeyboard().text("⬅️ Katalog", "cat").text(s.backMenu, "menu");
}

// -------------------------------------------------------------- render ---

/** Tampilkan layar: edit pesan (kalau dari tombol) atau kirim baru.
 *  Pesan FOTO tidak bisa diedit teksnya pakai editMessageText (Telegram
 *  menolak: "there is no text in the message to edit") — untuk pesan foto
 *  pakai editMessageCaption, supaya navigasi tetap 1 pesan, tidak menumpuk
 *  pesan baru di bawah banner. */
async function render(
  ctx: Context,
  text: string,
  keyboard?: InlineKeyboard,
): Promise<void> {
  const opts = {
    parse_mode: "HTML" as const,
    reply_markup: keyboard,
    disable_web_page_preview: true,
  };
  if (ctx.callbackQuery?.message) {
    try {
      const msg = ctx.callbackQuery.message;
      if ("photo" in msg && msg.photo?.length) {
        // editMessageCaption grammY: caption masuk di objek opsi.
        await ctx.editMessageCaption({
          caption: text,
          parse_mode: "HTML",
          reply_markup: keyboard,
        });
      } else {
        await ctx.editMessageText(text, opts);
      }
      return;
    } catch {
      // Pesan tidak bisa diedit (terlalu lama / isi sama) — kirim baru saja.
    }
  }
  await ctx.reply(text, opts);
}

async function answerCb(ctx: Context): Promise<void> {
  try {
    await ctx.answerCallbackQuery();
  } catch {
    // Callback kedaluwarsa — abaikan.
  }
}

/** Wajib-join channel komunitas (Dim: sebelum pakai bot harus join dulu).
 *  - Channel belum dikonfigurasi (TELEGRAM_CHANNEL_ID kosong) = gate mati,
 *    bot jalan normal (fail open).
 *  - Hasil cek di-cache 10 menit per user; tombol "Saya Sudah Join" cek ulang.
 *  - API error = fail open (jangan blokir pembeli gara-gara gangguan API),
 *    tapi log keras supaya misconfig ketahuan. */
const CHANNEL_JOIN_CACHE_MS = 10 * 60 * 1000;
const channelJoinCache = new Map<number, { joined: boolean; at: number }>();

async function isChannelMember(
  ctx: Context,
  tgId: number,
  force = false,
): Promise<boolean> {
  const channelId = telegramChannelId();
  if (!channelId) return true;
  if (!force) {
    const cached = channelJoinCache.get(tgId);
    if (cached && Date.now() - cached.at < CHANNEL_JOIN_CACHE_MS) {
      return cached.joined;
    }
  }
  let joined = true; // fail open saat API error
  try {
    const member = await ctx.api.getChatMember(channelId, tgId);
    joined = ["creator", "administrator", "member"].includes(member.status);
  } catch (err) {
    console.error(
      "[join-gate] getChatMember gagal (fail open):",
      err instanceof Error ? err.message : String(err),
    );
  }
  channelJoinCache.set(tgId, { joined, at: Date.now() });
  return joined;
}

/** Layar wajib-join: tombol Gabung (link channel) + Saya Sudah Join (cek ulang). */
async function showJoinScreen(ctx: Context, notYet = false): Promise<void> {
  const id = uid(ctx);
  if (id == null) return;
  const s = sOf(id);
  const url = telegramChannelUrl();
  const kb = new InlineKeyboard();
  if (url) kb.url(s.joinChannelButton, url).row();
  kb.text(s.joinChannelCheckButton, "check_joined");
  await render(
    ctx,
    notYet ? `${s.joinChannelText}\n\n${s.joinChannelNotYet}` : s.joinChannelText,
    kb,
  );
}

/** Gate: true = boleh lanjut; false = layar join sudah ditampilkan. */
async function requireJoined(ctx: Context): Promise<boolean> {
  const id = uid(ctx);
  if (id == null) return false;
  if (await isChannelMember(ctx, id)) return true;
  await showJoinScreen(ctx);
  return false;
}

// ---------------------------------------------------------------- menu ---

/** Jam WIB (0-23) untuk sapaan waktu di /start. */
function wibHour(date = new Date()): number {
  const h = Number(
    new Intl.DateTimeFormat("en-US", {
      timeZone: "Asia/Jakarta",
      hour: "numeric",
      hour12: false,
    }).format(date),
  );
  return h % 24;
}

async function showMenu(ctx: Context): Promise<void> {
  const id = uid(ctx);
  if (id == null) return;
  if (!(await requireJoined(ctx))) return;
  const s = sOf(id);
  const stats = await getBuyerStats(`tg:${id}`);
  const wallet = getWallet(id);
  const store = getStoreStats();
  const text = s.welcome({
    name: ctx.from?.first_name ?? "teman",
    tgId: id,
    username: ctx.from?.username,
    totalSpentIdr: stats.totalSpentIdr,
    balanceIdr: wallet.balanceIdr,
    totalUsers: store.totalUsers,
    deliveredOrders: store.deliveredOrders,
    channelUrl: telegramChannelUrl(),
    hourWib: wibHour(),
  });
  const kb = menuKeyboard(langOf(id));
  // Home dari command (/start, /menu): kirim banner sebagai foto.
  // Dari tombol (callback) tetap edit teks — supaya chat tidak dipenuhi
  // foto setiap kali kembali ke menu. Kalau env kosong / foto gagal,
  // fallback ke balasan teks biasa — bot tidak boleh diam gara-gara banner.
  if (!ctx.callbackQuery) {
    const fileId = (process.env.BOT_BANNER_FILE_ID ?? "").trim();
    if (fileId) {
      try {
        await ctx.replyWithPhoto(fileId, {
          caption: text,
          parse_mode: "HTML",
          reply_markup: kb,
        });
        return;
      } catch (err) {
        console.error("[bot] kirim banner gagal, fallback teks:", err);
      }
    }
  }
  await render(ctx, text, kb);
}

/** Katalog BOT — fokus Sondev/Muse saja (2026-11-21): produk supplier lain
 *  (Gemini/LahaStore) DISEMBUNYIKAN dari bot. Tanpa kategori = langsung
 *  daftar produk kalau cuma ada 1 kategori; dengan kategori = daftar produk
 *  kategori itu ("Semua" = semua produk Sondev). Web tidak berubah. */
async function showCatalog(ctx: Context, category?: string): Promise<void> {
  const id = uid(ctx);
  if (id == null) return;
  const s = sOf(id);
  const sondevCats = [
    ...new Set(
      listActiveProducts()
        .filter((p) => p.supplier === "sondev")
        .map((p) => p.category),
    ),
  ];
  if (!category) {
    if (sondevCats.length === 1) {
      await showCatalog(ctx, sondevCats[0]);
      return;
    }
    const kb = new InlineKeyboard();
    for (const cat of sondevCats) {
      kb.text(cat, `cat:${cat}`).row();
    }
    kb.text(s.categoryAll, "cat:all").row();
    kb.text(s.backMenu, "menu");
    await render(ctx, s.catalogPickCategory, kb);
    return;
  }
  const kb = new InlineKeyboard();
  for (const p of listActiveProductsByCategory(category).filter(
    (p) => p.supplier === "sondev",
  )) {
    const emoji =
      p.stockState === "available" ? "✅" : p.stockState === "out_of_stock" ? "🔴" : "⏳";
    const suffix =
      p.stockState === "available"
        ? rupiah(p.retailPriceIdr)
        : s.stockLabel(p.stockState);
    kb.text(`${emoji} ${p.name} — ${suffix}`, `prod:${p.id}`).row();
  }
  kb.text("⬅️ Kategori", "cat").text(s.backMenu, "menu");
  await render(ctx, s.catalogTitle, kb);
}

async function showProduct(ctx: Context, productId: string): Promise<void> {
  const id = uid(ctx);
  if (id == null) return;
  const s = sOf(id);
  const product = getCatalogProduct(productId);
  const kb = new InlineKeyboard();
  if (product.stockState !== "available") {
    // Format stok habis sudah membawa nama produk sendiri — jangan dobel.
    if (product.stockState === "out_of_stock") {
      await render(ctx, s.outOfStock(product.name), backToCatalogRow(langOf(id)));
      return;
    }
    await render(
      ctx,
      `${product.name}\n\n${s.comingSoon}`,
      backToCatalogRow(langOf(id)),
    );
    return;
  }
  if (product.id === "muse-token-1b") {
    // Ala Sondev: kode invite DULU, baru pilih jumlah — paket qty tidak
    // ditampilkan di detail; muncul setelah kode diterima.
    kb.text("⚡ Masukkan Kode Invite", `entercode:${product.id}`).row();
  } else {
    kb.text(`🛒 Beli 1 — ${rupiah(product.retailPriceIdr)}`, `qty:${product.id}:1`).row();
  }
  kb.text("⬅️ Katalog", "cat").text(s.backMenu, "menu");
  const caption = s.productDetail({
    name: product.name,
    desc: product.description,
    price: product.retailPriceIdr,
    unit: product.unit,
    badge: product.badge,
    features: product.features ?? [],
    stock: s.stockLabel(product.stockState),
    targetNote: product.requiresTarget
      ? "Token dikirim ke akun Muse kamu — masukkan kode invite dulu (buka aplikasi Muse → Invite → salin kode/linknya)."
      : undefined,
  });
  // Detail produk ala Lhie: banner foto produk + caption (tetap 1 pesan).
  // Pesan saat ini foto -> ganti medianya; pesan teks -> hapus lalu kirim
  // foto baru (editMessageMedia pada pesan teks DITOLAK Telegram).
  // Kalau banner tidak ada / gagal -> fallback teks biasa.
  const bannerFileId = productBannerFileId(product.id);
  if (bannerFileId) {
    try {
      const msg = ctx.callbackQuery?.message;
      if (msg && "photo" in msg && msg.photo?.length) {
        await ctx.editMessageMedia(
          {
            type: "photo",
            media: bannerFileId,
            caption,
            parse_mode: "HTML",
          },
          { reply_markup: kb },
        );
        return;
      }
      if (msg) {
        try {
          await ctx.deleteMessage();
        } catch {
          // Pesan sudah tidak ada / tidak bisa dihapus — abaikan.
        }
      }
      await ctx.replyWithPhoto(bannerFileId, {
        caption,
        parse_mode: "HTML",
        reply_markup: kb,
      });
      return;
    } catch (err) {
      console.error("[bot] banner produk gagal, fallback teks:", err);
      // Lanjut ke fallback teks di bawah — jangan diam.
    }
  }
  await render(ctx, caption, kb);
}

// ------------------------------------------------------------ checkout ---

/**
 * Baca kode invite dari apa pun yang dikirim pembeli — ala bot Sondev:
 * kode polos ("UVI0QD"), link invite lengkap, atau teks bagikan dari Muse
 * (bot otomatis mendeteksi kodenya). Yang dikirim ke Sondev sebagai target
 * adalah KODEnya (docs: target = invite code, cth "ABCD1234EFGH").
 */
export function extractInviteCode(text: string): string | null {
  const t = text.trim();
  if (!t) return null;
  const fromLink = /muse\.ai\/invite\/([A-Za-z0-9_-]+)/i.exec(t);
  if (fromLink) return fromLink[1].toUpperCase();
  // Kode polos: satu token 4–12 karakter alfanumerik. Kode Muse TIDAK
  // selalu mengandung angka — "TZVXJR" (huruf semua) adalah kode valid
  // (ditemukan saat tes Dim 2026-11-21). Format saja tidak bisa membedakan
  // kata asal-asalan ("hello") dari kode huruf-semua — kode mati baru
  // ketahuan saat Sondev menebus (lalu refund).
  if (/^[A-Za-z0-9_-]{4,12}$/.test(t)) {
    return t.toUpperCase();
  }
  // Teks bagikan: cari kode hanya kalau teksnya memang konteks bagikan
  // (mengandung "invite"/"kode"/link Muse). Kalau tidak, kata biasa
  // seperti "halo dunia" bukan kode — "HALO" sendiri lolos sebagai token.
  if (!/invite|kode|muse\.ai/i.test(t)) return null;
  const stop = new Set(["MUSE", "HTTPS", "HTTP", "INVITE", "TOKEN", "TOKENS", "COM", "WWW", "AI"]);
  const tokens = t.toUpperCase().match(/[A-Z0-9]{4,12}/g) ?? [];
  const found = tokens.find((tok) => !stop.has(tok));
  return found ?? null;
}

/** Minta kode invite (langkah 1 alur Sondev) + simpan state menunggu. */
async function askCode(ctx: Context, productId: string): Promise<void> {
  const id = uid(ctx);
  if (id == null) return;
  const s = sOf(id);
  getSession(id).awaiting = { kind: "code", productId };
  await render(ctx, s.codePrompt, backRow(langOf(id)));
}

/**
 * Kode diterima: tampilkan pilihan jumlah ala Sondev (harga dasar + stok
 * live dari /stock; gagal baca stok = "Tersedia", jangan blokir checkout).
 * Draft dibuat di sini — jumlah diisi saat pembeli tap paket.
 */
async function showQtyAfterCode(
  ctx: Context,
  productId: string,
  code: string,
): Promise<void> {
  const id = uid(ctx);
  if (id == null) return;
  const s = sOf(id);
  const product = getCatalogProduct(productId);
  const session = getSession(id);
  session.draft = { productId, qty: 0, target: code, discountIdr: 0 };
  let stockLabel = s.stockLabel("available");
  try {
    const stock = await getSupplier(product.supplier).getStock(
      product.supplierProductCode,
    );
    if (typeof stock === "number") stockLabel = `${stock}B`;
  } catch {
    // Stok tidak terbaca — tampilkan "Tersedia", checkout tetap jalan.
  }
  const kb = new InlineKeyboard();
  for (let i = 0; i < QTY_PACKS.length; i += 2) {
    const a = QTY_PACKS[i];
    const b = QTY_PACKS[i + 1];
    if (b === undefined) {
      kb.text(
        `${a}B (Maks) (${rupiah(product.retailPriceIdr * a)})`,
        `qty:${product.id}:${a}`,
      ).row();
    } else {
      kb.text(
        `${a}B (${rupiah(product.retailPriceIdr * a)})`,
        `qty:${product.id}:${a}`,
      ).text(
        `${b}B (${rupiah(product.retailPriceIdr * b)})`,
        `qty:${product.id}:${b}`,
      ).row();
    }
  }
  kb.text("✏️ Jumlah lain…", `qtycustom:${product.id}`).row();
  kb.text("⬅️ Katalog", "cat").text(s.backMenu, "menu");
  await render(
    ctx,
    s.qtyTitle({ code, price: product.retailPriceIdr, stock: stockLabel }),
    kb,
  );
}

function draftTotals(draft: CheckoutDraft): { subtotal: number; total: number } {
  const product = getCatalogProduct(draft.productId);
  const subtotal = product.retailPriceIdr * draft.qty;
  return { subtotal, total: Math.max(0, subtotal - draft.discountIdr) };
}

async function askPromo(ctx: Context, draft: CheckoutDraft): Promise<void> {
  const id = uid(ctx);
  if (id == null) return;
  const s = sOf(id);
  const kb = new InlineKeyboard()
    .text("🎟️ Masukkan Kode Promo", "promo_enter")
    .row()
    .text("➡️ Lewati", "promo_skip")
    .row()
    .text(s.backMenu, "menu");
  await render(ctx, s.promoAsk, kb);
}

async function showPay(ctx: Context, draft: CheckoutDraft, notice?: string): Promise<void> {
  const id = uid(ctx);
  if (id == null) return;
  const s = sOf(id);
  const product = getCatalogProduct(draft.productId);
  const { subtotal, total } = draftTotals(draft);
  const wallet = getWallet(id);
  const kb = new InlineKeyboard();
  if (total === 0) {
    kb.text("✅ Proses Order (Gratis)", "pay_wallet").row();
  } else {
    kb.text(`${s.payWallet} (${rupiah(total)})`, "pay_wallet").row();
    kb.text(`${s.payMidtrans} (${rupiah(total)})`, "pay_midtrans").row();
  }
  kb.text("🎟️ Pakai Promo", "promo_enter").row();
  kb.text("❌ Batal", "cancel_checkout").text(s.backMenu, "menu");
  const text = `${notice ? `${notice}\n\n` : ""}${s.payTitle({
    name: product.name,
    qty: draft.qty,
    subtotal,
    discount: draft.discountIdr,
    total,
    balance: wallet.balanceIdr,
    code: draft.target ?? undefined,
  })}`;
  await render(ctx, text, kb);
}

/** Mulai checkout: jumlah sudah dipilih. Target dulu bila wajib, lalu promo. */
async function beginCheckout(ctx: Context, productId: string, qty: number): Promise<void> {
  const id = uid(ctx);
  if (id == null) return;
  const s = sOf(id);
  const product = getCatalogProduct(productId);
  if (product.stockState !== "available") {
    if (product.stockState === "out_of_stock") {
      await render(ctx, s.outOfStock(product.name), backToCatalogRow(langOf(id)));
      return;
    }
    await render(
      ctx,
      `${product.name}\n\n${s.comingSoon}`,
      backToCatalogRow(langOf(id)),
    );
    return;
  }
  const session = getSession(id);
  const draft: CheckoutDraft = { productId, qty, discountIdr: 0 };
  session.draft = draft;
  if (product.requiresTarget) {
    // Ala Sondev: kode invite dulu (langkahnya dijelaskan di prompt).
    await askCode(ctx, productId);
    return;
  }
  await askPromo(ctx, draft);
}

/**
 * Pastikan order DB ada untuk draft ini (dipakai ulang saat ganti cara bayar
 * / retry). Promo ditukar SEBELUM order dibuat, dengan ID order yang sama.
 */
async function ensureDraftOrder(
  ctx: Context,
  draft: CheckoutDraft,
): Promise<{ order: StoreOrder } | { error: string }> {
  const id = uid(ctx);
  if (id == null) return { error: "no-user" };
  const s = sOf(id);
  if (draft.orderId) {
    const existing = await getStoreOrder(draft.orderId);
    if (existing) return { order: existing };
  }
  // Cek ulang batas 30x sebelum order dibuat — kode bisa mencapai 30x di
  // antara input kode dan bayar (dipakai orang lain / order lain). Dicek
  // sebelum promo ditukar supaya promo tidak hangus sia-sia.
  if (draft.target && countCodeUses(draft.target) >= 30) {
    return { error: s.codeUsedUp(draft.target) };
  }
  const orderId = newOrderId();
  if (draft.promoCode && !draft.promoRedeemed) {
    const redeemed = redeemPromo({
      code: draft.promoCode,
      tgId: id,
      orderId,
      discountIdr: draft.discountIdr,
    });
    if (!redeemed) {
      draft.promoCode = undefined;
      draft.discountIdr = 0;
      draft.promoRedeemed = false;
      return { error: s.promoInvalid("already_used") };
    }
    draft.promoRedeemed = true;
  }
  const order = await createStoreOrder({
    id: orderId,
    catalogProductId: draft.productId,
    quantity: draft.qty,
    buyerRef: `tg:${id}`,
    target: draft.target ?? null,
    discountIdr: draft.discountIdr,
    promoCode: draft.promoCode ?? null,
  });
  draft.orderId = order.id;
  return { order };
}

/** Jalankan fulfillment di background + kabari pembeli saat selesai. */
function fulfillInBackground(orderId: string): void {
  void fulfillOrder(orderId)
    .then((done) => notifyBuyer(done))
    .catch((err: unknown) => {
      console.error(`[bot] fulfill ${orderId} gagal:`, err);
    });
}

async function payWithWallet(ctx: Context): Promise<void> {
  const id = uid(ctx);
  if (id == null) return;
  const s = sOf(id);
  const session = getSession(id);
  const draft = session.draft;
  if (!draft) {
    await showCatalog(ctx);
    return;
  }
  const ensured = await ensureDraftOrder(ctx, draft);
  if ("error" in ensured) {
    await showPay(ctx, draft, `⚠️ ${ensured.error}`);
    return;
  }
  const order = ensured.order;
  if (order.status !== "awaiting_payment") {
    await render(ctx, s.orderDetail({
      id: order.id,
      product: order.catalogProductId,
      qty: order.quantity,
      total: order.retailTotalIdr,
      status: s.statusLabel(order.status),
      items: order.deliveredItems,
      link: order.resultLink,
      error: order.error,
    }), backRow(langOf(id)));
    return;
  }
  await setOrderPaidVia(order.id, "wallet");
  if (order.retailTotalIdr > 0) {
    const debited = debitWallet({
      tgId: id,
      amountIdr: order.retailTotalIdr,
      kind: "purchase",
      ref: order.id,
      note: getCatalogProduct(order.catalogProductId).name,
    });
    if (debited == null) {
      await showPay(ctx, draft, s.walletInsufficient);
      return;
    }
  }
  await markPaid(order.id);
  try {
    maybeRewardReferrer(id);
  } catch (err) {
    console.error("[bot] reward referral gagal:", err);
  }
  session.draft = undefined;
  session.awaiting = undefined;
  await render(
    ctx,
    order.retailTotalIdr === 0
      ? s.freeOrderDone(order.id)
      : s.walletPaid(order.id),
    backRow(langOf(id)),
  );
  fulfillInBackground(order.id);
}

/** Kirim foto QR pembayaran: kartu standee komposit (QR dinamis di tengah
 *  frame standee) kalau template ada, QR polos kalau tidak. Nominal +
 *  kedaluwarsa ada di caption, bukan di gambar. */
/** Foto QR: kartu standee Dim + QR dinamis di tengah. Referensi (id order
 *  UD-… / id top-up TOPUP-…) menempelkan tombol "Saya Sudah Bayar" di bawah
 *  foto — pembeli tap setelah bayar, bot langsung cek feed GoBiz saat itu
 *  juga (di samping poller otomatis tiap 30 detik). */
async function sendQrisPhoto(
  ctx: Context,
  qrString: string,
  caption: string,
  reference?: string,
): Promise<void> {
  const id = uid(ctx);
  const { png } = await renderQrisCardOrPlain(qrString);
  let replyMarkup: InlineKeyboard | undefined;
  if (id != null) {
    const s = sOf(id);
    const kb = new InlineKeyboard();
    if (reference) kb.text(s.qrisCheckButton, `qris_check:${reference}`).row();
    kb.text(s.backMenu, "menu");
    replyMarkup = kb;
  }
  await ctx.replyWithPhoto(new InputFile(png, "qris-unicodes-digital.png"), {
    caption,
    parse_mode: "HTML",
    reply_markup: replyMarkup,
  });
}

/** Cek manual pembayaran QRIS (tombol "Saya Sudah Bayar" di foto QR):
 *  jalankan satu tick poller sekarang, lalu laporkan status referensi itu.
 *  Throttle 10 detik per referensi supaya feed GoBiz tidak dihajar spam tap. */
const MANUAL_QRIS_CHECK_MIN_MS = 10_000;
const lastManualQrisCheck = new Map<string, number>();

async function checkQrisPayment(ctx: Context, reference: string): Promise<void> {
  const id = uid(ctx);
  if (id == null) return;
  const s = sOf(id);
  const checkKb = new InlineKeyboard()
    .text(s.qrisCheckButton, `qris_check:${reference}`)
    .row()
    .text(s.backMenu, "menu");
  const now = Date.now();
  const last = lastManualQrisCheck.get(reference) ?? 0;
  if (now - last < MANUAL_QRIS_CHECK_MIN_MS) {
    await ctx.reply(s.qrisCheckTooSoon, {
      parse_mode: "HTML",
      reply_markup: checkKb,
    });
    return;
  }
  lastManualQrisCheck.set(reference, now);
  const tick = await tickQrisPayments();
  if (!tick.ok) {
    await ctx.reply(
      tick.code === "auth_required" ? s.qrisCheckNoSession : s.qrisCheckFailed,
      { parse_mode: "HTML", reply_markup: checkKb },
    );
    return;
  }
  const payment = getQrisPaymentByReference(reference);
  if (reference.startsWith("UD-")) {
    const order = await getStoreOrder(reference);
    if (!order) {
      await ctx.reply(s.qrisCheckFailed, { parse_mode: "HTML", reply_markup: checkKb });
      return;
    }
    if (
      order.status === "paid" ||
      order.status === "processing" ||
      order.status === "delivered"
    ) {
      await ctx.reply(s.qrisCheckFoundOrder(order.id), {
        parse_mode: "HTML",
        reply_markup: backRow(langOf(id)),
      });
      return;
    }
    if (
      payment?.status === "expired" ||
      (order.status === "failed" && (order.error ?? "").includes("kedaluwarsa"))
    ) {
      await ctx.reply(s.qrisCheckExpired, {
        parse_mode: "HTML",
        reply_markup: backRow(langOf(id)),
      });
      return;
    }
    await ctx.reply(s.qrisCheckPending, {
      parse_mode: "HTML",
      reply_markup: checkKb,
    });
    return;
  }
  if (reference.startsWith("TOPUP-")) {
    const topup = getTopup(reference);
    if (!topup) {
      await ctx.reply(s.qrisCheckFailed, { parse_mode: "HTML", reply_markup: checkKb });
      return;
    }
    if (topup.status === "paid") {
      await ctx.reply(s.qrisCheckFoundTopup(topup.amountIdr), {
        parse_mode: "HTML",
        reply_markup: backRow(langOf(id)),
      });
      return;
    }
    if (payment?.status === "expired") {
      await ctx.reply(s.qrisCheckExpired, {
        parse_mode: "HTML",
        reply_markup: backRow(langOf(id)),
      });
      return;
    }
    await ctx.reply(s.qrisCheckPending, {
      parse_mode: "HTML",
      reply_markup: checkKb,
    });
    return;
  }
  await ctx.reply(s.qrisCheckFailed, { parse_mode: "HTML", reply_markup: checkKb });
}

/** Cek manual pembayaran Midtrans (tombol "Saya Sudah Bayar" di pesan link
 *  bayar) — jalankan satu tick poller Midtrans sekarang, lalu laporkan
 *  status referensi itu. Cerminan checkQrisPayment untuk jalur Midtrans. */
async function checkMidtransPayment(ctx: Context, reference: string): Promise<void> {
  const id = uid(ctx);
  if (id == null) return;
  const s = sOf(id);
  const checkKb = new InlineKeyboard()
    .text(s.qrisCheckButton, `midtrans_check:${reference}`)
    .row()
    .text(s.backMenu, "menu");
  const now = Date.now();
  const last = lastManualQrisCheck.get(reference) ?? 0;
  if (now - last < MANUAL_QRIS_CHECK_MIN_MS) {
    await ctx.reply(s.qrisCheckTooSoon, {
      parse_mode: "HTML",
      reply_markup: checkKb,
    });
    return;
  }
  lastManualQrisCheck.set(reference, now);
  if (!midtransConfigured()) {
    await ctx.reply(s.midtransCheckNoConfig, {
      parse_mode: "HTML",
      reply_markup: checkKb,
    });
    return;
  }
  const tick = await tickMidtransPayments();
  if (!tick.ok) {
    await ctx.reply(s.qrisCheckFailed, {
      parse_mode: "HTML",
      reply_markup: checkKb,
    });
    return;
  }
  const payment = getMidtransPayment(reference);
  if (reference.startsWith("UD-")) {
    const order = await getStoreOrder(reference);
    if (!order) {
      await ctx.reply(s.qrisCheckFailed, { parse_mode: "HTML", reply_markup: checkKb });
      return;
    }
    if (
      order.status === "paid" ||
      order.status === "processing" ||
      order.status === "delivered"
    ) {
      await ctx.reply(s.qrisCheckFoundOrder(order.id), {
        parse_mode: "HTML",
        reply_markup: backRow(langOf(id)),
      });
      return;
    }
    if (
      payment?.status === "expired" ||
      (order.status === "failed" && (order.error ?? "").includes("kedaluwarsa"))
    ) {
      await ctx.reply(s.qrisCheckExpired, {
        parse_mode: "HTML",
        reply_markup: backRow(langOf(id)),
      });
      return;
    }
    await ctx.reply(s.qrisCheckPending, {
      parse_mode: "HTML",
      reply_markup: checkKb,
    });
    return;
  }
  if (reference.startsWith("TOPUP-")) {
    const topup = getTopup(reference);
    if (!topup) {
      await ctx.reply(s.qrisCheckFailed, { parse_mode: "HTML", reply_markup: checkKb });
      return;
    }
    if (topup.status === "paid") {
      await ctx.reply(s.qrisCheckFoundTopup(topup.amountIdr), {
        parse_mode: "HTML",
        reply_markup: backRow(langOf(id)),
      });
      return;
    }
    if (payment?.status === "expired") {
      await ctx.reply(s.qrisCheckExpired, {
        parse_mode: "HTML",
        reply_markup: backRow(langOf(id)),
      });
      return;
    }
    await ctx.reply(s.qrisCheckPending, {
      parse_mode: "HTML",
      reply_markup: checkKb,
    });
    return;
  }
  await ctx.reply(s.qrisCheckFailed, { parse_mode: "HTML", reply_markup: checkKb });
}

/** Bayar order via QRIS dinamis GoBiz (jalur LAMA).
 *  TIDAK ditawarkan di menu sejak Midtrans jadi jalur aktif (2026-11-21);
 *  dipertahankan sebagai fallback + dipakai tes QRIS. Tombolnya (pay_qris)
 *  masih terdaftar tapi tidak muncul di layar bayar. */
async function payWithQris(ctx: Context): Promise<void> {
  const id = uid(ctx);
  if (id == null) return;
  const s = sOf(id);
  const session = getSession(id);
  const draft = session.draft;
  if (!draft) {
    await showCatalog(ctx);
    return;
  }
  const ensured = await ensureDraftOrder(ctx, draft);
  if ("error" in ensured) {
    await showPay(ctx, draft, `⚠️ ${ensured.error}`);
    return;
  }
  const order = ensured.order;
  if (order.status !== "awaiting_payment") {
    await render(ctx, s.orderDetail({
      id: order.id,
      product: order.catalogProductId,
      qty: order.quantity,
      total: order.retailTotalIdr,
      status: s.statusLabel(order.status),
      items: order.deliveredItems,
      link: order.resultLink,
      error: order.error,
    }), backRow(langOf(id)));
    return;
  }
  await setOrderPaidVia(order.id, "qris");
  if (order.retailTotalIdr === 0) {
    // Gratis berkat promo — tanpa QR.
    await markPaid(order.id);
    try {
      maybeRewardReferrer(id);
    } catch (err) {
      console.error("[bot] reward referral gagal:", err);
    }
    session.draft = undefined;
    session.awaiting = undefined;
    await render(ctx, s.freeOrderDone(order.id), backRow(langOf(id)));
    fulfillInBackground(order.id);
    return;
  }
  try {
    const staticPayload = getGopayStaticQris();
    if (!staticPayload) {
      throw new Error("QRIS statis belum dipasang di panel admin (/admin).");
    }
    const { payment, qrString } = createQrisPayment({
      reference: order.id,
      amountIdr: order.retailTotalIdr,
      staticPayload,
    });
    session.draft = undefined;
    session.awaiting = undefined;
    await sendQrisPhoto(
      ctx,
      qrString,
      s.qrisLink(order.id, order.retailTotalIdr, payment.unique_amount_idr, payment.expires_at),
      order.id,
    );
  } catch (err) {
    console.error("[bot] QRIS gagal:", err);
    await render(
      ctx,
      s.qrisFailed(err instanceof Error ? err.message : undefined),
      backRow(langOf(id)),
    );
  }
}

/** Bayar order via Midtrans Snap (QRIS) — jalur pembayaran aktif.
 *  Kalau order ini sudah punya link Midtrans pending yang belum kedaluwarsa,
 *  pakai lagi link itu (jangan buat transaksi baru dengan order_id sama). */
async function payWithMidtrans(ctx: Context): Promise<void> {
  const id = uid(ctx);
  if (id == null) return;
  const s = sOf(id);
  const session = getSession(id);
  const draft = session.draft;
  if (!draft) {
    await showCatalog(ctx);
    return;
  }
  const ensured = await ensureDraftOrder(ctx, draft);
  if ("error" in ensured) {
    await showPay(ctx, draft, `⚠️ ${ensured.error}`);
    return;
  }
  const order = ensured.order;
  if (order.status !== "awaiting_payment") {
    await render(ctx, s.orderDetail({
      id: order.id,
      product: order.catalogProductId,
      qty: order.quantity,
      total: order.retailTotalIdr,
      status: s.statusLabel(order.status),
      items: order.deliveredItems,
      link: order.resultLink,
      error: order.error,
    }), backRow(langOf(id)));
    return;
  }
  await setOrderPaidVia(order.id, "midtrans");
  if (order.retailTotalIdr === 0) {
    // Gratis berkat promo — tanpa bayar.
    await markPaid(order.id);
    try {
      maybeRewardReferrer(id);
    } catch (err) {
      console.error("[bot] reward referral gagal:", err);
    }
    session.draft = undefined;
    session.awaiting = undefined;
    await render(ctx, s.freeOrderDone(order.id), backRow(langOf(id)));
    fulfillInBackground(order.id);
    return;
  }
  try {
    const existing = getMidtransPayment(order.id);
    let redirectUrl = existing?.redirect_url;
    let expiresAt = existing?.expires_at;
    if (
      !existing ||
      existing.status !== "pending" ||
      new Date(existing.expires_at).getTime() <= Date.now()
    ) {
      const product = getCatalogProduct(order.catalogProductId);
      const snap = await createSnapTransaction({
        orderId: order.id,
        amountIdr: order.retailTotalIdr,
        itemName: product.name,
      });
      const payment = createMidtransPayment({
        orderId: order.id,
        amountIdr: order.retailTotalIdr,
        snapToken: snap.token,
        redirectUrl: snap.redirectUrl,
      });
      redirectUrl = payment.redirect_url;
      expiresAt = payment.expires_at;
    }
    session.draft = undefined;
    session.awaiting = undefined;
    const kb = new InlineKeyboard()
      .url(s.midtransOpenButton, redirectUrl ?? "")
      .row()
      .text(s.qrisCheckButton, `midtrans_check:${order.id}`)
      .row()
      .text(s.backMenu, "menu");
    await render(
      ctx,
      s.midtransLink(order.id, order.retailTotalIdr, expiresAt ?? ""),
      kb,
    );
  } catch (err) {
    console.error("[bot] Midtrans gagal:", err);
    await render(
      ctx,
      s.midtransFailed(err instanceof Error ? err.message : undefined),
      backRow(langOf(id)),
    );
  }
}

// -------------------------------------------------------------- wallet ---

async function showWallet(ctx: Context): Promise<void> {
  const id = uid(ctx);
  if (id == null) return;
  const s = sOf(id);
  const w = getWallet(id);
  const kb = new InlineKeyboard()
    .text("➕ Isi Saldo", "topup")
    .text("📜 Riwayat", "txns")
    .row()
    .text(s.backMenu, "menu");
  await render(
    ctx,
    s.walletTitle(w.balanceIdr, w.totalTopupIdr, w.totalSpentIdr),
    kb,
  );
}

async function showTopup(ctx: Context): Promise<void> {
  const id = uid(ctx);
  if (id == null) return;
  const s = sOf(id);
  const kb = new InlineKeyboard();
  for (let i = 0; i < TOPUP_AMOUNTS.length; i += 2) {
    const a = TOPUP_AMOUNTS[i];
    const b = TOPUP_AMOUNTS[i + 1];
    kb.text(rupiah(a), `topup:${a}`);
    if (b !== undefined) kb.text(rupiah(b), `topup:${b}`);
    kb.row();
  }
  kb.text("✏️ Nominal lain…", "topup_custom").row();
  kb.text("⬅️ Saldo", "wallet").text(s.backMenu, "menu");
  await render(ctx, s.topupTitle, kb);
}

async function startTopup(ctx: Context, amountIdr: number): Promise<void> {
  const id = uid(ctx);
  if (id == null) return;
  const s = sOf(id);
  try {
    const topup = createTopup(id, amountIdr);
    const snap = await createSnapTransaction({
      orderId: topup.id,
      amountIdr: topup.amountIdr,
      itemName: `Isi saldo Unicodes Digital ${rupiah(topup.amountIdr)}`,
    });
    const payment = createMidtransPayment({
      orderId: topup.id,
      amountIdr: topup.amountIdr,
      snapToken: snap.token,
      redirectUrl: snap.redirectUrl,
    });
    const kb = new InlineKeyboard()
      .url(s.midtransOpenButton, snap.redirectUrl)
      .row()
      .text(s.qrisCheckButton, `midtrans_check:${topup.id}`)
      .row()
      .text(s.backMenu, "menu");
    await render(
      ctx,
      s.midtransTopupCreated(topup.amountIdr, payment.expires_at),
      kb,
    );
  } catch (err) {
    console.error("[bot] top-up Midtrans gagal:", err);
    await render(
      ctx,
      s.midtransFailed(err instanceof Error ? err.message : undefined),
      backRow(langOf(id)),
    );
  }
}

async function showTxns(ctx: Context): Promise<void> {
  const id = uid(ctx);
  if (id == null) return;
  const s = sOf(id);
  const txns = listWalletTxns(id, 10);
  const text =
    txns.length === 0
      ? s.txnsEmpty
      : `${s.txnsTitle}\n\n${txns
          .map((txn) =>
            s.txnLine({
              kind: txn.kind,
              amount: txn.amountIdr,
              date: txn.createdAt.slice(0, 10),
              note: txn.note,
            }),
          )
          .join("\n")}`;
  const kb = new InlineKeyboard()
    .text("⬅️ Saldo", "wallet")
    .text(s.backMenu, "menu");
  await render(ctx, text, kb);
}

// -------------------------------------------------------------- orders ---

async function showMyOrders(ctx: Context): Promise<void> {
  const id = uid(ctx);
  if (id == null) return;
  const s = sOf(id);
  const orders = await listOrdersByBuyer(`tg:${id}`, 10);
  if (orders.length === 0) {
    const kb = new InlineKeyboard()
      .text(s.catalog, "cat")
      .row()
      .text(s.backMenu, "menu");
    await render(ctx, s.ordersEmpty, kb);
    return;
  }
  const kb = new InlineKeyboard();
  for (const o of orders) {
    const emoji =
      o.status === "delivered" ? "✅" : o.status === "failed" ? "❌" : o.status === "refunded" ? "↩️" : "⏳";
    kb.text(`${emoji} ${o.id} — ${rupiah(o.retailTotalIdr)}`, `ord:${o.id}`).row();
  }
  kb.text(s.backMenu, "menu");
  await render(ctx, s.ordersTitle, kb);
}

async function showOrderDetail(ctx: Context, orderId: string): Promise<void> {
  const id = uid(ctx);
  if (id == null) return;
  const s = sOf(id);
  const order = await getStoreOrder(orderId.toUpperCase());
  if (!order || order.buyerRef !== `tg:${id}`) {
    await render(ctx, s.orderNotFound, backRow(langOf(id)));
    return;
  }
  const kb = new InlineKeyboard()
    .text("⬅️ Pesanan Saya", "orders")
    .text(s.backMenu, "menu");
  await render(
    ctx,
    s.orderDetail({
      id: order.id,
      product: order.catalogProductId,
      qty: order.quantity,
      total: order.retailTotalIdr,
      status: s.statusLabel(order.status),
      items: order.deliveredItems,
      link: order.resultLink,
      error: order.error,
    }),
    kb,
  );
}

// ------------------------------------------------------------ referral ---

async function showReferral(ctx: Context): Promise<void> {
  const id = uid(ctx);
  if (id == null) return;
  const s = sOf(id);
  const stats = getReferralStats(id);
  const link = referralLink(id);
  const shareUrl = `https://t.me/share/url?url=${encodeURIComponent(link)}&text=${encodeURIComponent(
    "Belanja token Muse murah di Unicodes Digital!",
  )}`;
  const kb = new InlineKeyboard()
    .text("🏆 Top Referral", "reftop")
    .url("📤 Bagikan Link", shareUrl)
    .row()
    .text(s.backMenu, "menu");
  await render(
    ctx,
    s.referralTitle(referralCode(id), link, stats.invited, stats.completed, stats.creditEarned),
    kb,
  );
}

async function showTopReferrers(ctx: Context): Promise<void> {
  const id = uid(ctx);
  if (id == null) return;
  const s = sOf(id);
  const top = topReferrers(5);
  const text =
    top.length === 0
      ? s.topReferrersEmpty
      : `${s.topReferrersTitle}\n\n${top
          .map((r, i) => s.topReferrerLine(i + 1, r.tgId, r.invited, r.rewarded))
          .join("\n")}`;
  const kb = new InlineKeyboard()
    .text("⬅️ Referral", "ref")
    .text(s.backMenu, "menu");
  await render(ctx, text, kb);
}

// -------------------------------------------------------------- promo ---

async function showPromo(ctx: Context): Promise<void> {
  const id = uid(ctx);
  if (id == null) return;
  const s = sOf(id);
  const kb = new InlineKeyboard()
    .text("🎟️ Masukkan Kode", "promo_enter")
    .row()
    .text(s.backMenu, "menu");
  await render(ctx, s.promoTitle, kb);
}

// ------------------------------------------------------------ support ---

async function showSupport(ctx: Context): Promise<void> {
  const id = uid(ctx);
  if (id == null) return;
  const s = sOf(id);
  const kb = new InlineKeyboard()
    .text("✉️ Buat Tiket Support", "support_msg")
    .row()
    .text("🎫 Tiket Saya", "my_tickets")
    .row()
    .text(s.backMenu, "menu");
  await render(ctx, s.supportTitle, kb);
}

/** Daftar tiket milik user (id + status + cuplikan pesan pertama). */
async function showMyTickets(ctx: Context): Promise<void> {
  const id = uid(ctx);
  if (id == null) return;
  const s = sOf(id);
  const tickets = listTicketsByUser(id, 5);
  const kb = new InlineKeyboard()
    .text("✉️ Buat Tiket Baru", "support_msg")
    .row()
    .text(s.backMenu, "menu");
  if (tickets.length === 0) {
    await render(ctx, `${s.myTicketsTitle}${s.myTicketsEmpty}`, kb);
    return;
  }
  const lines = tickets.map((tk) =>
    s.myTicketLine(
      tk.id,
      tk.subject || "(tanpa judul)",
      tk.status === "open" ? "🟢 Terbuka" : tk.status === "pending" ? "🟡 Dibalas admin" : "🔒 Ditutup",
    ),
  );
  await render(ctx, `${s.myTicketsTitle}${lines.join("\n\n")}`, kb);
}

// ------------------------------------------------------------ language ---

async function showLang(ctx: Context): Promise<void> {
  const id = uid(ctx);
  if (id == null) return;
  const s = sOf(id);
  const kb = new InlineKeyboard()
    .text("🇮🇩 Bahasa Indonesia", "lang:id")
    .text("🇬🇧 English", "lang:en")
    .row()
    .text(s.backMenu, "menu");
  await render(ctx, s.langTitle, kb);
}

export function createBot(): Bot {
  const token = telegramBotToken();
  const bot = new Bot(token, { client: { fetch: fixedFetch as typeof fetch } });

  // PARALEL antar pengguna, berurutan di dalam satu chat: update dari chat
  // yang berbeda diproses bersamaan (jadi /start satu orang tidak ngantre
  // di belakang proses lambat orang lain), tapi dua update dari orang yang
  // sama tetap berurutan — aman buat session + checkout. Dipakai bareng
  // @grammyjs/runner di run.ts; dengan bot.start() biasa pun tidak merusak.
  bot.use(sequentialize((ctx) => ctx.chat?.id.toString()));

  // Daftarkan pengguna di DB setiap ada interaksi (upsert murah).
  bot.use(async (ctx, next) => {
    if (ctx.from) {
      try {
        ensureUser({
          tgId: ctx.from.id,
          firstName: ctx.from.first_name,
          username: ctx.from.username,
        });
      } catch (err) {
        console.error("[bot] ensureUser gagal:", err);
      }
    }
    await next();
  });

  bot.command("start", async (ctx) => {
    const id = uid(ctx);
    if (id == null) return;
    // Referral: /start ref_UD-<id pengundang>
    const payload = (ctx.match ?? "").trim();
    const refMatch = /^ref_UD-(\d+)$/.exec(payload);
    if (refMatch) {
      const referrer = Number(refMatch[1]);
      if (Number.isFinite(referrer) && referrer !== id) {
        try {
          ensureUser({ tgId: id, referrerTgId: referrer });
          recordReferral(referrer, id);
        } catch (err) {
          console.error("[bot] referral gagal:", err);
        }
      }
    }
    await showMenu(ctx);
  });

  bot.command("menu", async (ctx) => {
    await showMenu(ctx);
  });

  bot.command("products", async (ctx) => {
    if (!(await requireJoined(ctx))) return;
    await showCatalog(ctx);
  });

  bot.command("support", async (ctx) => {
    await showSupport(ctx);
  });

  bot.command("cancel", async (ctx) => {
    const id = uid(ctx);
    if (id == null) return;
    cancelAwaiting(id);
    const session = getSession(id);
    session.draft = undefined;
    await ctx.reply(sOf(id).cancelled);
  });

  bot.command("order", async (ctx) => {
    const id = uid(ctx);
    if (id == null) return;
    const orderId = (ctx.match ?? "").trim();
    if (!orderId) {
      // Tanpa ID = lacak order: tampilkan daftar pesanan saya.
      await showMyOrders(ctx);
      return;
    }
    await showOrderDetail(ctx, orderId);
  });

  // Khusus admin — jalankan fulfillment setelah bayar terkonfirmasi.
  // Normalnya ini otomatis via poller QRIS (feed GoBiz -> markPaid -> fulfillOrder).
  bot.command("fulfill", async (ctx) => {
    const id = uid(ctx);
    if (id == null) return;
    const s = sOf(id);
    if (!isAdmin(ctx)) {
      await ctx.reply(s.adminOnly);
      return;
    }
    const orderId = (ctx.match ?? "").trim().toUpperCase();
    if (!orderId) {
      await ctx.reply(s.fulfillUsage);
      return;
    }
    await ctx.reply(s.fulfillProcessing(orderId));
    try {
      await markPaid(orderId);
      const done = await fulfillOrder(orderId);
      await ctx.reply(
        s.orderDetail({
          id: done.id,
          product: done.catalogProductId,
          qty: done.quantity,
          total: done.retailTotalIdr,
          status: s.statusLabel(done.status),
          items: done.deliveredItems,
          link: done.resultLink,
          error: done.error,
        }),
      );
      await notifyBuyer(done);
    } catch (err) {
      await ctx.reply(
        `Gagal: ${err instanceof Error ? err.message : String(err)}`,
      );
    }
  });

  // Admin: buat kode promo.
  // /promo_add <KODE> <percent|fixed> <nilai> [max_uses] [days_valid]
  bot.command("promo_add", async (ctx) => {
    const id = uid(ctx);
    if (id == null) return;
    const s = sOf(id);
    if (!isAdmin(ctx)) {
      await ctx.reply(s.adminOnly);
      return;
    }
    const args = (ctx.match ?? "").trim().split(/\s+/).filter(Boolean);
    const [code, kind, valueRaw, maxUsesRaw, daysValidRaw] = args;
    if (!code || (kind !== "percent" && kind !== "fixed") || !valueRaw) {
      await ctx.reply(s.promoAddUsage);
      return;
    }
    const value = Number(valueRaw);
    const maxUses = maxUsesRaw ? Number(maxUsesRaw) : null;
    const daysValid = daysValidRaw ? Number(daysValidRaw) : null;
    if (!Number.isFinite(value) || value <= 0) {
      await ctx.reply(s.promoAddUsage);
      return;
    }
    try {
      const promo = createPromoCode({
        code,
        kind,
        value,
        maxUses: maxUses != null && Number.isFinite(maxUses) ? Math.floor(maxUses) : null,
        validDays: daysValid != null && Number.isFinite(daysValid) ? Math.floor(daysValid) : null,
        createdBy: `tg:${id}`,
      });
      await ctx.reply(
        s.promoAdded(promo.code, promo.kind, promo.value),
        { parse_mode: "HTML" },
      );
    } catch (err) {
      await ctx.reply(
        `Gagal membuat promo: ${err instanceof Error ? err.message : String(err)}`,
      );
    }
  });

  bot.command("promo_list", async (ctx) => {
    const id = uid(ctx);
    if (id == null) return;
    const s = sOf(id);
    if (!isAdmin(ctx)) {
      await ctx.reply(s.adminOnly);
      return;
    }
    const promos = listPromoCodes();
    if (promos.length === 0) {
      await ctx.reply(s.promoListEmpty);
      return;
    }
    const lines = promos.map((p) => {
      const disc =
        p.kind === "percent" ? `${p.value}%` : rupiah(p.value);
      const uses = p.maxUses != null ? `${p.usesCount}/${p.maxUses}` : `${p.usesCount}/∞`;
      const valid = p.validUntil
        ? new Date(p.validUntil).getTime() < Date.now()
          ? "kedaluwarsa"
          : `s.d. ${p.validUntil.slice(0, 10)}`
        : "tanpa batas";
      return `<code>${p.code}</code> — ${disc} • dipakai ${uses} • ${valid}`;
    });
    await ctx.reply(`${s.promoListTitle}\n\n${lines.join("\n")}`, {
      parse_mode: "HTML",
    });
  });

  // ------------------------------------------------------- callback query ---

  bot.callbackQuery("menu", async (ctx) => {
    await answerCb(ctx);
    await showMenu(ctx);
  });

  bot.callbackQuery("check_joined", async (ctx) => {
    await answerCb(ctx);
    const id = uid(ctx);
    if (id == null) return;
    if (await isChannelMember(ctx, id, true)) {
      await showMenu(ctx);
    } else {
      await showJoinScreen(ctx, true);
    }
  });

  bot.callbackQuery("cat", async (ctx) => {
    await answerCb(ctx);
    if (!(await requireJoined(ctx))) return;
    await showCatalog(ctx);
  });

  bot.callbackQuery(/^cat:(.+)$/, async (ctx) => {
    await answerCb(ctx);
    if (!(await requireJoined(ctx))) return;
    const cat = ctx.match[1] === "all" ? "Semua" : ctx.match[1];
    await showCatalog(ctx, cat);
  });

  bot.callbackQuery(/^prod:(.+)$/, async (ctx) => {
    await answerCb(ctx);
    await showProduct(ctx, ctx.match[1]);
  });

  // Kompatibilitas: tombol Beli lama (buy:) -> detail produk.
  bot.callbackQuery(/^buy:(.+)$/, async (ctx) => {
    await answerCb(ctx);
    await showProduct(ctx, ctx.match[1]);
  });

  bot.callbackQuery(/^qty:(.+):(\d+)$/, async (ctx) => {
    await answerCb(ctx);
    const qty = Math.min(30, Math.max(1, Number(ctx.match[2])));
    const id = uid(ctx);
    // Kode sudah dimasukkan (alur Sondev) -> jumlah langsung ke ringkasan.
    // Produk harus tetap tersedia — kalau stok habis, jatuh ke beginCheckout
    // (pesan stok) supaya tombol qty lama tidak bisa memaksa checkout.
    if (id != null) {
      const draft = getSession(id).draft;
      const product = getCatalogProduct(ctx.match[1]);
      if (
        draft &&
        draft.productId === ctx.match[1] &&
        draft.target &&
        product.stockState === "available"
      ) {
        draft.qty = qty;
        await showPay(ctx, draft);
        return;
      }
    }
    await beginCheckout(ctx, ctx.match[1], qty);
  });

  bot.callbackQuery(/^entercode:(.+)$/, async (ctx) => {
    await answerCb(ctx);
    await askCode(ctx, ctx.match[1]);
  });

  bot.callbackQuery(/^qtycustom:(.+)$/, async (ctx) => {
    await answerCb(ctx);
    const id = uid(ctx);
    if (id == null) return;
    const s = sOf(id);
    getSession(id).awaiting = { kind: "custom_qty", productId: ctx.match[1] };
    await render(ctx, s.qtyCustomPrompt, backToCatalogRow(langOf(id)));
  });

  bot.callbackQuery("promo_enter", async (ctx) => {
    await answerCb(ctx);
    const id = uid(ctx);
    if (id == null) return;
    const s = sOf(id);
    const session = getSession(id);
    session.awaiting = session.draft
      ? { kind: "promo", draft: session.draft }
      : { kind: "promo_check" };
    const kb = new InlineKeyboard().text(s.backMenu, "menu");
    await render(ctx, s.promoEnterPrompt, kb);
  });

  bot.callbackQuery("promo_skip", async (ctx) => {
    await answerCb(ctx);
    const id = uid(ctx);
    if (id == null) return;
    const draft = getSession(id).draft;
    if (!draft) {
      await showCatalog(ctx);
      return;
    }
    await showPay(ctx, draft);
  });

  bot.callbackQuery("pay_wallet", async (ctx) => {
    await answerCb(ctx);
    if (!(await requireJoined(ctx))) return;
    await payWithWallet(ctx);
  });

  bot.callbackQuery(/^qris_check:(.+)$/, async (ctx) => {
    await answerCb(ctx);
    await checkQrisPayment(ctx, ctx.match[1]);
  });

  bot.callbackQuery(/^midtrans_check:(.+)$/, async (ctx) => {
    await answerCb(ctx);
    await checkMidtransPayment(ctx, ctx.match[1]);
  });

  bot.callbackQuery("pay_qris", async (ctx) => {
    await answerCb(ctx);
    if (!(await requireJoined(ctx))) return;
    await payWithQris(ctx);
  });

  bot.callbackQuery("pay_midtrans", async (ctx) => {
    await answerCb(ctx);
    if (!(await requireJoined(ctx))) return;
    await payWithMidtrans(ctx);
  });

  bot.callbackQuery("cancel_checkout", async (ctx) => {
    await answerCb(ctx);
    const id = uid(ctx);
    if (id == null) return;
    const session = getSession(id);
    session.draft = undefined;
    session.awaiting = undefined;
    await showMenu(ctx);
  });

  bot.callbackQuery("wallet", async (ctx) => {
    await answerCb(ctx);
    await showWallet(ctx);
  });

  bot.callbackQuery("topup", async (ctx) => {
    await answerCb(ctx);
    await showTopup(ctx);
  });

  bot.callbackQuery(/^topup:(\d+)$/, async (ctx) => {
    await answerCb(ctx);
    await startTopup(ctx, Number(ctx.match[1]));
  });

  bot.callbackQuery("topup_custom", async (ctx) => {
    await answerCb(ctx);
    const id = uid(ctx);
    if (id == null) return;
    const s = sOf(id);
    getSession(id).awaiting = { kind: "custom_topup" };
    await render(ctx, s.topupCustomPrompt, backRow(langOf(id)));
  });

  bot.callbackQuery("txns", async (ctx) => {
    await answerCb(ctx);
    await showTxns(ctx);
  });

  bot.callbackQuery("orders", async (ctx) => {
    await answerCb(ctx);
    await showMyOrders(ctx);
  });

  bot.callbackQuery(/^ord:(.+)$/, async (ctx) => {
    await answerCb(ctx);
    await showOrderDetail(ctx, ctx.match[1]);
  });

  bot.callbackQuery("ref", async (ctx) => {
    await answerCb(ctx);
    await showReferral(ctx);
  });

  bot.callbackQuery("reftop", async (ctx) => {
    await answerCb(ctx);
    await showTopReferrers(ctx);
  });

  bot.callbackQuery("promo", async (ctx) => {
    await answerCb(ctx);
    await showPromo(ctx);
  });

  bot.callbackQuery("support", async (ctx) => {
    await answerCb(ctx);
    const id = uid(ctx);
    if (id == null) return;
    await showSupport(ctx);
  });

  bot.callbackQuery("support_msg", async (ctx) => {
    await answerCb(ctx);
    const id = uid(ctx);
    if (id == null) return;
    const s = sOf(id);
    getSession(id).awaiting = { kind: "ticket" };
    await render(ctx, s.supportAsk, backRow(langOf(id)));
  });

  bot.callbackQuery("my_tickets", async (ctx) => {
    await answerCb(ctx);
    const id = uid(ctx);
    if (id == null) return;
    await showMyTickets(ctx);
  });

  bot.callbackQuery("policy", async (ctx) => {
    await answerCb(ctx);
    const id = uid(ctx);
    if (id == null) return;
    await render(ctx, sOf(id).policyTitle, backRow(langOf(id)));
  });

  bot.callbackQuery("reseller", async (ctx) => {
    await answerCb(ctx);
    const id = uid(ctx);
    if (id == null) return;
    await render(ctx, sOf(id).resellerTitle, backRow(langOf(id)));
  });

  bot.callbackQuery("lang", async (ctx) => {
    await answerCb(ctx);
    await showLang(ctx);
  });

  bot.callbackQuery(/^lang:(id|en)$/, async (ctx) => {
    await answerCb(ctx);
    const id = uid(ctx);
    if (id == null) return;
    const lang = ctx.match[1] as Lang;
    setUserLang(id, lang);
    await ctx.reply(t(lang).langSet);
    await showMenu(ctx);
  });

  // --------------------------------------------------------- pesan teks ---
  // Urutan penting: handler ini didaftarkan TERAKHIR supaya command
  // (/cancel, /order, ...) tetap jalan saat ada input yang ditunggu.
  bot.on("message:text", async (ctx) => {
    const id = uid(ctx);
    if (id == null) return;
    const s = sOf(id);
    const text = ctx.message.text.trim();

    // Perintah (/) sudah ditangani handler command di atas — abaikan di sini.
    if (text.startsWith("/")) {
      await ctx.reply(s.unknownCommand);
      return;
    }

    const session = getSession(id);
    const awaiting = session.awaiting;
    if (!awaiting) return; // teks bebas tanpa konteks — abaikan

    // --- Kode invite Muse (alur Sondev) ---
    if (awaiting.kind === "code") {
      const code = extractInviteCode(text ?? "");
      if (!code) {
        await ctx.reply(s.codeNotFound);
        return;
      }
      // Satu kode maks 30x penebusan (ala Sondev: "This code is used up").
      // Dihitung dari order berbayar di bot ini — kode yang dipakai di luar
      // bot tidak terlihat; penebusan ke-31+ tetap gagal di Sondev lalu refund.
      if (countCodeUses(code) >= 30) {
        await ctx.reply(s.codeUsedUp(code), { parse_mode: "HTML" });
        return; // tetap menunggu kode lain
      }
      session.awaiting = undefined;
      await showQtyAfterCode(ctx, awaiting.productId, code);
      return;
    }

    // --- Target produk on-demand ---
    if (awaiting.kind === "target") {
      const draft = awaiting.draft;
      if (!text) {
        // Pesan kosong (cuma spasi) bukan target yang valid — minta ulang,
        // jangan lanjut ke promo dengan target kosong (Sondev menolak 400).
        const product = getCatalogProduct(draft.productId);
        await ctx.reply(s.targetPrompt(product.targetLabel ?? "Target"));
        return;
      }
      session.awaiting = undefined;
      // Target disimpan di draft; order dibuat saat bayar (createStoreOrder
      // menerima target) — tidak ada order yang dibuat sebelum bayar di v2.
      draft.target = text;
      await askPromo(ctx, draft);
      return;
    }

    // --- Jumlah custom ---
    if (awaiting.kind === "custom_qty") {
      const n = Number(text);
      if (!Number.isFinite(n) || n < 1 || n > 30) {
        await ctx.reply(s.qtyInvalid);
        return;
      }
      session.awaiting = undefined;
      // Kode sudah dimasukkan (alur Sondev) -> jumlah langsung ke ringkasan.
      const draft = session.draft;
      const product = getCatalogProduct(awaiting.productId);
      if (
        draft &&
        draft.productId === awaiting.productId &&
        draft.target &&
        product.stockState === "available"
      ) {
        draft.qty = Math.floor(n);
        await showPay(ctx, draft);
        return;
      }
      await beginCheckout(ctx, awaiting.productId, Math.floor(n));
      return;
    }

    // --- Kode promo ---
    if (awaiting.kind === "promo" || awaiting.kind === "promo_check") {
      session.awaiting = undefined;
      const code = text.toUpperCase().replace(/\s+/g, "");
      const check = validatePromo(code, id);
      if (!check.ok) {
        await ctx.reply(s.promoInvalid(check.reason));
        if (awaiting.kind === "promo") await showPay(ctx, awaiting.draft);
        return;
      }
      const promo = check.promo;
      if (awaiting.kind === "promo") {
        const draft = awaiting.draft;
        const { subtotal } = draftTotals(draft);
        draft.promoCode = promo.code;
        draft.discountIdr = calcPromoDiscount(promo, subtotal);
        await showPay(ctx, draft, s.promoApplied(promo.code, draft.discountIdr));
      } else {
        await ctx.reply(
          `✅ Kode <code>${promo.code}</code> valid (${promo.kind === "percent" ? `${promo.value}%` : rupiah(promo.value)}). Masukkan saat checkout.`,
          { parse_mode: "HTML" },
        );
      }
      return;
    }

    // --- Nominal isi saldo custom ---
    if (awaiting.kind === "custom_topup") {
      const n = Number(text.replace(/[.,\s]/g, ""));
      if (!Number.isFinite(n) || n < MIN_TOPUP_IDR) {
        await ctx.reply(s.topupInvalid);
        return;
      }
      session.awaiting = undefined;
      await startTopup(ctx, Math.floor(n));
      return;
    }

    // --- Pesan untuk tiket support — TIDAK diteruskan ke admin.
    //  Tiket baru kalau belum ada yang terbuka; kalau ada tiket terbuka,
    //  pesan masuk ke tiket itu (status kembali 'open' = butuh admin).
    if (awaiting.kind === "ticket") {
      session.awaiting = undefined;
      const open = getOpenTicketByUser(id);
      if (open) {
        addUserMessage(open.id, text);
        await ctx.reply(s.ticketAppended(open.id), { parse_mode: "HTML" });
      } else {
        const ticket = createTicket({
          tgId: id,
          username: ctx.from?.username ?? null,
          firstName: ctx.from?.first_name ?? null,
          text,
        });
        await ctx.reply(s.ticketCreated(ticket.id), { parse_mode: "HTML" });
      }
      return;
    }
  });

  // Daftarkan shortcut command (menu "/" di Telegram) sekali saat start.
  // Kegagalan tidak boleh mematikan bot (mis. jaringan sempat putus) —
  // bot tetap jalan dengan handler command-nya; daftar command bisa
  // didaftarkan ulang di restart berikutnya.
  void bot.api.setMyCommands(BOT_COMMANDS).catch((err: unknown) => {
    console.error(
      "[bot] setMyCommands gagal:",
      err instanceof Error ? err.message : String(err),
    );
  });

  return bot;
}
