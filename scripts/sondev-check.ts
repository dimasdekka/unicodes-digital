// One-off: cek ulang SEMUA endpoint Sondev yang dipakai adapter.
// TIDAK memanggil createOrder (itu order beneran, potong saldo).
// Jalankan: npx tsx scripts/sondev-check.ts
import dotenv from "dotenv";
dotenv.config({ path: ".env.local" });

import { SondevAdapter } from "../src/lib/suppliers/sondev";

async function run(name: string, fn: () => Promise<unknown>): Promise<void> {
  const t0 = Date.now();
  try {
    const out = await fn();
    console.log(
      `OK   ${name} (${Date.now() - t0}ms): ${JSON.stringify(out).slice(0, 300)}`,
    );
  } catch (e) {
    console.log(
      `FAIL ${name} (${Date.now() - t0}ms): ${(e as Error).message.slice(0, 300)}`,
    );
  }
}

async function main(): Promise<void> {
  const s = new SondevAdapter();
  await run("GET /status/public", () => s.getPublicStatus());
  await run("GET /profile (saldo)", () => s.getBalance());
  await run("GET /products (katalog)", () => s.listProducts());
  await run("GET /stock?code=muse_token", () => s.getStock("muse_token"));
  await run("GET /orders?limit=5", () => s.listOrders(5));
  await run("GET /orders/TEST-TIDAK-ADA (probe)", () => s.getOrder("TEST-TIDAK-ADA"));
  await run("GET /logs?limit=5", () => s.getLogs(5));
  await run("GET /webhook/dead-letters", () => s.getDeadLetters());
  await run("POST /webhook/test", () => s.testWebhook());
  await run("POST /webhook/replay", () => s.replayWebhooks());
  console.log("SELESAI — createOrder TIDAK dites (order beneran, potong saldo).");
}

main();
