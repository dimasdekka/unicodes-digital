// Entry point bot news (@unicodesdigitalbot, long polling).
// Jalankan: npx tsx src/bot/news-run.ts
// Berdampingan dengan bot toko (run.ts) — proses terpisah, token terpisah.
// Kalau TELEGRAM_NEWS_BOT_TOKEN kosong, proses keluar diam-diam.
import dotenv from "dotenv";
dotenv.config({ path: ".env.local" });
import { createNewsBot } from "./news-bot";
import { telegramNewsBotToken } from "../lib/config";

const token = telegramNewsBotToken();
if (!token) {
  console.log("[news-bot] TELEGRAM_NEWS_BOT_TOKEN kosong — news bot tidak jalan.");
  process.exit(0);
}

const bot = createNewsBot(token);

bot.catch((err) => {
  console.error("[news-bot] error:", err);
});

bot.start().then(() => {
  console.log("Unicodes Digital news bot jalan (long polling)...");
});

process.once("SIGINT", () => bot.stop());
process.once("SIGTERM", () => bot.stop());
