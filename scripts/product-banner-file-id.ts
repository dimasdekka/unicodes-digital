// One-off: kirim banner produk ke admin, ambil file_id, lalu hapus pesannya.
// Pakai: npx tsx scripts/product-banner-file-id.ts
// (Token dibaca dari .env.local — TIDAK pernah dicetak.)
import { readFileSync } from "node:fs";

async function main(): Promise<void> {
  const env = readFileSync(".env.local", "utf8");
  const token = env
    .split("\n")
    .find((l) => l.startsWith("TELEGRAM_BOT_TOKEN="))
    ?.split("=")[1]
    ?.trim();
  if (!token) throw new Error("TELEGRAM_BOT_TOKEN tidak ada di .env.local");
  const adminId = "807719771";
  const file = new URL("../src/bot/assets/product-muse-token-1b.jpg", import.meta.url);

  const form = new FormData();
  form.append("chat_id", adminId);
  form.append("photo", new Blob([readFileSync(file)], { type: "image/jpeg" }), "product-muse-token-1b.jpg");
  const send = await fetch(`https://api.telegram.org/bot${token}/sendPhoto`, {
    method: "POST",
    body: form,
  });
  const sent = (await send.json()) as {
    ok: boolean;
    result?: { message_id: number; photo?: { file_id: string }[] };
    description?: string;
  };
  if (!sent.ok || !sent.result?.photo?.length) {
    throw new Error(`sendPhoto gagal: ${sent.description ?? JSON.stringify(sent)}`);
  }
  const fileId = sent.result.photo[sent.result.photo.length - 1].file_id;
  const msgId = sent.result.message_id;
  await fetch(`https://api.telegram.org/bot${token}/deleteMessage`, {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify({ chat_id: adminId, message_id: msgId }),
  });
  // Simpan ke .env.local (ganti baris lama kalau sudah ada).
  const key = "PRODUCT_BANNER_FILE_ID_MUSE_TOKEN_1B";
  const lines = readFileSync(".env.local", "utf8").split("\n");
  const idx = lines.findIndex((l) => l.startsWith(`${key}=`));
  if (idx >= 0) lines[idx] = `${key}=${fileId}`;
  else lines.push(`${key}=${fileId}`);
  const { writeFileSync } = await import("node:fs");
  writeFileSync(".env.local", lines.join("\n"));
  console.log(`${key} tersimpan di .env.local`);
}

main().catch((e) => {
  console.error(e instanceof Error ? e.message : e);
  process.exit(1);
});
