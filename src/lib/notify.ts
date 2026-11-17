// Notifikasi Telegram ke pembeli saat order selesai/gagal.
// Dipakai jalur bot (bayar saldo) DAN poller QRIS —
// pembeli tidak perlu mengecek /order terus-menerus.
// Dikirim via fetch langsung ke Bot API (tanpa grammY) supaya bisa dipakai
// dari proses web (Next.js) maupun proses bot. Gagal kirim = log saja,
// jangan pernah menggagalkan order.
import type { StoreOrder } from "./orders";
import { getUser, tgIdFromBuyerRef } from "./wallet";
import { telegramAdminIds, telegramBotToken } from "./config";
import { rupiah, t } from "../bot/i18n";

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
