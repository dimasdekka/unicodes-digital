// Tombol Menu bot Telegram (tombol biru "Menu" di pojok kiri bawah chat).
// Target: Menu Button type web_app "Buka Toko" yang membuka Mini App /tg.
//
// ⚠️ JANGAN PANGGIL setWebAppMenuButton() SEBELUM DEPLOY RAILWAY SELESAI:
// URL Web App harus domain PUBLIK (https://<domain>/tg). Selama
// NEXT_PUBLIC_APP_URL masih localhost / belum diisi, fungsi ini sengaja
// throw — tombol menu tidak boleh menunjuk ke URL yang tidak bisa dibuka
// pembeli dari HP-nya.
//
// Setelah deploy (lihat DEPLOY.md bagian "Telegram Web App"):
//   1. Isi NEXT_PUBLIC_APP_URL=https://<domain> di env Railway + .env.local.
//   2. Jalankan sekali dari repo ini:
//        npx tsx src/bot/set-menu-button.ts
//   3. Buka chat bot di Telegram — tombol "Menu" kiri bawah jadi "Buka Toko".
// Fungsi ini idempotent (boleh dijalankan ulang kapan saja).
export function getWebAppUrl(): string | undefined {
  const base = (process.env.NEXT_PUBLIC_APP_URL ?? "").trim().replace(/\/+$/, "");
  if (!base) return undefined;
  // Belum publik (dev lokal) — sembunyikan tombol Web App.
  if (/^https?:\/\/(localhost|127\.0\.0\.1)(:\d+)?$/i.test(base)) return undefined;
  return `${base}/tg`;
}

export async function setWebAppMenuButton(): Promise<{
  ok: boolean;
  description?: string;
}> {
  const token = process.env.TELEGRAM_BOT_TOKEN;
  if (!token) {
    throw new Error("TELEGRAM_BOT_TOKEN belum diisi di .env.local");
  }
  const url = getWebAppUrl();
  if (!url) {
    throw new Error(
      "NEXT_PUBLIC_APP_URL belum menunjuk domain publik — isi dulu setelah deploy Railway (lihat DEPLOY.md), lalu jalankan: npx tsx src/bot/set-menu-button.ts",
    );
  }
  const res = await fetch(
    `https://api.telegram.org/bot${token}/setChatMenuButton`,
    {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({
        menu_button: {
          type: "web_app",
          text: "Buka Toko",
          web_app: { url },
        },
      }),
      signal: AbortSignal.timeout(20000),
    },
  );
  const body = (await res.json().catch(() => ({}))) as Record<string, unknown>;
  if (!res.ok || body.ok !== true) {
    throw new Error(
      `setChatMenuButton gagal: ${JSON.stringify(body).slice(0, 300)}`,
    );
  }
  return { ok: true, description: `Menu Button -> ${url}` };
}
