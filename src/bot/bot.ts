// Bot Telegram Unicodes Digital — etalase kedua, alur order SAMA dengan web.
// Semua logika (katalog, order, supplier) dipakai ulang dari src/lib —
// bot di sini cuma antarmuka chat di atasnya.
//
// Jalankan: npm run bot  (long polling; butuh TELEGRAM_BOT_TOKEN di .env.local)
// Token: @BotFather -> /newbot. Admin IDs: /getid atau @userinfobot.
import { Bot, InlineKeyboard } from "grammy";
import { getCatalogProduct, listActiveProducts } from "../lib/catalog";
import {
  createStoreOrder,
  fulfillOrder,
  getStoreOrder,
  markPaid,
  setOrderTarget,
  type StoreOrder,
} from "../lib/orders";
import { createSnapTransaction } from "../lib/payments/midtrans";

/** Menunggu target dari pembeli (produk on-demand Sondev): chatId -> order. */
const pendingTarget = new Map<number, { orderId: string; label: string }>();

const rupiah = (n: number) => `Rp${n.toLocaleString("id-ID")}`;

function isAdmin(ctx: { from?: { id: number } }): boolean {
  const admins = (process.env.TELEGRAM_ADMIN_IDS ?? "")
    .split(",")
    .map((s) => s.trim())
    .filter(Boolean);
  return !!ctx.from && admins.includes(String(ctx.from.id));
}

function productKeyboard(): InlineKeyboard {
  const kb = new InlineKeyboard();
  for (const p of listActiveProducts()) {
    kb.text(`${p.name} — ${rupiah(p.retailPriceIdr)}`, `buy:${p.id}`).row();
  }
  return kb;
}

function productListText(): string {
  return listActiveProducts()
    .map(
      (p) =>
        `<b>${p.name}</b> — ${rupiah(p.retailPriceIdr)}\n${p.description}\nvia ${
          p.supplier === "sondev" ? "Sondev (Muse)" : "LahaStore (AI lain)"
        }`,
    )
    .join("\n\n");
}

function orderStatusText(order: StoreOrder): string {
  const lines = [
    `Order: ${order.id}`,
    `Produk: ${order.catalogProductId} x${order.quantity}`,
    `Total: ${rupiah(order.retailTotalIdr)}`,
    `Status: ${order.status}`,
    `Supplier: ${order.supplier}`,
  ];
  if (order.deliveredItems.length) {
    lines.push("", "Hasil:", ...order.deliveredItems.map((i) => `- ${i}`));
  }
  if (order.resultLink) lines.push(`Link: ${order.resultLink}`);
  if (order.error) lines.push(`Error: ${order.error}`);
  return lines.join("\n");
}

export function createBot(): Bot {
  const token = process.env.TELEGRAM_BOT_TOKEN;
  if (!token) {
    throw new Error(
      "TELEGRAM_BOT_TOKEN belum diisi di .env.local (ambil dari @BotFather)",
    );
  }
  const bot = new Bot(token);

  bot.command("start", async (ctx) => {
    await ctx.reply(
      "Selamat datang di Unicodes Digital — reseller AI & digital.\n" +
        "Muse dipenuhi Sondev, AI lain (Gemini) dipenuhi LahaStore.\n\n" +
        "Perintah:\n/products — lihat katalog\n/order <id> — lacak order",
      { reply_markup: productKeyboard() },
    );
  });

  bot.command("products", async (ctx) => {
    await ctx.reply(productListText(), {
      reply_markup: productKeyboard(),
      parse_mode: "HTML",
    });
  });

  bot.callbackQuery(/^buy:(.+)$/, async (ctx) => {
    const productId = ctx.match[1];
    const product = getCatalogProduct(productId);
    const order = await createStoreOrder({
      catalogProductId: product.id,
      quantity: 1,
      buyerRef: `tg:${ctx.from?.id ?? "?"}`,
    });
    // Produk on-demand (Muse) WAJIB target (kode invite / email) — tanya dulu
    // sebelum membuat link bayar (docs Sondev v1.2: tanpa target -> 400 INVALID_ORDER).
    if (product.requiresTarget && ctx.from) {
      pendingTarget.set(ctx.from.id, {
        orderId: order.id,
        label: product.targetLabel ?? "Target",
      });
      await ctx.reply(
        `Order ${order.id} dibuat.\n${product.name} — ${rupiah(order.retailTotalIdr)}\n\n` +
          `Kirim ${product.targetLabel ?? "target"} kamu di chat ini (satu pesan).`,
      );
      await ctx.answerCallbackQuery();
      return;
    }
    try {
      const payment = await createSnapTransaction(order.id);
      await ctx.reply(
        `Order ${order.id} dibuat.\n${product.name} — ${rupiah(order.retailTotalIdr)}\n\n` +
          `Bayar di sini: ${payment.redirectUrl}\n\n` +
          "Setelah bayar lunas, order otomatis diteruskan ke supplier dan hasilnya dikirim ke sini. " +
          `Lacak dengan /order ${order.id}`,
      );
    } catch (err) {
      await ctx.reply(
        `Order ${order.id} dibuat tapi pembayaran belum bisa dibuat: ${err instanceof Error ? err.message : String(err)}\n` +
          "Coba lagi nanti atau hubungi admin.",
      );
    }
    await ctx.answerCallbackQuery();
  });

  // Terima target dari pembeli (balasan setelah tap Beli produk on-demand).
  bot.on("message:text", async (ctx) => {
    const chatId = ctx.from?.id;
    if (!chatId) return;
    const pending = pendingTarget.get(chatId);
    if (!pending) return;
    const text = ctx.message.text.trim();
    if (text.startsWith("/")) return; // perintah — bukan target
    pendingTarget.delete(chatId);
    try {
      await setOrderTarget(pending.orderId, text);
      const payment = await createSnapTransaction(pending.orderId);
      const order = await getStoreOrder(pending.orderId);
      await ctx.reply(
        `Target diterima. Order ${pending.orderId} — ${order ? rupiah(order.retailTotalIdr) : ""}\n\n` +
          `Bayar di sini: ${payment.redirectUrl}\n\n` +
          "Setelah bayar lunas, order otomatis diteruskan ke supplier dan hasilnya dikirim ke sini.",
      );
    } catch (err) {
      await ctx.reply(
        `Gagal membuat pembayaran: ${err instanceof Error ? err.message : String(err)}`,
      );
    }
  });

  bot.command("order", async (ctx) => {
    const id = ctx.match.trim();
    if (!id) {
      await ctx.reply("Pakai: /order <id> — contoh: /order UD-XXXX");
      return;
    }
    const order = await getStoreOrder(id.toUpperCase());
    if (!order) {
      await ctx.reply("Order tidak ditemukan.");
      return;
    }
    await ctx.reply(orderStatusText(order));
  });

  // Khusus admin — jalankan fulfillment setelah bayar terkonfirmasi.
  // Normalnya ini otomatis via webhook Midtrans (bayar lunas -> markPaid -> fulfillOrder).
  bot.command("fulfill", async (ctx) => {
    if (!isAdmin(ctx)) {
      await ctx.reply("Perintah ini khusus admin.");
      return;
    }
    const id = ctx.match.trim().toUpperCase();
    if (!id) {
      await ctx.reply("Pakai: /fulfill <id>");
      return;
    }
    await ctx.reply(`Memproses order ${id} ke supplier...`);
    try {
      await markPaid(id);
      const done = await fulfillOrder(id);
      await ctx.reply(orderStatusText(done));
    } catch (err) {
      await ctx.reply(
        `Gagal: ${err instanceof Error ? err.message : String(err)}`,
      );
    }
  });

  return bot;
}
