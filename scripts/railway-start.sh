#!/bin/sh
# Railway: jalankan web + bot Telegram dalam SATU service.
# Satu service = satu volume = satu file dev.db dipakai bareng web & bot.
# Kalau dipisah jadi 2 service, masing-masing punya volume sendiri dan
# databasenya jadi dua (order web tidak kelihatan di bot, dan sebaliknya).
#
# Bot jalan di background (long polling, tidak butuh port).
# Web jalan di foreground — Railway memberi $PORT (default 3000).
# Kalau web mati, container mati dan Railway me-restart service-nya.
npx tsx src/bot/run.ts &
exec npm run start -- -p "${PORT:-3000}"
