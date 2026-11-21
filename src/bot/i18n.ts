// Teks bot Telegram — Indonesia (default) & English.
// Semua layar bot mengambil teks dari sini lewat t(lang).key —
// tambah bahasa baru = tambah satu entri di STR.
import type { Lang } from "../lib/wallet";
import type { StockState } from "../lib/catalog";

export type { Lang };

export const rupiah = (n: number): string => `Rp${n.toLocaleString("id-ID")}`;

/** Jam (HH:mm) dari ISO string, zona WIB — untuk kedaluwarsa QR di caption. */
export const wibTime = (iso: string): string =>
  new Date(iso).toLocaleTimeString("id-ID", {
    timeZone: "Asia/Jakarta",
    hour: "2-digit",
    minute: "2-digit",
  });

export interface Strings {
  // Sapaan /start: nama + statistik order (hanya kalau orders > 0).
  // Referral SENGAJA tidak ada di sini — hanya di menu 🎁 Referral.
  welcome: (name: string, orders: number, spent: number) => string;
  menuPrompt: string;
  catalog: string;
  myBalance: string;
  myOrders: string;
  referral: string;
  promo: string;
  support: string;
  policy: string;
  reseller: string;
  language: string;
  backMenu: string;
  catalogTitle: string;
  catalogPickCategory: string;
  categoryAll: string;
  productDetail: (p: {
    name: string;
    desc: string;
    price: number;
    unit?: string;
    badge?: string;
    features: string[];
    stock: string;
    targetNote?: string;
  }) => string;
  comingSoon: string;
  outOfStock: string;
  stockLabel: (state: StockState) => string;
  qtyPrompt: string;
  qtyCustomPrompt: string;
  qtyInvalid: string;
  targetPrompt: (label: string) => string;
  promoAsk: string;
  promoEnterPrompt: string;
  promoApplied: (code: string, discount: number) => string;
  promoInvalid: (reason: string) => string;
  promoSkipped: string;
  payTitle: (p: {
    name: string;
    qty: number;
    subtotal: number;
    discount: number;
    total: number;
    balance: number;
  }) => string;
  payWallet: string;
  payQris: string;
  payWalletShort: string;
  walletInsufficient: string;
  walletPaid: (orderId: string) => string;
  walletRefunded: string;
  /** Caption foto QR order: nominal unik + kedaluwarsa (jam WIB). */
  qrisLink: (orderId: string, total: number, uniqueAmount: number, expiresAt: string) => string;
  /** Pesan gagal membuat QR (dengan alasan, kalau ada). */
  qrisFailed: (reason?: string) => string;
  /** Tombol di foto QR: pembeli menekan setelah bayar -> cek feed sekarang. */
  qrisCheckButton: string;
  qrisCheckFoundOrder: (orderId: string) => string;
  qrisCheckFoundTopup: (amount: number) => string;
  qrisCheckPending: string;
  qrisCheckExpired: string;
  qrisCheckNoSession: string;
  qrisCheckFailed: string;
  qrisCheckTooSoon: string;
  freeOrderDone: (orderId: string) => string;
  walletTitle: (balance: number, totalTopup: number, totalSpent: number) => string;
  topupTitle: string;
  topupCustomPrompt: string;
  topupInvalid: string;
  /** Caption foto QR top-up: nominal unik + kedaluwarsa (jam WIB). */
  topupCreated: (amount: number, uniqueAmount: number, expiresAt: string) => string;
  topupFailed: string;
  txnsTitle: string;
  txnsEmpty: string;
  txnLine: (t: { kind: string; amount: number; date: string; note?: string | null }) => string;
  ordersTitle: string;
  ordersEmpty: string;
  orderDetail: (o: {
    id: string;
    product: string;
    qty: number;
    total: number;
    status: string;
    items: string[];
    link: string | null;
    error: string | null;
  }) => string;
  statusLabel: (status: string) => string;
  referralTitle: (code: string, link: string, invited: number, completed: number, credit: number) => string;
  topReferrersTitle: string;
  topReferrersEmpty: string;
  topReferrerLine: (rank: number, tgId: number, invited: number, rewarded: number) => string;
  promoTitle: string;
  supportTitle: string;
  supportAsk: string;
  supportSent: string;
  supportForwarded: (name: string, tgId: number, text: string) => string;
  supportReplyHint: string;
  policyTitle: string;
  resellerTitle: string;
  langTitle: string;
  langSet: string;
  orderNotFound: string;
  orderUsage: string;
  fulfillUsage: string;
  fulfillProcessing: (id: string) => string;
  adminOnly: string;
  promoAddUsage: string;
  promoAdded: (code: string, kind: string, value: number) => string;
  promoListTitle: string;
  promoListEmpty: string;
  unknownCommand: string;
  cancelled: string;
}

const id: Strings = {
  welcome: (name, orders, spent) =>
    `⚡ Welcome to Unicodes Digital, ${name}! 💜\n\n` +
    `Toko AI & produk digital — harga jujur, proses instan.\n\n` +
    `🛍️ Di sini kamu bisa beli:\n` +
    `• Muse AI Token — recharge 1B sampai 30B\n\n` +
    `💳 Cara belanja: pilih produk → bayar via QRIS → token langsung dikirim ke akun kamu. Atau isi saldo dulu di menu Saldo Saya, biar checkout berikutnya lebih cepat.\n\n` +
    (orders > 0
      ? `📦 ${orders} order • Total belanja: ${rupiah(spent)}\n\n`
      : "") +
    `Pilih menu di bawah untuk mulai:`,
  menuPrompt: "Pilih menu di bawah:",
  catalog: "🛍️ Katalog Produk",
  myBalance: "💳 Saldo Saya",
  myOrders: "📦 Pesanan Saya",
  referral: "🎁 Referral",
  promo: "🎟️ Promo",
  support: "💬 Support",
  policy: "📜 Kebijakan",
  reseller: "🤝 Reseller & API",
  language: "🌐 Bahasa",
  backMenu: "⬅️ Menu Utama",
  catalogTitle:
    "🛍️ <b>Katalog Produk</b>\n\nPilih produk untuk lihat detail & beli. Harga sudah termasuk semua biaya.",
  catalogPickCategory: "🛍️ <b>Katalog Produk</b>\n\nPilih kategori dulu:",
  categoryAll: "Semua",
  productDetail: (p) =>
    `${p.badge ? `🏷️ <b>${p.badge}</b>\n` : ""}<b>${p.name}</b>\n\n${p.desc}\n\n` +
    (p.features.length ? p.features.map((f) => `✅ ${f}`).join("\n") + "\n\n" : "") +
    `💰 Harga: <b>${rupiah(p.price)}</b>${p.unit ? ` ${p.unit}` : ""}\n` +
    `📦 Stok: ${p.stock}\n` +
    (p.targetNote ? `\n🎯 ${p.targetNote}` : ""),
  comingSoon: "⏳ Segera hadir — produk ini belum bisa dibeli sekarang. Cek lagi nanti ya!",
  outOfStock: "🔴 Stok habis — produk ini sedang kosong. Cek lagi nanti ya!",
  stockLabel: (state) =>
    state === "available" ? "Tersedia" : state === "out_of_stock" ? "Stok Habis" : "Segera Hadir",
  qtyPrompt: "Pilih jumlah yang mau dibeli:",
  qtyCustomPrompt:
    "Ketik jumlah yang mau dibeli (angka 1–30), atau /cancel untuk batal.",
  qtyInvalid: "Jumlah harus angka 1–30. Coba lagi, atau /cancel untuk batal.",
  targetPrompt: (label) =>
    `🎯 Kirim ${label} kamu di chat ini (satu pesan).\n\nContoh: https://muse.ai/invite/3015497298687799`,
  promoAsk:
    "Punya kode promo? Ketik kodenya di chat ini, atau tap “Lewati” untuk lanjut tanpa promo.",
  promoEnterPrompt: "Ketik kode promo kamu (satu pesan), atau /cancel untuk batal.",
  promoApplied: (code, discount) =>
    `✅ Promo <code>${code}</code> dipakai! Diskon ${rupiah(discount)}.`,
  promoInvalid: (reason) =>
    reason === "not_found"
      ? "❌ Kode promo tidak ditemukan. Periksa lagi ejaannya."
      : reason === "expired"
        ? "❌ Kode promo sudah kedaluwarsa."
        : reason === "exhausted"
          ? "❌ Kuota kode promo sudah habis."
          : "❌ Kode promo ini sudah pernah kamu pakai.",
  promoSkipped: "Lanjut tanpa promo.",
  payTitle: (p) =>
    `🧾 <b>Ringkasan Order</b>\n\n${p.name} × ${p.qty}\n` +
    `Subtotal: ${rupiah(p.subtotal)}\n` +
    (p.discount > 0 ? `Diskon promo: −${rupiah(p.discount)}\n` : "") +
    `Total bayar: <b>${rupiah(p.total)}</b>\n` +
    `💳 Saldo kamu: ${rupiah(p.balance)}\n\nPilih cara bayar:`,
  payWallet: "💳 Bayar dari Saldo",
  payQris: "💳 Bayar via QRIS",
  payWalletShort: "💳 Saldo",
  walletInsufficient:
    "❌ Saldo kamu tidak cukup untuk order ini. Isi saldo dulu atau bayar via QRIS.",
  walletPaid: (orderId) =>
    `✅ Dibayar dari saldo! Order ${orderId} sedang diproses ke supplier…`,
  walletRefunded: "Saldo sudah dikembalikan ke dompet kamu.",
  qrisLink: (orderId, total, uniqueAmount, expiresAt) =>
    `🧾 Order ${orderId} dibuat — total ${rupiah(total)}.\n\n` +
    `Scan QR di atas dengan aplikasi apa pun (GoPay, OVO, DANA, m-banking):\n` +
    `• Nominal: <b>${rupiah(uniqueAmount)}</b> — HARUS pas, jangan dibulatkan\n` +
    `• Berlaku sampai: ${wibTime(expiresAt)} WIB (5 menit)\n\n` +
    `Setelah dibayar, order otomatis diteruskan ke supplier dan hasilnya dikirim ke chat ini. ` +
    `Kalau QR kedaluwarsa, buat order baru dari katalog. Lacak dengan /order ${orderId}`,
  qrisFailed: (reason) =>
    `❌ QR pembayaran belum bisa dibuat sekarang.${reason ? `\nAlasan: ${reason}` : ""}\n\nCoba lagi nanti atau hubungi admin.`,
  qrisCheckButton: "✅ Saya Sudah Bayar",
  qrisCheckFoundOrder: (orderId) =>
    `✅ Pembayaran diterima! Order ${orderId} sedang diproses ke supplier — hasilnya dikirim ke chat ini.`,
  qrisCheckFoundTopup: (amount) =>
    `✅ Pembayaran diterima! Saldo ${rupiah(amount)} sudah masuk ke dompet kamu.`,
  qrisCheckPending:
    "⏳ Pembayaran belum masuk. Kalau kamu baru saja bayar, tunggu beberapa detik lalu tap tombolnya lagi.",
  qrisCheckExpired:
    "❌ QR sudah kedaluwarsa (5 menit). Buat order baru dari katalog.",
  qrisCheckNoSession:
    "⚠️ Sistem belum bisa mengecek pembayaran otomatis — sesi GoBiz belum login. Hubungi admin.",
  qrisCheckFailed: "❌ Gagal mengecek pembayaran. Coba lagi sebentar.",
  qrisCheckTooSoon: "⏳ Baru saja dicek — tunggu beberapa detik lagi.",
  freeOrderDone: (orderId) =>
    `✅ Order ${orderId} gratis berkat promo — langsung diproses ke supplier…`,
  walletTitle: (balance, totalTopup, totalSpent) =>
    `💳 <b>Saldo Saya</b>\n\nSaldo: <b>${rupiah(balance)}</b>\n` +
    `Total isi saldo: ${rupiah(totalTopup)}\nTotal belanja: ${rupiah(totalSpent)}\n\n` +
    `Saldo bisa dipakai bayar order apa pun — tanpa biaya tambahan.`,
  topupTitle: "➕ <b>Isi Saldo</b>\n\nPilih nominal, atau ketik nominal sendiri (minimal Rp10.000).",
  topupCustomPrompt:
    "Ketik nominal isi saldo (angka, minimal 10000), atau /cancel untuk batal.",
  topupInvalid: "Nominal harus angka minimal 10000. Coba lagi, atau /cancel untuk batal.",
  topupCreated: (amount, uniqueAmount, expiresAt) =>
    `💳 Isi saldo ${rupiah(amount)}.\n\nScan QR di atas:\n` +
    `• Nominal: <b>${rupiah(uniqueAmount)}</b> — HARUS pas, jangan dibulatkan\n` +
    `• Berlaku sampai: ${wibTime(expiresAt)} WIB (5 menit)\n\nSaldo masuk otomatis setelah pembayaran lunas.`,
  topupFailed: "❌ QR pembayaran belum bisa dibuat. Coba lagi nanti.",
  txnsTitle: "📜 <b>Riwayat Dompet</b>\n\n10 transaksi terakhir:",
  txnsEmpty: "Belum ada transaksi dompet.",
  txnLine: (t) =>
    `${t.amount >= 0 ? "➕" : "➖"} ${rupiah(Math.abs(t.amount))} — ${t.kind}${t.note ? ` (${t.note})` : ""} • ${t.date}`,
  ordersTitle: "📦 <b>Pesanan Saya</b>\n\nTap order untuk lihat detail:",
  ordersEmpty: "Kamu belum punya order. Yuk cek katalog! 🛍️",
  orderDetail: (o) =>
    `📦 <b>Order ${o.id}</b>\n\nProduk: ${o.product} × ${o.qty}\n` +
    `Total: ${rupiah(o.total)}\nStatus: ${o.status}\n` +
    (o.items.length ? `\nHasil:\n${o.items.map((i) => `• ${i}`).join("\n")}` : "") +
    (o.link ? `\n🔗 Link: ${o.link}` : "") +
    (o.error ? `\n⚠️ ${o.error}` : ""),
  statusLabel: (status) =>
    status === "awaiting_payment"
      ? "⏳ Menunggu pembayaran"
      : status === "paid"
        ? "✅ Dibayar — antre proses"
        : status === "processing"
          ? "⚙️ Sedang diproses"
          : status === "delivered"
            ? "🎉 Selesai / Terkirim"
            : status === "refunded"
              ? "↩️ Di-refund"
              : "❌ Gagal",
  referralTitle: (code, link, invited, completed, credit) =>
    `🎁 <b>Referral</b>\n\nKode kamu: <code>${code}</code>\n` +
    `Link undang: ${link}\n\n` +
    `👥 Teman diundang: ${invited}\n✅ Selesai order pertama: ${completed}\n` +
    `💰 Kredit didapat: ${rupiah(credit)}\n\n` +
    `Aturan: kamu dapat kredit dompet ${rupiah(1500)} setiap teman yang kamu undang ` +
    `menyelesaikan order berbayar PERTAMA mereka.`,
  topReferrersTitle: "🏆 <b>Top Referral</b>",
  topReferrersEmpty: "Belum ada yang mengundang teman. Jadilah yang pertama! 🎁",
  topReferrerLine: (rank, tgId, invited, rewarded) =>
    `${rank}. User ${tgId} — ${invited} undangan, ${rewarded} berhasil`,
  promoTitle:
    "🎟️ <b>Promo</b>\n\nPunya kode promo? Masukkan kodenya saat checkout — diskon langsung memotong total bayar. Satu kode hanya bisa dipakai sekali per pengguna.",
  supportTitle:
    "💬 <b>Support</b>\n\nAda kendala order / pembayaran? Hubungi admin: @Dimas0227\n\nAtau kirim pesan langsung dari sini — admin akan membalas ke chat ini.",
  supportAsk: "✉️ Tulis pesan kamu untuk admin (satu pesan), atau /cancel untuk batal.",
  supportSent: "✅ Pesan kamu sudah diteruskan ke admin. Tunggu balasannya di chat ini ya.",
  supportForwarded: (name, tgId, text) =>
    `📩 <b>Pesan support</b> dari ${name} (tg:${tgId}):\n\n${text}\n\n<i>Balas pesan ini (reply) untuk menjawab pembeli.</i>`,
  supportReplyHint: "Balas pesan support yang diteruskan (reply) untuk menjawab pembeli.",
  policyTitle:
    "📜 <b>Kebijakan Toko</b>\n\n" +
    "🚀 <b>Pengiriman</b>\nProduk digital dikirim instan setelah pembayaran lunas. Proses ke supplier maksimal 10 menit — kalau lebih dari itu, hubungi support.\n\n" +
    "↩️ <b>Refund</b>\nKalau fulfillment gagal setelah kamu bayar: bayar saldo kembali ke dompet otomatis; bayar QRIS ditransfer balik manual oleh admin (maks. 1 hari kerja) — hubungi support kalau belum masuk.\n\n" +
    "🛡️ <b>Garansi</b>\nAkun bergaransi penggantian 1 hari sejak diterima, selama dipakai sesuai aturan produk. Token yang sudah dipakai tidak bisa digaransi.\n\n" +
    "💳 <b>Pembayaran</b>\nQRIS dinamis (GoPay, OVO, DANA, m-banking apa pun) atau saldo dompet.\n\n" +
    "📩 <b>Bantuan</b>\nSupport: @Dimas0227",
  resellerTitle:
    "🤝 <b>Reseller & API</b>\n\nProgram reseller & API akses masih disiapkan — segera hadir.\n\nMau jadi reseller Unicodes Digital lebih awal? Hubungi admin: @Dimas0227",
  langTitle: "🌐 <b>Bahasa / Language</b>\n\nPilih bahasa bot:",
  langSet: "✅ Bahasa diubah ke Bahasa Indonesia.",
  orderNotFound: "Order tidak ditemukan.",
  orderUsage: "Pakai: /order <id> — contoh: /order UD-XXXX",
  fulfillUsage: "Pakai: /fulfill <id>",
  fulfillProcessing: (id) => `Memproses order ${id} ke supplier...`,
  adminOnly: "Perintah ini khusus admin.",
  promoAddUsage:
    "Pakai: /promo_add <KODE> <percent|fixed> <nilai> [max_uses] [days_valid]\nContoh: /promo_add HEMAT10 percent 10 5 30",
  promoAdded: (code, kind, value) =>
    `✅ Promo <code>${code}</code> dibuat (${kind} ${kind === "percent" ? `${value}%` : rupiah(value)}).`,
  promoListTitle: "🎟️ <b>Daftar Promo</b>",
  promoListEmpty: "Belum ada kode promo.",
  unknownCommand: "Perintah tidak dikenal. Ketik /menu untuk lihat menu.",
  cancelled: "Dibatalkan. Ketik /menu untuk kembali ke menu.",
};

const en: Strings = {
  welcome: (name, orders, spent) =>
    `⚡ Welcome to Unicodes Digital, ${name}! 💜\n\n` +
    `AI & digital products store — honest prices, instant delivery.\n\n` +
    `🛍️ Here you can buy:\n` +
    `• Muse AI Token — 1B to 30B recharge\n\n` +
    `💳 How to shop: pick a product → pay via QRIS → the token is sent straight to your account. Or top up your balance in the My Balance menu first, so your next checkout is faster.\n\n` +
    (orders > 0
      ? `📦 ${orders} ${orders === 1 ? "order" : "orders"} • Total spent: ${rupiah(spent)}\n\n`
      : "") +
    `Pick a menu below to start:`,
  menuPrompt: "Pick a menu below:",
  catalog: "🛍️ Product Catalog",
  myBalance: "💳 My Balance",
  myOrders: "📦 My Orders",
  referral: "🎁 Referral",
  promo: "🎟️ Promo",
  support: "💬 Support",
  policy: "📜 Store Policy",
  reseller: "🤝 Reseller & API",
  language: "🌐 Language",
  backMenu: "⬅️ Main Menu",
  catalogTitle:
    "🛍️ <b>Product Catalog</b>\n\nPick a product to see details & buy. Prices are all-in.",
  catalogPickCategory: "🛍️ <b>Product Catalog</b>\n\nPick a category first:",
  categoryAll: "All",
  productDetail: (p) =>
    `${p.badge ? `🏷️ <b>${p.badge}</b>\n` : ""}<b>${p.name}</b>\n\n${p.desc}\n\n` +
    (p.features.length ? p.features.map((f) => `✅ ${f}`).join("\n") + "\n\n" : "") +
    `💰 Price: <b>${rupiah(p.price)}</b>${p.unit ? ` ${p.unit}` : ""}\n` +
    `📦 Stock: ${p.stock}\n` +
    (p.targetNote ? `\n🎯 ${p.targetNote}` : ""),
  comingSoon: "⏳ Coming soon — this product can't be bought yet. Check back later!",
  outOfStock: "🔴 Out of stock — this product is currently unavailable. Check back later!",
  stockLabel: (state) =>
    state === "available" ? "Available" : state === "out_of_stock" ? "Out of stock" : "Coming soon",
  qtyPrompt: "Choose how many you want to buy:",
  qtyCustomPrompt: "Type the quantity (a number 1–30), or /cancel to abort.",
  qtyInvalid: "Quantity must be a number 1–30. Try again, or /cancel to abort.",
  targetPrompt: (label) =>
    `🎯 Send your ${label} here in this chat (one message).\n\nExample: https://muse.ai/invite/3015497298687799`,
  promoAsk:
    "Have a promo code? Type it here, or tap “Skip” to continue without one.",
  promoEnterPrompt: "Type your promo code (one message), or /cancel to abort.",
  promoApplied: (code, discount) =>
    `✅ Promo <code>${code}</code> applied! Discount ${rupiah(discount)}.`,
  promoInvalid: (reason) =>
    reason === "not_found"
      ? "❌ Promo code not found. Check the spelling."
      : reason === "expired"
        ? "❌ This promo code has expired."
        : reason === "exhausted"
          ? "❌ This promo code's quota is used up."
          : "❌ You've already used this promo code.",
  promoSkipped: "Continuing without a promo.",
  payTitle: (p) =>
    `🧾 <b>Order Summary</b>\n\n${p.name} × ${p.qty}\n` +
    `Subtotal: ${rupiah(p.subtotal)}\n` +
    (p.discount > 0 ? `Promo discount: −${rupiah(p.discount)}\n` : "") +
    `Total to pay: <b>${rupiah(p.total)}</b>\n` +
    `💳 Your balance: ${rupiah(p.balance)}\n\nChoose a payment method:`,
  payWallet: "💳 Pay from Balance",
  payQris: "💳 Pay via QRIS",
  payWalletShort: "💳 Balance",
  walletInsufficient:
    "❌ Your balance isn't enough for this order. Top up first or pay via QRIS.",
  walletPaid: (orderId) =>
    `✅ Paid from balance! Order ${orderId} is being processed…`,
  walletRefunded: "Your balance has been refunded to your wallet.",
  qrisLink: (orderId, total, uniqueAmount, expiresAt) =>
    `🧾 Order ${orderId} created — total ${rupiah(total)}.\n\n` +
    `Scan the QR above with any app (GoPay, OVO, DANA, mobile banking):\n` +
    `• Amount: <b>${rupiah(uniqueAmount)}</b> — must match exactly, don't round it\n` +
    `• Valid until: ${wibTime(expiresAt)} WIB (5 minutes)\n\n` +
    `Once paid, the order is forwarded to the supplier automatically and the result is sent to this chat. ` +
    `If the QR expires, create a new order from the catalog. Track it with /order ${orderId}`,
  qrisFailed: (reason) =>
    `❌ Payment QR couldn't be created right now.${reason ? `\nReason: ${reason}` : ""}\n\nTry again later or contact support.`,
  qrisCheckButton: "✅ I've Paid",
  qrisCheckFoundOrder: (orderId) =>
    `✅ Payment received! Order ${orderId} is being processed — the result will be sent to this chat.`,
  qrisCheckFoundTopup: (amount) =>
    `✅ Payment received! ${rupiah(amount)} has been added to your wallet.`,
  qrisCheckPending:
    "⏳ Payment hasn't arrived yet. If you just paid, wait a few seconds and tap the button again.",
  qrisCheckExpired:
    "❌ This QR has expired (5 minutes). Create a new order from the catalog.",
  qrisCheckNoSession:
    "⚠️ The system can't check payments automatically yet — the GoBiz session isn't logged in. Contact the admin.",
  qrisCheckFailed: "❌ Couldn't check the payment. Try again in a moment.",
  qrisCheckTooSoon: "⏳ Just checked — wait a few more seconds.",
  freeOrderDone: (orderId) =>
    `✅ Order ${orderId} is free thanks to the promo — processing it right away…`,
  walletTitle: (balance, totalTopup, totalSpent) =>
    `💳 <b>My Balance</b>\n\nBalance: <b>${rupiah(balance)}</b>\n` +
    `Total topped up: ${rupiah(totalTopup)}\nTotal spent: ${rupiah(totalSpent)}\n\n` +
    `Balance can pay for any order — no extra fees.`,
  topupTitle: "➕ <b>Top Up</b>\n\nPick an amount, or type your own (minimum Rp10.000).",
  topupCustomPrompt: "Type the top-up amount (a number, minimum 10000), or /cancel to abort.",
  topupInvalid: "Amount must be a number of at least 10000. Try again, or /cancel to abort.",
  topupCreated: (amount, uniqueAmount, expiresAt) =>
    `💳 Top up ${rupiah(amount)}.\n\nScan the QR above:\n` +
    `• Amount: <b>${rupiah(uniqueAmount)}</b> — must match exactly\n` +
    `• Valid until: ${wibTime(expiresAt)} WIB (5 minutes)\n\nBalance is credited automatically once the payment settles.`,
  topupFailed: "❌ Payment QR couldn't be created. Try again later.",
  txnsTitle: "📜 <b>Wallet History</b>\n\nLast 10 transactions:",
  txnsEmpty: "No wallet transactions yet.",
  txnLine: (t) =>
    `${t.amount >= 0 ? "➕" : "➖"} ${rupiah(Math.abs(t.amount))} — ${t.kind}${t.note ? ` (${t.note})` : ""} • ${t.date}`,
  ordersTitle: "📦 <b>My Orders</b>\n\nTap an order to see details:",
  ordersEmpty: "You don't have any orders yet. Check the catalog! 🛍️",
  orderDetail: (o) =>
    `📦 <b>Order ${o.id}</b>\n\nProduct: ${o.product} × ${o.qty}\n` +
    `Total: ${rupiah(o.total)}\nStatus: ${o.status}\n` +
    (o.items.length ? `\nResult:\n${o.items.map((i) => `• ${i}`).join("\n")}` : "") +
    (o.link ? `\n🔗 Link: ${o.link}` : "") +
    (o.error ? `\n⚠️ ${o.error}` : ""),
  statusLabel: (status) =>
    status === "awaiting_payment"
      ? "⏳ Waiting for payment"
      : status === "paid"
        ? "✅ Paid — queued"
        : status === "processing"
          ? "⚙️ Processing"
          : status === "delivered"
            ? "🎉 Done / Delivered"
            : status === "refunded"
              ? "↩️ Refunded"
              : "❌ Failed",
  referralTitle: (code, link, invited, completed, credit) =>
    `🎁 <b>Referral</b>\n\nYour code: <code>${code}</code>\n` +
    `Invite link: ${link}\n\n` +
    `👥 Friends invited: ${invited}\n✅ Finished their first order: ${completed}\n` +
    `💰 Credit earned: ${rupiah(credit)}\n\n` +
    `Rule: you earn ${rupiah(1500)} wallet credit for every invited friend who ` +
    `completes their FIRST paid order.`,
  topReferrersTitle: "🏆 <b>Top Referrals</b>",
  topReferrersEmpty: "Nobody has invited friends yet. Be the first! 🎁",
  topReferrerLine: (rank, tgId, invited, rewarded) =>
    `${rank}. User ${tgId} — ${invited} invites, ${rewarded} completed`,
  promoTitle:
    "🎟️ <b>Promo</b>\n\nHave a promo code? Enter it at checkout — the discount cuts your total right away. Each code can be used once per user.",
  supportTitle:
    "💬 <b>Support</b>\n\nOrder / payment issues? Contact admin: @Dimas0227\n\nOr send a message right from here — the admin will reply in this chat.",
  supportAsk: "✉️ Write your message to the admin (one message), or /cancel to abort.",
  supportSent: "✅ Your message was forwarded to the admin. They'll reply here.",
  supportForwarded: (name, tgId, text) =>
    `📩 <b>Support message</b> from ${name} (tg:${tgId}):\n\n${text}\n\n<i>Reply to this message to answer the buyer.</i>`,
  supportReplyHint: "Reply to a forwarded support message to answer the buyer.",
  policyTitle:
    "📜 <b>Store Policy</b>\n\n" +
    "🚀 <b>Delivery</b>\nDigital products are delivered instantly after payment. Supplier processing takes at most 10 minutes — if it takes longer, contact support.\n\n" +
    "↩️ <b>Refund</b>\nIf fulfillment fails after you paid: balance payments go back to your wallet automatically; QRIS payments are refunded manually by the admin (max 1 business day) — contact support if it hasn't arrived.\n\n" +
    "🛡️ <b>Warranty</b>\nAccounts come with a 1-day replacement warranty from delivery, when used per the product rules. Tokens already used can't be warrantied.\n\n" +
    "💳 <b>Payment</b>\nDynamic QRIS (GoPay, OVO, DANA, any mobile banking) or wallet balance.\n\n" +
    "📩 <b>Help</b>\nSupport: @Dimas0227",
  resellerTitle:
    "🤝 <b>Reseller & API</b>\n\nThe reseller & API program is still being prepared — coming soon.\n\nWant to become a Unicodes Digital reseller early? Contact admin: @Dimas0227",
  langTitle: "🌐 <b>Language</b>\n\nChoose the bot language:",
  langSet: "✅ Language changed to English.",
  orderNotFound: "Order not found.",
  orderUsage: "Usage: /order <id> — e.g. /order UD-XXXX",
  fulfillUsage: "Usage: /fulfill <id>",
  fulfillProcessing: (id) => `Processing order ${id} to the supplier...`,
  adminOnly: "This command is admin-only.",
  promoAddUsage:
    "Usage: /promo_add <CODE> <percent|fixed> <value> [max_uses] [days_valid]\nExample: /promo_add SAVE10 percent 10 5 30",
  promoAdded: (code, kind, value) =>
    `✅ Promo <code>${code}</code> created (${kind} ${kind === "percent" ? `${value}%` : rupiah(value)}).`,
  promoListTitle: "🎟️ <b>Promo List</b>",
  promoListEmpty: "No promo codes yet.",
  unknownCommand: "Unknown command. Type /menu to see the menu.",
  cancelled: "Cancelled. Type /menu to go back to the menu.",
};

export const STR: Record<Lang, Strings> = { id, en };

export function t(lang: Lang): Strings {
  return STR[lang] ?? STR.id;
}
