// CLI sekali jalan: set Menu Button bot jadi Web App "Buka Toko".
// Jalankan SETELAH deploy Railway + NEXT_PUBLIC_APP_URL terisi domain publik:
//   npx tsx src/bot/set-menu-button.ts
// Jangan jalankan sebelum itu — getWebAppUrl() akan menolak (localhost).
import dotenv from "dotenv";
dotenv.config({ path: ".env.local" });
import { setWebAppMenuButton } from "./menu-button";

setWebAppMenuButton()
  .then((r) => {
    console.log("OK:", r.description);
  })
  .catch((err: unknown) => {
    console.error("GAGAL:", err instanceof Error ? err.message : String(err));
    process.exit(1);
  });
