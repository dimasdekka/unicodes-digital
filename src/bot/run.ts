// Entry point bot Telegram Unicodes Digital (long polling).
// Jalankan: npm run bot
// Untuk produksi (VPS/railway), jalankan proses ini terus-menerus
// (mis. systemd / Railway worker) — terpisah dari proses web (npm run dev/start).
// dotenv bawaan hanya baca `.env`, sedangkan semua secret proyek ada di
// `.env.local` — jadi path-nya disebut eksplisit.
import dotenv from "dotenv";
dotenv.config({ path: ".env.local" });
import { createBot } from "./bot";
import { gopayPollIntervalMs } from "../lib/config";
import { tickQrisPayments } from "../lib/payments/qris-poller";
import { tickMidtransPayments } from "../lib/payments/midtrans-poller";

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

// Poller pembayaran — SATU-SATUNYA tempat pembayaran dinyatakan lunas
// (QRIS GoBiz di qris-poller.ts, Midtrans di midtrans-poller.ts). Jalan di
// proses bot saja (bukan di web) supaya tidak ada dua poller yang rebutan
// memproses transaksi yang sama. tick() tidak pernah throw; kalau sesi
// GoBiz belum login, tick mengembalikan ok:false dan dicatat sebagai
// peringatan (bukan crash). Midtrans yang belum dikonfigurasi diam saja.
let pollerRunning = false;
async function pollOnce(): Promise<void> {
  if (pollerRunning) return; // tick sebelumnya belum selesai — lewati jadwal ini
  pollerRunning = true;
  try {
    const result = await tickQrisPayments();
    if (!result.ok) {
      console.warn(`[qris-poller] ${result.code}: ${result.error}`);
    } else if (result.matched > 0 || result.expired > 0) {
      console.log(
        `[qris-poller] dicek ${result.checked}, lunas ${result.matched}, kedaluwarsa ${result.expired}.`,
      );
    }
    const midtrans = await tickMidtransPayments();
    if (!midtrans.ok) {
      console.warn(`[midtrans-poller] ${midtrans.code}: ${midtrans.error}`);
    } else if (midtrans.matched > 0 || midtrans.expired > 0) {
      console.log(
        `[midtrans-poller] dicek ${midtrans.checked}, lunas ${midtrans.matched}, kedaluwarsa ${midtrans.expired}.`,
      );
    }
  } catch (err) {
    console.error(
      "[qris-poller] tick gagal:",
      err instanceof Error ? err.message : String(err),
    );
  } finally {
    pollerRunning = false;
  }
}
setInterval(() => void pollOnce(), gopayPollIntervalMs());
setTimeout(() => void pollOnce(), 10_000); // tick pertama 10 detik setelah start

process.once("SIGINT", () => bot.stop());
process.once("SIGTERM", () => bot.stop());
