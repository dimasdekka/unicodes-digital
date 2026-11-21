// Fetch pengganti untuk grammY di VM ini (BUG yang bikin bot diam sejak awal).
// grammY 1.46 memakai AbortSignal polyfill (paket `abort-controller`) yang
// DITOLAK fetch bawaan Node 24 ("Expected signal to be an instance of
// AbortSignal") — akibatnya SEMUA request Telegram gagal instan dan long
// polling tidak pernah menerima update apa pun, tanpa error yang kelihatan
// (runner hanya retry diam-diam). Di sini signal-nya dikonversi ke
// AbortController native, dan opsi node-fetch (agent/compress/duplex) yang
// disisipkan grammY dibuang karena undici tidak mengenalnya.
//
// Dipakai bot toko (bot.ts) DAN bot news (news-bot.ts) — JANGAN hapus,
// jangan ganti dengan fetch biasa.
export function fixedFetch(
  url: string | URL | Request,
  init: Record<string, unknown> = {},
): Promise<Response> {
  const { agent: _agent, compress: _compress, duplex: _duplex, signal, ...rest } =
    init as Record<string, unknown> & { signal?: AbortSignal };
  let nativeSignal: AbortSignal | undefined;
  if (signal) {
    const controller = new AbortController();
    if (signal.aborted) controller.abort();
    else
      signal.addEventListener("abort", () => controller.abort(), { once: true });
    nativeSignal = controller.signal;
  }
  return fetch(url, {
    ...(rest as RequestInit),
    ...(nativeSignal ? { signal: nativeSignal } : {}),
  });
}
