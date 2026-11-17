// Entry point bot Telegram Unicodes Digital (long polling).
// Jalankan: npm run bot
// Untuk produksi (VPS/railway), jalankan proses ini terus-menerus
// (mis. systemd / Railway worker) — terpisah dari proses web (npm run dev/start).
// dotenv bawaan hanya baca `.env`, sedangkan semua secret proyek ada di
// `.env.local` — jadi path-nya disebut eksplisit.
import dotenv from "dotenv";
dotenv.config({ path: ".env.local" });
import { createBot } from "./bot";

const bot = createBot();

// Jangan mati kalau polling sempat error (mis. jaringan putus sesaat) —
// log saja, runner grammY akan coba lagi. Tanpa ini, satu error tak
// tertangani bisa mematikan seluruh proses bot.
bot.catch((err) => {
  console.error("[bot] error:", err);
});

bot.start().then(() => {
  console.log("Unicodes Digital bot jalan (long polling)...");
});

process.once("SIGINT", () => bot.stop());
process.once("SIGTERM", () => bot.stop());
