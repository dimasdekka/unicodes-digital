// Cek koneksi Midtrans (sandbox/production sesuai .env.local).
// TIDAK menagih apa pun: transaksi Snap sandbox hanya tes; key TIDAK
// pernah dicetak. Jalankan: npx tsx scripts/midtrans-check.ts
import dotenv from "dotenv";
dotenv.config({ path: ".env.local" });
import { midtransIsProduction } from "../src/lib/config";
import {
  createSnapTransaction,
  getTransactionStatus,
} from "../src/lib/payments/midtrans";

async function main(): Promise<void> {
  console.log(`mode: ${midtransIsProduction() ? "PRODUCTION" : "SANDBOX"}`);
  const orderId = `TEST-${Date.now()}`;
  const snap = await createSnapTransaction({
    orderId,
    amountIdr: 1500,
    itemName: "Tes koneksi Midtrans",
  });
  console.log("Snap OK — token diterima, redirect:", snap.redirectUrl.slice(0, 60) + "…");
  const status = await getTransactionStatus(orderId);
  console.log("Status OK — body:", JSON.stringify(status));
}

main().catch((err) => {
  console.error("GAGAL:", err instanceof Error ? err.message : String(err));
  process.exit(1);
});
