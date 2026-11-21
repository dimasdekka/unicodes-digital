#!/bin/sh
# VPS / Oracle Always Free: jalankan web + bot Telegram dalam SATU service.
# Sama seperti scripts/railway-start.sh — satu service = satu file dev.db
# dipakai bareng web & bot. Bedanya: PORT default 3100 (bukan $PORT Railway).
#
# Bot toko + bot news jalan di background (long polling, tidak butuh port).
# Web jalan di foreground — kalau web mati, systemd me-restart service-nya.
npx tsx src/bot/run.ts &
npx tsx src/bot/news-run.ts &
exec npm run start -- -p "${PORT:-3100}"
