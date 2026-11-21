// Notifikasi Telegram ke pembeli saat order selesai/gagal.
// Dipakai jalur bot (bayar saldo) DAN poller QRIS —
// pembeli tidak perlu mengecek /order terus-menerus.
// Dikirim via fetch langsung ke Bot API (tanpa grammY) supaya bisa dipakai
// dari proses web (Next.js) maupun proses bot. Gagal kirim = log saja,
// jangan pernah menggagalkan order.
import type { StoreOrder } from "./orders";
import { getUser, tgIdFromBuyerRef } from "./wallet";
import { getDb } from "./db";
import { getCatalogProduct } from "./catalog";
import { IDR_PER_USD } from "./pricing";
import {
  telegramAdminIds,
  telegramBotToken,
  telegramChannelId,
} from "./config";
import { rupiah, t } from "../bot/i18n";

/** Samarkan tengah string: 4 karakter awal + •••• + 4 karakter akhir. */
function maskMiddle(value: string): string {
  if (value.length <= 8) return `${value.slice(0, 2)}••••`;
  return `${value.slice(0, 4)}••••${value.slice(-4)}`;
}

/** Posting "NEW ORDER" ke channel komunitas (bukti transaksi, ala Sondev
 *  Store). HANYA order delivered. Yang diposting: ringkasan order saja —
 *  TIDAK PERNAH item hasil (kode invite/token/link) atau target pembeli.
 *  Buyer + Order ID disamarkan. Dedupe via tabel channel_posts: satu order
 *  diposting sekali. Channel belum dikonfigurasi = diam-diam tidak posting.
 *  Gagal kirim = log saja, jangan pernah menggagalkan order. */
export async function notifyChannelOrder(order: StoreOrder): Promise<boolean> {
  if (order.status !== "delivered") return false;
  const channelId = telegramChannelId();
  if (!channelId) return false;
  let token: string;
  try {
    token = telegramBotToken();
  } catch {
    return false;
  }
  // Dedupe dulu (insert-or-ignore): sudah pernah diposting = lewati.
  try {
    const res = getDb()
      .prepare(
        "INSERT OR IGNORE INTO channel_posts (order_id, posted_at) VALUES (?, ?)",
      )
      .run(order.id, new Date().toISOString());
    if (res.changes === 0) return false;
  } catch (err) {
    console.error(
      "[notify] channel dedupe gagal:",
      err instanceof Error ? err.message : String(err),
    );
    return false;
  }
  const tgId = tgIdFromBuyerRef(order.buyerRef);
  const user = tgId != null ? getUser(tgId) : undefined;
  const buyerRaw = user?.username ?? user?.firstName ?? "";
  const buyer = buyerRaw ? `@${buyerRaw.slice(0, 2)}••••` : "—";
  let productName = order.catalogProductId;
  let qtyUnit = "item";
  try {
    productName = getCatalogProduct(order.catalogProductId).name;
    if (order.catalogProductId.startsWith("muse-token")) qtyUnit = "B";
  } catch {
    // nama katalog tidak ketemu — pakai id produk apa adanya
  }
  const usd = (order.retailTotalIdr / IDR_PER_USD)
    .toFixed(2)
    .replace(/\.?0+$/, "");
  const method =
    order.paidVia === "wallet"
      ? "Saldo"
      : order.paidVia === "qris"
        ? "QRIS"
        : "—";
  const payment =
    order.paidVia === "wallet"
      ? "Wallet Balance"
      : order.paidVia === "qris"
        ? "QRIS"
        : "—";
  const dateWib = new Intl.DateTimeFormat("en-GB", {
    timeZone: "Asia/Jakarta",
    day: "2-digit",
    month: "short",
    year: "numeric",
    hour: "2-digit",
    minute: "2-digit",
    hour12: false,
  }).format(new Date(order.createdAt));
  const text = [
    `🎉 <b>NEW ORDER</b>`,
    `──────────────────`,
    `🛍️ <b>Status: DELIVERED</b> ⚡`,
    ``,
    `👤 <b>Buyer:</b> ${buyer}`,
    `📦 <b>Product:</b> ${productName} (${order.quantity}${qtyUnit})`,
    `🔢 <b>Quantity:</b> ${order.quantity}${qtyUnit}`,
    `💰 <b>Total:</b> ${rupiah(order.retailTotalIdr)} / $${usd}`,
    `💳 <b>Method:</b> ${method}`,
    `🧾 <b>Payment:</b> ${payment}`,
    `🆔 <b>Order ID:</b> ${maskMiddle(order.id)}`,
    `📅 <b>Date:</b> ${dateWib} WIB`,
    ``,
    `🚀 <b>Automated &amp; Instant sent by bot</b>`,
  ].join("\n");
  try {
    const res = await fetch(`https://api.telegram.org/bot${token}/sendMessage`, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({
        chat_id: channelId,
        text,
        parse_mode: "HTML",
        reply_markup: {
          inline_keyboard: [
            [{ text: "🛍️ Buy Now", url: "https://t.me/unicodesdigital_bot" }],
          ],
        },
      }),
      signal: AbortSignal.timeout(15000),
    });
    if (!res.ok) {
      console.error("[notify] channel post gagal:", res.status);
      return false;
    }
    return true;
  } catch (err) {
    console.error(
      "[notify] channel post gagal:",
      err instanceof Error ? err.message : String(err),
    );
    return false;
  }
}

export async function notifyBuyer(order: StoreOrder): Promise<void> {
  const tgId = tgIdFromBuyerRef(order.buyerRef);
  if (tgId == null) return; // order web / tanpa pembeli Telegram
  let token: string;
  try {
    token = telegramBotToken();
  } catch {
    return;
  }
  const user = getUser(tgId);
  const lang = user?.lang ?? "id";
  const s = t(lang);
  const lines: string[] = [
    order.status === "delivered"
      ? lang === "id"
        ? `🎉 <b>Order ${order.id} selesai!</b>`
        : `🎉 <b>Order ${order.id} is done!</b>`
      : lang === "id"
        ? `❌ <b>Order ${order.id} gagal.</b>`
        : `❌ <b>Order ${order.id} failed.</b>`,
    "",
    `Produk: ${order.catalogProductId} × ${order.quantity}`,
    `Total: ${rupiah(order.retailTotalIdr)}`,
    `Status: ${s.statusLabel(order.status)}`,
  ];
  if (order.deliveredItems.length) {
    lines.push("", "Hasil:", ...order.deliveredItems.map((i) => `• ${i}`));
  }
  if (order.resultLink) lines.push(`🔗 Link: ${order.resultLink}`);
  if (order.error) lines.push(`⚠️ ${order.error}`);
  if (order.status === "delivered") {
    lines.push(
      "",
      lang === "id"
        ? "Terima kasih sudah belanja di Unicodes Digital! 💜"
        : "Thanks for shopping at Unicodes Digital! 💜",
    );
  }
  try {
    await fetch(`https://api.telegram.org/bot${token}/sendMessage`, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({
        chat_id: tgId,
        text: lines.join("\n"),
        parse_mode: "HTML",
      }),
      signal: AbortSignal.timeout(15000),
    });
  } catch (err) {
    console.error(
      "[notify] gagal kirim ke",
      tgId,
      err instanceof Error ? err.message : String(err),
    );
  }
  // Order selesai -> bukti transaksi ke channel komunitas (sekali saja).
  if (order.status === "delivered") {
    await notifyChannelOrder(order);
  }
}

/** Kirim pesan ke admin pertama (TELEGRAM_ADMIN_IDS[0]).
 *  Dipakai untuk hal yang butuh tindakan manusia — mis. refund manual QRIS
 *  setelah fulfillment gagal. Gagal kirim = log saja, jangan pernah melempar
 *  (pemanggil tidak boleh gagal hanya karena notifikasi). */
export async function notifyAdmin(text: string): Promise<void> {
  const ids = telegramAdminIds();
  if (ids.length === 0) return;
  let token: string;
  try {
    token = telegramBotToken();
  } catch {
    return;
  }
  try {
    await fetch(`https://api.telegram.org/bot${token}/sendMessage`, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({
        chat_id: ids[0],
        text,
        parse_mode: "HTML",
      }),
      signal: AbortSignal.timeout(15000),
    });
  } catch (err) {
    console.error(
      "[notify] gagal kirim ke admin:",
      err instanceof Error ? err.message : String(err),
    );
  }
}
