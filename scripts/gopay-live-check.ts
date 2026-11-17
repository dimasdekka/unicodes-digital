// Live-check GoBiz — READ-ONLY, tanpa OTP, tanpa pembayaran.
// Jalankan MANUAL (oleh Dim / operator) SETELAH login OTP di panel /admin:
//   npx tsx scripts/gopay-live-check.ts
//
// Yang dicek: status sesi tersimpan, profil merchant (+ cocok tidaknya
// QRIS statis outlet dengan yang di settings), dan transaksi feed 1 jam
// terakhir (jumlah + 5 terbaru: id/nominal/waktu/status).
// TIDAK memanggil requestOtp / verifyOtp, TIDAK menulis apa pun, TIDAK
// menampilkan token. Keluar 1 kalau sesi belum login / kedaluwarsa.
//
// ⚠️ JANGAN jalankan skrip ini dari agent otomatis — hanya saat Dim siap
// mengetes login GoBiz live (OTP live hanya dipicu dari panel /admin).
import dotenv from "dotenv";
dotenv.config({ path: ".env.local" });

import { gopayClient } from "../src/lib/payments/gopay";
import { getGopayStaticQris } from "../src/lib/payments/qris-settings";

async function main(): Promise<void> {
  const session = gopayClient.sessionStatus();
  console.log("== Sesi GoBiz ==");
  console.log(`  login: ${session.authenticated ? "YA" : "TIDAK / KEDALUWARSA"}`);
  console.log(`  HP: ${session.phone ?? "-"}`);
  console.log(`  token berlaku s.d.: ${session.expiresAt ?? "-"}`);
  console.log(`  merchant_id: ${session.merchantId ?? "-"}`);
  if (!session.authenticated) {
    console.log("\nBelum login — minta OTP dulu di panel /admin, lalu jalankan lagi.");
    process.exit(1);
  }

  console.log("\n== Profil merchant ==");
  try {
    const { profile, staticQrisMatches, warnings } =
      await gopayClient.getMerchantProfile();
    console.log(`  merchant: ${profile.merchantName ?? "?"} (${profile.outletName ?? "?"})`);
    console.log(`  outlet: ${profile.outlets.length} (qrString: ${profile.outlets.filter((o) => o.qrString).length} terisi)`);
    console.log(`  QRIS statis cocok dengan settings: ${staticQrisMatches ? "YA" : "TIDAK"}`);
    for (const w of warnings) console.log(`  ⚠️ ${w}`);
  } catch (err) {
    console.log(`  GAGAL: ${err instanceof Error ? err.message : String(err)}`);
    process.exit(1);
  }

  console.log("\n== Feed 1 jam terakhir ==");
  console.log(`  QRIS statis tersimpan: ${getGopayStaticQris() ? "YA" : "TIDAK"}`);
  try {
    const { transactions, truncated } = await gopayClient.listFeedTransactions();
    console.log(`  transaksi: ${transactions.length}${truncated ? " (terpotong — lebih dari 10 halaman)" : ""}`);
    for (const t of transactions.slice(0, 5)) {
      const when = t.time == null ? "?" : new Date(t.time).toISOString();
      console.log(
        `  - ${t.id}: Rp${t.amountIdr.toLocaleString("id-ID")} • ${t.status || "?"} • ${when}`,
      );
    }
  } catch (err) {
    console.log(`  GAGAL: ${err instanceof Error ? err.message : String(err)}`);
    process.exit(1);
  }
  console.log("\nOK — GoBiz live-check selesai.");
}

void main();
