// Bot news Unicodes Digital (@unicodesdigitalbot) — MINIMAL.
// Tugasnya cuma dua:
//   1. Menjawab /start (dan pesan lain) dengan info: bot ini memposting
//      semua info transaksi; belanja tetap di bot toko.
//   2. (Bukan di sini) Posting berita transaksi ke channel — itu dilakukan
//      notifyChannelOrder() memakai TOKEN bot ini (lihat src/lib/notify.ts),
//      jadi posting tidak butuh proses polling.
// Long polling di sini HANYA supaya /start dibalas — bot publik yang diam
// kelihatan rusak. Pakai fixedFetch yang sama seperti bot toko.
import { Bot } from "grammy";
import { fixedFetch } from "./fixed-fetch";
import { telegramChannelUrl } from "../lib/config";

const STORE_BOT_URL = "https://t.me/unicodesdigital_bot";

function infoText(): string {
  const lines = [
    "📢 Unicodes Digital — News & Info Transaksi",
    "",
    "Bot ini memposting semua info transaksi: order masuk, order terkirim, promo, dan update stok. Bagian yang sensitif selalu disensor (nama pembeli disamarkan, kode hasil / target tidak pernah ditampilkan).",
    "",
    `🛍️ Belanja: ${STORE_BOT_URL}`,
  ];
  const channelUrl = telegramChannelUrl();
  if (channelUrl) lines.push(`👥 Komunitas: ${channelUrl}`);
  return lines.join("\n");
}

export function createNewsBot(token: string): Bot {
  const bot = new Bot(token, { client: { fetch: fixedFetch as typeof fetch } });
  bot.command("start", async (ctx) => {
    await ctx.reply(infoText());
  });
  // Pesan lain: balas info yang sama (bot ini bukan toko — jangan diam).
  bot.on("message:text", async (ctx) => {
    await ctx.reply(infoText());
  });
  return bot;
}
