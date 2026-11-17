// One-off: tarik katalog + harga live dari kedua supplier (Sondev & LahaStore).
// Pakai adapter proyek (key/secret dibaca dari .env.local, TIDAK pernah di-print).
// Jalankan: npx tsx scripts/fetch-catalogs.ts
import dotenv from "dotenv";
dotenv.config({ path: ".env.local" });

import { SondevAdapter } from "../src/lib/suppliers/sondev";
import { LahaStoreAdapter } from "../src/lib/suppliers/lahastore";
import { lahastoreConfig } from "../src/lib/config";

const { baseUrl: LAHA_BASE, apiKey: LAHA_KEY } = lahastoreConfig();

async function lahaRaw(path: string): Promise<unknown> {
  const res = await fetch(`${LAHA_BASE}${path}`, {
    headers: { "X-API-Key": LAHA_KEY },
  });
  const text = await res.text();
  try {
    return { http: res.status, body: JSON.parse(text) };
  } catch {
    return { http: res.status, body: text.slice(0, 500) };
  }
}

async function main() {
  console.log("===== SONDEV =====");
  const sondev = new SondevAdapter();
  try {
    const bal = await sondev.getBalance();
    console.log("BALANCE:", JSON.stringify(bal));
  } catch (e) {
    console.log("BALANCE GAGAL:", (e as Error).message);
  }
  try {
    const prods = await sondev.listProducts();
    console.log(`PRODUCTS (${prods.length}):`);
    for (const p of prods) {
      console.log(JSON.stringify(p));
    }
  } catch (e) {
    console.log("PRODUCTS GAGAL:", (e as Error).message);
  }
  try {
    const st = await sondev.getPublicStatus();
    console.log("PUBLIC STATUS:", JSON.stringify(st).slice(0, 400));
  } catch (e) {
    console.log("PUBLIC STATUS GAGAL:", (e as Error).message);
  }

  console.log("\n===== LAHASTORE =====");
  const laha = new LahaStoreAdapter();
  try {
    const bal = await laha.getBalance();
    console.log("BALANCE:", JSON.stringify(bal));
  } catch (e) {
    console.log("BALANCE GAGAL:", (e as Error).message);
  }
  try {
    const prods = await laha.listProducts();
    console.log(`SHOP PRODUCTS (${prods.length}):`);
    for (const p of prods) {
      console.log(JSON.stringify(p));
    }
  } catch (e) {
    console.log("SHOP PRODUCTS GAGAL:", (e as Error).message);
  }
  console.log("\n--- SHEERID service_info ---");
  console.log(JSON.stringify(await lahaRaw("/sheerid/service_info")).slice(0, 2000));
  console.log("\n--- XIAOMI3 services ---");
  console.log(JSON.stringify(await lahaRaw("/xiaomi3/services")).slice(0, 2000));
}

main().catch((e) => {
  console.error("FATAL:", e);
  process.exit(1);
});
