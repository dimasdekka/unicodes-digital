//! Teks bot Telegram — port `src/bot/i18n.ts` (Indonesia default & English).
//! Semua layar bot mengambil teks dari sini lewat `t(lang)` —
//! tambah bahasa baru = tambah satu `Strings` di bawah.
//! TEKS DISALIN PERSIS dari versi TS (termasuk emoji, garis, dan spasi).

use crate::catalog::StockState;
use crate::lib::wallet::Lang;

/// `Rp2.000` — pemisah ribuan "." seperti `toLocaleString("id-ID")`.
pub fn rupiah(n: i64) -> String {
    let neg = n < 0;
    let digits = n.unsigned_abs().to_string();
    let mut out = String::new();
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push('.');
        }
        out.push(c);
    }
    format!("Rp{}{}", if neg { "-" } else { "" }, out)
}

/// Jam (HH:mm) dari ISO string, zona WIB — untuk kedaluwarsa QR di caption.
pub fn wib_time(iso: &str) -> String {
    match chrono::DateTime::parse_from_rfc3339(iso) {
        Ok(dt) => dt
            .with_timezone(&chrono::FixedOffset::east_opt(7 * 3600).unwrap())
            .format("%H:%M")
            .to_string(),
        Err(_) => "--:--".to_string(),
    }
}

/// Data layar /start ala Lhie: profil pembeli + aktivitas toko.
/// channelUrl kosong = channel belum dikonfigurasi -> baris promo channel
/// dan tombol Ikuti Channel tidak ditampilkan.
#[derive(Debug, Clone)]
pub struct WelcomeParams {
    pub name: String,
    pub tg_id: i64,
    pub username: Option<String>,
    pub total_spent_idr: i64,
    pub balance_idr: i64,
    pub total_users: i64,
    pub delivered_orders: i64,
    pub channel_url: String,
    pub hour_wib: i64,
}

/// Parameter teks detail produk — cerminan i18n TS; tidak semua field
/// dipakai setiap bahasa (dipakai tes + render detail).
#[allow(dead_code)]
#[derive(Debug, Clone)]
pub struct ProductDetailParams {
    pub name: String,
    pub desc: String,
    pub price: i64,
    pub unit: Option<String>,
    pub badge: Option<String>,
    pub features: Vec<String>,
    pub stock: String,
    pub target_note: Option<String>,
}

#[derive(Debug, Clone)]
pub struct QtyTitleParams {
    pub code: String,
    pub price: i64,
    pub stock: String,
}

#[derive(Debug, Clone)]
pub struct PayTitleParams {
    pub name: String,
    pub qty: i64,
    pub subtotal: i64,
    pub discount: i64,
    pub total: i64,
    pub balance: i64,
    /// Kode invite (hanya produk on-demand) — tampil di ringkasan.
    pub code: Option<String>,
}

#[derive(Debug, Clone)]
pub struct TxnLineParams {
    pub kind: String,
    pub amount: i64,
    pub date: String,
    pub note: Option<String>,
}

#[derive(Debug, Clone)]
pub struct OrderDetailParams {
    pub id: String,
    pub product: String,
    pub qty: i64,
    pub total: i64,
    pub status: String,
    pub items: Vec<String>,
    pub link: Option<String>,
    pub error: Option<String>,
}

/// Kumpulan string i18n per bahasa — cerminan i18n.ts; tidak semua
/// string dipakai setiap jalur (dipakai tes + render bot).
#[allow(dead_code)]
pub struct Strings {
    pub welcome: fn(&WelcomeParams) -> String,
    pub menu_prompt: &'static str,
    pub catalog: &'static str,
    pub my_balance: &'static str,
    pub my_orders: &'static str,
    pub referral: &'static str,
    pub promo: &'static str,
    pub support: &'static str,
    pub policy: &'static str,
    pub reseller: &'static str,
    pub language: &'static str,
    pub follow_channel: &'static str,
    pub back_menu: &'static str,
    pub catalog_title: &'static str,
    pub catalog_pick_category: &'static str,
    pub category_all: &'static str,
    pub product_detail: fn(&ProductDetailParams) -> String,
    pub coming_soon: &'static str,
    pub out_of_stock: fn(&str) -> String,
    /// Kode invite sudah dipakai 30x (maks penebusan Muse).
    pub code_used_up: fn(&str) -> String,
    pub stock_label: fn(StockState) -> String,
    pub qty_prompt: &'static str,
    pub qty_custom_prompt: &'static str,
    pub qty_invalid: &'static str,
    pub target_prompt: fn(&str) -> String,
    /// Langkah-langkah kode invite ala Sondev (kode dulu, baru jumlah).
    pub code_prompt: &'static str,
    /// Teks kiriman tidak mengandung kode invite yang bisa dibaca.
    pub code_not_found: &'static str,
    /// Layar pilih jumlah setelah kode diterima (harga dasar + stok live).
    pub qty_title: fn(&QtyTitleParams) -> String,
    pub promo_ask: &'static str,
    pub promo_enter_prompt: &'static str,
    pub promo_applied: fn(&str, i64) -> String,
    pub promo_invalid: fn(&str) -> String,
    pub promo_skipped: &'static str,
    pub pay_title: fn(&PayTitleParams) -> String,
    pub pay_wallet: &'static str,
    pub pay_qris: &'static str,
    pub pay_wallet_short: &'static str,
    pub wallet_insufficient: &'static str,
    pub wallet_paid: fn(&str) -> String,
    pub wallet_refunded: &'static str,
    /// Caption foto QR order: nominal unik + kedaluwarsa (jam WIB).
    pub qris_link: fn(&str, i64, i64, &str) -> String,
    /// Pesan gagal membuat QR (dengan alasan, kalau ada).
    pub qris_failed: fn(Option<&str>) -> String,
    /// Tombol di foto QR: pembeli menekan setelah bayar -> cek feed sekarang.
    pub qris_check_button: &'static str,
    pub qris_check_found_order: fn(&str) -> String,
    pub qris_check_found_topup: fn(i64) -> String,
    pub qris_check_pending: &'static str,
    pub qris_check_expired: &'static str,
    pub qris_check_no_session: &'static str,
    pub qris_check_failed: &'static str,
    pub qris_check_too_soon: &'static str,
    /// Wajib-join channel komunitas.
    pub join_channel_text: &'static str,
    pub join_channel_button: &'static str,
    pub join_channel_check_button: &'static str,
    pub join_channel_not_yet: &'static str,
    /// Pembayaran Midtrans Snap.
    pub pay_midtrans: &'static str,
    pub midtrans_open_button: &'static str,
    pub midtrans_link: fn(&str, i64, &str) -> String,
    pub midtrans_topup_created: fn(i64, &str) -> String,
    pub midtrans_failed: fn(Option<&str>) -> String,
    pub midtrans_check_no_config: &'static str,
    pub free_order_done: fn(&str) -> String,
    pub wallet_title: fn(i64, i64, i64) -> String,
    pub topup_title: &'static str,
    pub topup_custom_prompt: &'static str,
    pub topup_invalid: &'static str,
    /// Caption foto QR top-up: nominal unik + kedaluwarsa (jam WIB).
    pub topup_created: fn(i64, i64, &str) -> String,
    pub topup_failed: &'static str,
    pub txns_title: &'static str,
    pub txns_empty: &'static str,
    pub txn_line: fn(&TxnLineParams) -> String,
    pub orders_title: &'static str,
    pub orders_empty: &'static str,
    pub order_detail: fn(&OrderDetailParams) -> String,
    pub status_label: fn(&str) -> String,
    pub referral_title: fn(&str, &str, i64, i64, i64) -> String,
    pub top_referrers_title: &'static str,
    pub top_referrers_empty: &'static str,
    pub top_referrer_line: fn(i64, i64, i64, i64) -> String,
    pub promo_title: &'static str,
    pub support_title: &'static str,
    pub support_ask: &'static str,
    /// Tiket baru dibuat dari pesan user.
    pub ticket_created: fn(&str) -> String,
    /// Pesan user ditambahkan ke tiket yang masih terbuka.
    pub ticket_appended: fn(&str) -> String,
    /// Daftar tiket milik user (layar "Tiket Saya").
    pub my_tickets_title: &'static str,
    pub my_tickets_empty: &'static str,
    pub my_ticket_line: fn(&str, &str, &str) -> String,
    /// Layar pilih order sebelum buat tiket support.
    pub support_pick_order_title: &'static str,
    /// Tombol "Tanpa order / Lainnya" di layar pilih order.
    pub support_pick_no_order: &'static str,
    /// Teks status order yang masih berjalan di baris pilih order.
    pub support_order_processing: &'static str,
    /// Satu baris tombol pilih order: (awalan status, id, produk, qty, total).
    pub support_order_row: fn(&str, &str, &str, i64, i64) -> String,
    /// Teks minta pesan setelah order dipilih.
    pub support_ask_with_order: fn(&str) -> String,
    pub policy_title: &'static str,
    pub reseller_title: &'static str,
    pub lang_title: &'static str,
    pub lang_set: &'static str,
    pub order_not_found: &'static str,
    pub order_usage: &'static str,
    pub fulfill_usage: &'static str,
    pub fulfill_processing: fn(&str) -> String,
    pub admin_only: &'static str,
    pub promo_add_usage: &'static str,
    pub promo_added: fn(&str, &str, i64) -> String,
    pub promo_list_title: &'static str,
    pub promo_list_empty: &'static str,
    pub unknown_command: &'static str,
    pub cancelled: &'static str,
}

// ------------------------------------------------------------------ ID ---

fn id_welcome(p: &WelcomeParams) -> String {
    let (greet, icon) = if (4..10).contains(&p.hour_wib) {
        ("Selamat pagi", "🌅")
    } else if (10..15).contains(&p.hour_wib) {
        ("Selamat siang", "☀️")
    } else if (15..18).contains(&p.hour_wib) {
        ("Selamat sore", "🌇")
    } else {
        ("Selamat malam", "🌙")
    };
    format!(
        "✦ UNICODES DIGITAL\n━━━━━━━━━━━━━━━━━━\n{icon} {greet}, {}!\nSelamat datang di toko digital kami.\n\n👤 INFORMASI AKUN\n┌ ID Telegram : {}\n├ Username    : {}\n├ Total Belanja : {}\n└ Saldo Aktif : {}\n\n📊 STATISTIK TOKO\n├ 👥 Total Pengguna : {}\n└ ✅ Pesanan Sukses : {}\n\n━━━━━━━━━━━━━━━━━━\n✨ Semua kebutuhan digitalmu,\n   dalam satu tempat.\n\n{}Silakan pilih menu di bawah untuk mulai",
        p.name,
        p.tg_id,
        p.username.as_ref().map(|u| format!("@{u}")).unwrap_or_else(|| "—".to_string()),
        rupiah(p.total_spent_idr),
        rupiah(p.balance_idr),
        p.total_users,
        p.delivered_orders,
        if p.channel_url.is_empty() {
            String::new()
        } else {
            "📣 Promo, update stok, dan bukti transaksi terbaru tersedia di channel resmi Unicodes Digital.\n\n".to_string()
        },
    )
}

fn id_product_detail(p: &ProductDetailParams) -> String {
    let mut s = String::new();
    if let Some(badge) = &p.badge {
        s.push_str(&format!("🏷️ <b>{badge}</b>\n"));
    }
    s.push_str(&format!(
        "<b>{}</b>\n\n💰 Harga: <b>{}</b>/unit\n📦 Stok: {}\n\n📋 Detail produk\n{}\n",
        p.name,
        rupiah(p.price),
        p.stock,
        p.desc,
    ));
    if !p.features.is_empty() {
        s.push_str(&p.features.iter().map(|f| format!("✅ {f}")).collect::<Vec<_>>().join("\n"));
        s.push('\n');
    }
    if let Some(note) = &p.target_note {
        s.push_str(&format!("\n🎯 {note}"));
    }
    s
}

fn id_out_of_stock(name: &str) -> String {
    format!(
        "❌ Stok Habis\n━━━━━━━━━━━━━━\n✨ {name}\nProduk ini sedang habis dan belum bisa dipesan.\n\nCoba lihat katalog untuk produk lain ya."
    )
}

fn id_code_used_up(code: &str) -> String {
    format!(
        "🚫 Kode <code>{code}</code> sudah dipakai 30x. Muse tidak menerima penebusan lagi untuk kode ini — pakai kode invite lain ya."
    )
}

fn id_stock_label(state: StockState) -> String {
    match state {
        StockState::Available => "Tersedia".to_string(),
        StockState::OutOfStock => "Stok Habis".to_string(),
        StockState::ComingSoon => "Segera Hadir".to_string(),
    }
}

fn id_target_prompt(label: &str) -> String {
    format!(
        "🎯 Kirim {label} kamu di chat ini (satu pesan).\n\nLink ini buat nunjukin tokennya dikirim ke akun Muse yang mana.\nCara ambil: buka aplikasi Muse → Invite → salin link."
    )
}

fn id_qty_title(p: &QtyTitleParams) -> String {
    format!(
        "⚡ Kode Invite Muse: <code>{}</code>\n\nHarga dasar: {} / 1B token.\n📦 Stok ready: {}\n\nPilih jumlah token yang mau ditebus:",
        p.code,
        rupiah(p.price),
        p.stock,
    )
}

fn id_promo_applied(code: &str, discount: i64) -> String {
    format!("✅ Promo <code>{code}</code> dipakai! Diskon {}.", rupiah(discount))
}

fn id_promo_invalid(reason: &str) -> String {
    match reason {
        "not_found" => "❌ Kode promo tidak ditemukan. Periksa lagi ejaannya.".to_string(),
        "expired" => "❌ Kode promo sudah kedaluwarsa.".to_string(),
        "exhausted" => "❌ Kuota kode promo sudah habis.".to_string(),
        _ => "❌ Kode promo ini sudah pernah kamu pakai.".to_string(),
    }
}

fn id_pay_title(p: &PayTitleParams) -> String {
    let mut s = format!(
        "💳 <b>RINGKASAN PEMBAYARAN</b>\n────────────────\n📦 Produk: {} × {}\n",
        p.name, p.qty,
    );
    if let Some(code) = &p.code {
        s.push_str(&format!("⚡ Kode Invite: <code>{code}</code>\n"));
    }
    if p.discount > 0 {
        s.push_str(&format!(
            "Subtotal: {}\n🎟️ Diskon promo: −{}\n",
            rupiah(p.subtotal),
            rupiah(p.discount),
        ));
    }
    s.push_str(&format!(
        "💰 Total: <b>{}</b>\n💳 Saldo kamu: {}\n────────────────\n\nPilih cara bayar di bawah:",
        rupiah(p.total),
        rupiah(p.balance),
    ));
    s
}

fn id_wallet_paid(order_id: &str) -> String {
    format!("✅ Dibayar dari saldo! Order {order_id} sedang diproses ke supplier…")
}

fn id_qris_link(order_id: &str, total: i64, unique_amount: i64, expires_at: &str) -> String {
    format!(
        "🧾 Order {order_id} dibuat — total {}.\n\nScan QR di atas dengan aplikasi apa pun (GoPay, OVO, DANA, m-banking):\n• Nominal: <b>{}</b> — HARUS pas, jangan dibulatkan\n• Berlaku sampai: {} WIB (5 menit)\n\nSetelah dibayar, order otomatis diteruskan ke supplier dan hasilnya dikirim ke chat ini. Kalau QR kedaluwarsa, buat order baru dari katalog. Lacak dengan /order {order_id}",
        rupiah(total),
        rupiah(unique_amount),
        wib_time(expires_at),
    )
}

fn id_qris_failed(reason: Option<&str>) -> String {
    format!(
        "❌ QR pembayaran belum bisa dibuat sekarang.{}\n\nCoba lagi nanti atau hubungi admin.",
        reason.map(|r| format!("\nAlasan: {r}")).unwrap_or_default(),
    )
}

fn id_qris_check_found_order(order_id: &str) -> String {
    format!(
        "✅ Pembayaran diterima! Order {order_id} sedang diproses ke supplier — hasilnya dikirim ke chat ini."
    )
}

fn id_qris_check_found_topup(amount: i64) -> String {
    format!(
        "✅ Pembayaran diterima! Saldo {} sudah masuk ke dompet kamu.",
        rupiah(amount),
    )
}

fn id_midtrans_link(order_id: &str, total: i64, expires_at: &str) -> String {
    format!(
        "💳 <b>Pembayaran Midtrans</b>\n\nOrder: <code>{order_id}</code>\nTotal: <b>{}</b>\n\nTap tombol di bawah untuk bayar (QRIS) — link berlaku sampai {} WIB (30 menit).\n\nSetelah bayar, tap '✅ Saya Sudah Bayar' atau tunggu dicek otomatis.",
        rupiah(total),
        wib_time(expires_at),
    )
}

fn id_midtrans_topup_created(amount: i64, expires_at: &str) -> String {
    format!(
        "💳 <b>Isi saldo {}</b>\n\nTap tombol di bawah untuk bayar (QRIS) — link berlaku sampai {} WIB (30 menit).\n\nSaldo masuk otomatis setelah pembayaran lunas.",
        rupiah(amount),
        wib_time(expires_at),
    )
}

fn id_midtrans_failed(reason: Option<&str>) -> String {
    format!(
        "❌ Pembayaran Midtrans belum bisa dibuat sekarang.{}\n\nCoba lagi nanti atau hubungi admin.",
        reason.map(|r| format!("\nAlasan: {r}")).unwrap_or_default(),
    )
}

fn id_free_order_done(order_id: &str) -> String {
    format!("✅ Order {order_id} gratis berkat promo — langsung diproses ke supplier…")
}

fn id_wallet_title(balance: i64, total_topup: i64, total_spent: i64) -> String {
    format!(
        "💳 <b>Saldo Saya</b>\n\nSaldo: <b>{}</b>\nTotal isi saldo: {}\nTotal belanja: {}\n\nSaldo bisa dipakai bayar order apa pun — tanpa biaya tambahan.",
        rupiah(balance),
        rupiah(total_topup),
        rupiah(total_spent),
    )
}

fn id_topup_created(amount: i64, unique_amount: i64, expires_at: &str) -> String {
    format!(
        "💳 Isi saldo {}.\n\nScan QR di atas:\n• Nominal: <b>{}</b> — HARUS pas, jangan dibulatkan\n• Berlaku sampai: {} WIB (5 menit)\n\nSaldo masuk otomatis setelah pembayaran lunas.",
        rupiah(amount),
        rupiah(unique_amount),
        wib_time(expires_at),
    )
}

fn id_txn_line(t: &TxnLineParams) -> String {
    format!(
        "{} {} — {}{}{} • {}",
        if t.amount >= 0 { "➕" } else { "➖" },
        rupiah(t.amount.abs()),
        t.kind,
        t.note.as_ref().map(|n| format!(" ({n})")).unwrap_or_default(),
        "",
        t.date,
    )
}

fn id_order_detail(o: &OrderDetailParams) -> String {
    let mut s = format!(
        "📦 <b>Order {}</b>\n\nProduk: {} × {}\nTotal: {}\nStatus: {}\n",
        o.id, o.product, o.qty, rupiah(o.total), o.status,
    );
    if !o.items.is_empty() {
        s.push_str(&format!(
            "\nHasil:\n{}",
            o.items.iter().map(|i| format!("• {i}")).collect::<Vec<_>>().join("\n"),
        ));
    }
    if let Some(link) = &o.link {
        s.push_str(&format!("\n🔗 Link: {link}"));
    }
    if let Some(err) = &o.error {
        s.push_str(&format!("\n⚠️ {err}"));
    }
    s
}

fn id_status_label(status: &str) -> String {
    match status {
        "awaiting_payment" => "⏳ Menunggu pembayaran".to_string(),
        "paid" => "✅ Dibayar — antre proses".to_string(),
        "processing" => "⚙️ Sedang diproses".to_string(),
        "delivered" => "🎉 Selesai / Terkirim".to_string(),
        "refunded" => "↩️ Di-refund".to_string(),
        _ => "❌ Gagal".to_string(),
    }
}

fn id_referral_title(code: &str, link: &str, invited: i64, completed: i64, credit: i64) -> String {
    format!(
        "🎁 <b>Referral</b>\n\nKode kamu: <code>{code}</code>\nLink undang: {link}\n\n👥 Teman diundang: {invited}\n✅ Selesai order pertama: {completed}\n💰 Kredit didapat: {}\n\nAturan: kamu dapat kredit dompet {} setiap teman yang kamu undang menyelesaikan order berbayar PERTAMA mereka.",
        rupiah(credit),
        rupiah(1500),
    )
}

fn id_top_referrer_line(rank: i64, tg_id: i64, invited: i64, rewarded: i64) -> String {
    format!("{rank}. User {tg_id} — {invited} undangan, {rewarded} berhasil")
}

fn id_ticket_created(id: &str) -> String {
    format!(
        "✅ Tiket <code>{id}</code> dibuat. Admin akan membalas di chat ini — cek status tiketmu di menu 💬 Support."
    )
}

fn id_ticket_appended(id: &str) -> String {
    format!(
        "✅ Pesanmu ditambahkan ke tiket <code>{id}</code> yang masih terbuka. Admin akan membalas di chat ini."
    )
}

fn id_my_ticket_line(id: &str, subject: &str, status: &str) -> String {
    format!("<code>{id}</code> — {status}\n{subject}")
}

fn id_support_order_row(prefix: &str, id: &str, product: &str, qty: i64, total: i64) -> String {
    format!("{prefix} {id} — {product} ×{qty} — {}", rupiah(total))
}

fn id_support_ask_with_order(id: &str) -> String {
    format!(
        "✉️ Jelaskan kendala order <code>{id}</code> dalam satu pesan. Admin akan membalas lewat tiket ini.\n\nAtau /cancel untuk batal."
    )
}

fn id_fulfill_processing(id: &str) -> String {
    format!("Memproses order {id} ke supplier...")
}

fn id_promo_added(code: &str, kind: &str, value: i64) -> String {
    format!(
        "✅ Promo <code>{code}</code> dibuat ({} {}).",
        kind,
        if kind == "percent" {
            format!("{value}%")
        } else {
            rupiah(value)
        },
    )
}

// ------------------------------------------------------------------ EN ---

fn en_welcome(p: &WelcomeParams) -> String {
    let greet = if (4..10).contains(&p.hour_wib) {
        "Good morning"
    } else if (10..18).contains(&p.hour_wib) {
        "Good afternoon"
    } else {
        "Good evening"
    };
    // Ikon dihitung terpisah seperti TS (siang ☀️ sampai 15, 🌇 15–18).
    let icon = if (4..10).contains(&p.hour_wib) {
        "🌅"
    } else if (10..15).contains(&p.hour_wib) {
        "☀️"
    } else if (15..18).contains(&p.hour_wib) {
        "🌇"
    } else {
        "🌙"
    };
    format!(
        "✦ UNICODES DIGITAL\n━━━━━━━━━━━━━━━━━━\n{icon} {greet}, {}!\nWelcome to our digital store.\n\n👤 ACCOUNT INFO\n┌ Telegram ID : {}\n├ Username    : {}\n├ Total Spent : {}\n└ Active Balance : {}\n\n📊 STORE STATS\n├ 👥 Total Users : {}\n└ ✅ Successful Orders : {}\n\n━━━━━━━━━━━━━━━━━━\n✨ All your digital needs,\n   in one place.\n\n{}Please choose a menu below to start",
        p.name,
        p.tg_id,
        p.username.as_ref().map(|u| format!("@{u}")).unwrap_or_else(|| "—".to_string()),
        rupiah(p.total_spent_idr),
        rupiah(p.balance_idr),
        p.total_users,
        p.delivered_orders,
        if p.channel_url.is_empty() {
            String::new()
        } else {
            "📣 Promos, stock updates, and the latest transaction proofs are available on the official Unicodes Digital channel.\n\n".to_string()
        },
    )
}

fn en_product_detail(p: &ProductDetailParams) -> String {
    let mut s = String::new();
    if let Some(badge) = &p.badge {
        s.push_str(&format!("🏷️ <b>{badge}</b>\n"));
    }
    s.push_str(&format!(
        "<b>{}</b>\n\n💰 Price: <b>{}</b>/unit\n📦 Stock: {}\n\n📋 Product details\n{}\n",
        p.name,
        rupiah(p.price),
        p.stock,
        p.desc,
    ));
    if !p.features.is_empty() {
        s.push_str(&p.features.iter().map(|f| format!("✅ {f}")).collect::<Vec<_>>().join("\n"));
        s.push('\n');
    }
    if let Some(note) = &p.target_note {
        s.push_str(&format!("\n🎯 {note}"));
    }
    s
}

fn en_out_of_stock(name: &str) -> String {
    format!(
        "❌ Out of Stock\n━━━━━━━━━━━━━━\n✨ {name}\nThis product is currently out of stock and can't be ordered.\n\nCheck the catalog for other products."
    )
}

fn en_code_used_up(code: &str) -> String {
    format!(
        "🚫 Code <code>{code}</code> is used up. Muse will not accept any further redemptions on it — please use a different invite code."
    )
}

fn en_stock_label(state: StockState) -> String {
    match state {
        StockState::Available => "Available".to_string(),
        StockState::OutOfStock => "Out of stock".to_string(),
        StockState::ComingSoon => "Coming soon".to_string(),
    }
}

fn en_target_prompt(label: &str) -> String {
    format!(
        "🎯 Send your {label} here in this chat (one message).\n\nThis tells us which Muse account the tokens go to.\nHow to get it: open the Muse app → Invite → copy the link."
    )
}

fn en_qty_title(p: &QtyTitleParams) -> String {
    format!(
        "⚡ Muse Invite Code: <code>{}</code>\n\nBase price: {} / 1B tokens.\n📦 Stock ready: {}\n\nSelect the token quantity you want to redeem:",
        p.code,
        rupiah(p.price),
        p.stock,
    )
}

fn en_promo_applied(code: &str, discount: i64) -> String {
    format!("✅ Promo <code>{code}</code> applied! Discount {}.", rupiah(discount))
}

fn en_promo_invalid(reason: &str) -> String {
    match reason {
        "not_found" => "❌ Promo code not found. Check the spelling.".to_string(),
        "expired" => "❌ This promo code has expired.".to_string(),
        "exhausted" => "❌ This promo code's quota is used up.".to_string(),
        _ => "❌ You've already used this promo code.".to_string(),
    }
}

fn en_pay_title(p: &PayTitleParams) -> String {
    let mut s = format!(
        "💳 <b>ORDER PAYMENT SUMMARY</b>\n────────────────\n📦 Product: {} × {}\n",
        p.name, p.qty,
    );
    if let Some(code) = &p.code {
        s.push_str(&format!("⚡ Invite Code: <code>{code}</code>\n"));
    }
    if p.discount > 0 {
        s.push_str(&format!(
            "Subtotal: {}\n🎟️ Promo discount: −{}\n",
            rupiah(p.subtotal),
            rupiah(p.discount),
        ));
    }
    s.push_str(&format!(
        "💰 Total: <b>{}</b>\n💳 Your balance: {}\n────────────────\n\nSelect your payment method below:",
        rupiah(p.total),
        rupiah(p.balance),
    ));
    s
}

fn en_wallet_paid(order_id: &str) -> String {
    format!("✅ Paid from balance! Order {order_id} is being processed…")
}

fn en_qris_link(order_id: &str, total: i64, unique_amount: i64, expires_at: &str) -> String {
    format!(
        "🧾 Order {order_id} created — total {}.\n\nScan the QR above with any app (GoPay, OVO, DANA, mobile banking):\n• Amount: <b>{}</b> — must match exactly, don't round it\n• Valid until: {} WIB (5 minutes)\n\nOnce paid, the order is forwarded to the supplier automatically and the result is sent to this chat. If the QR expires, create a new order from the catalog. Track it with /order {order_id}",
        rupiah(total),
        rupiah(unique_amount),
        wib_time(expires_at),
    )
}

fn en_qris_failed(reason: Option<&str>) -> String {
    format!(
        "❌ Payment QR couldn't be created right now.{}\n\nTry again later or contact support.",
        reason.map(|r| format!("\nReason: {r}")).unwrap_or_default(),
    )
}

fn en_qris_check_found_order(order_id: &str) -> String {
    format!(
        "✅ Payment received! Order {order_id} is being processed — the result will be sent to this chat."
    )
}

fn en_qris_check_found_topup(amount: i64) -> String {
    format!(
        "✅ Payment received! {} has been added to your wallet.",
        rupiah(amount),
    )
}

fn en_midtrans_link(order_id: &str, total: i64, expires_at: &str) -> String {
    format!(
        "💳 <b>Midtrans Payment</b>\n\nOrder: <code>{order_id}</code>\nTotal: <b>{}</b>\n\nTap the button below to pay (QRIS) — link valid until {} WIB (30 minutes).\n\nAfter paying, tap '✅ I've Paid' or wait for the automatic check.",
        rupiah(total),
        wib_time(expires_at),
    )
}

fn en_midtrans_topup_created(amount: i64, expires_at: &str) -> String {
    format!(
        "💳 <b>Top up {}</b>\n\nTap the button below to pay (QRIS) — link valid until {} WIB (30 minutes).\n\nBalance is credited automatically once the payment clears.",
        rupiah(amount),
        wib_time(expires_at),
    )
}

fn en_midtrans_failed(reason: Option<&str>) -> String {
    format!(
        "❌ Midtrans payment couldn't be created right now.{}\n\nTry again later or contact support.",
        reason.map(|r| format!("\nReason: {r}")).unwrap_or_default(),
    )
}

fn en_free_order_done(order_id: &str) -> String {
    format!("✅ Order {order_id} is free thanks to the promo — processing it right away…")
}

fn en_wallet_title(balance: i64, total_topup: i64, total_spent: i64) -> String {
    format!(
        "💳 <b>My Balance</b>\n\nBalance: <b>{}</b>\nTotal topped up: {}\nTotal spent: {}\n\nBalance can pay for any order — no extra fees.",
        rupiah(balance),
        rupiah(total_topup),
        rupiah(total_spent),
    )
}

fn en_topup_created(amount: i64, unique_amount: i64, expires_at: &str) -> String {
    format!(
        "💳 Top up {}.\n\nScan the QR above:\n• Amount: <b>{}</b> — must match exactly\n• Valid until: {} WIB (5 minutes)\n\nBalance is credited automatically once the payment settles.",
        rupiah(amount),
        rupiah(unique_amount),
        wib_time(expires_at),
    )
}

fn en_txn_line(t: &TxnLineParams) -> String {
    format!(
        "{} {} — {}{} • {}",
        if t.amount >= 0 { "➕" } else { "➖" },
        rupiah(t.amount.abs()),
        t.kind,
        t.note.as_ref().map(|n| format!(" ({n})")).unwrap_or_default(),
        t.date,
    )
}

fn en_order_detail(o: &OrderDetailParams) -> String {
    let mut s = format!(
        "📦 <b>Order {}</b>\n\nProduct: {} × {}\nTotal: {}\nStatus: {}\n",
        o.id, o.product, o.qty, rupiah(o.total), o.status,
    );
    if !o.items.is_empty() {
        s.push_str(&format!(
            "\nResult:\n{}",
            o.items.iter().map(|i| format!("• {i}")).collect::<Vec<_>>().join("\n"),
        ));
    }
    if let Some(link) = &o.link {
        s.push_str(&format!("\n🔗 Link: {link}"));
    }
    if let Some(err) = &o.error {
        s.push_str(&format!("\n⚠️ {err}"));
    }
    s
}

fn en_status_label(status: &str) -> String {
    match status {
        "awaiting_payment" => "⏳ Waiting for payment".to_string(),
        "paid" => "✅ Paid — queued".to_string(),
        "processing" => "⚙️ Processing".to_string(),
        "delivered" => "🎉 Done / Delivered".to_string(),
        "refunded" => "↩️ Refunded".to_string(),
        _ => "❌ Failed".to_string(),
    }
}

fn en_referral_title(code: &str, link: &str, invited: i64, completed: i64, credit: i64) -> String {
    format!(
        "🎁 <b>Referral</b>\n\nYour code: <code>{code}</code>\nInvite link: {link}\n\n👥 Friends invited: {invited}\n✅ Finished their first order: {completed}\n💰 Credit earned: {}\n\nRule: you earn {} wallet credit for every invited friend who completes their FIRST paid order.",
        rupiah(credit),
        rupiah(1500),
    )
}

fn en_top_referrer_line(rank: i64, tg_id: i64, invited: i64, rewarded: i64) -> String {
    format!("{rank}. User {tg_id} — {invited} invites, {rewarded} completed")
}

fn en_ticket_created(id: &str) -> String {
    format!(
        "✅ Ticket <code>{id}</code> created. The admin will reply here in this chat — check your ticket status in the 💬 Support menu."
    )
}

fn en_ticket_appended(id: &str) -> String {
    format!(
        "✅ Your message was added to your still-open ticket <code>{id}</code>. The admin will reply here in this chat."
    )
}

fn en_my_ticket_line(id: &str, subject: &str, status: &str) -> String {
    format!("<code>{id}</code> — {status}\n{subject}")
}

fn en_support_order_row(prefix: &str, id: &str, product: &str, qty: i64, total: i64) -> String {
    format!("{prefix} {id} — {product} ×{qty} — {}", rupiah(total))
}

fn en_support_ask_with_order(id: &str) -> String {
    format!(
        "✉️ Describe the issue with order <code>{id}</code> in one message. The admin will reply via this ticket.\n\nOr /cancel to abort."
    )
}

fn en_fulfill_processing(id: &str) -> String {
    format!("Processing order {id} to the supplier...")
}

fn en_promo_added(code: &str, kind: &str, value: i64) -> String {
    format!(
        "✅ Promo <code>{code}</code> created ({} {}).",
        kind,
        if kind == "percent" {
            format!("{value}%")
        } else {
            rupiah(value)
        },
    )
}

// -------------------------------------------------------------- lookup ---

pub static STR_ID: Strings = Strings {
    welcome: id_welcome,
    menu_prompt: "Pilih menu di bawah:",
    catalog: "🛍️ Lihat Produk",
    my_balance: "💳 Saldo",
    my_orders: "📦 Pesanan",
    referral: "🎁 Referral",
    promo: "🎟️ Promo",
    support: "💬 Support",
    policy: "📜 Kebijakan",
    reseller: "🤝 Reseller & API",
    language: "🌐 Bahasa",
    follow_channel: "📢 Ikuti Channel Unicodes Digital",
    back_menu: "⬅️ Menu Utama",
    catalog_title: "🛍️ <b>Katalog Produk</b>\n\nPilih produk untuk lihat detail & beli. Harga sudah termasuk semua biaya.",
    catalog_pick_category: "🛍️ <b>Katalog Produk</b>\n\nPilih kategori dulu:",
    category_all: "Semua",
    product_detail: id_product_detail,
    coming_soon: "⏳ Segera hadir — produk ini belum bisa dibeli sekarang. Cek lagi nanti ya!",
    out_of_stock: id_out_of_stock,
    code_used_up: id_code_used_up,
    stock_label: id_stock_label,
    qty_prompt: "Pilih jumlah yang mau dibeli:",
    qty_custom_prompt: "Ketik jumlah yang mau dibeli (angka 1–30), atau /cancel untuk batal.",
    qty_invalid: "Jumlah harus angka 1–30. Coba lagi, atau /cancel untuk batal.",
    target_prompt: id_target_prompt,
    code_prompt: "⚡ Masukkan Kode Invite Muse\n\n1️⃣ Buka aplikasi Muse → menu Invite Friends / Referrals, salin teks bagikan atau kode invite kamu.\n2️⃣ Kirim ke chat ini — kode aja boleh (cth: UVI0QD), atau link/teks lengkapnya (bot otomatis baca kodenya).\n3️⃣ Tunggu sebentar — token masuk ke akun Muse kamu.\n\n💡 Satu kode cuma bisa dipakai 30 kali (1B sekali tebus, maks 30B per akun).\n\nContoh:\nUVI0QD",
    code_not_found: "❌ Format kode salah — kode tidak ketemu. Kirim kode invite kamu (cth: UVI0QD) atau link invite lengkapnya.",
    qty_title: id_qty_title,
    promo_ask: "Punya kode promo? Ketik kodenya di chat ini, atau tap “Lewati” untuk lanjut tanpa promo.",
    promo_enter_prompt: "Ketik kode promo kamu (satu pesan), atau /cancel untuk batal.",
    promo_applied: id_promo_applied,
    promo_invalid: id_promo_invalid,
    promo_skipped: "Lanjut tanpa promo.",
    pay_title: id_pay_title,
    pay_wallet: "💳 Bayar dari Saldo",
    pay_qris: "💳 Bayar via QRIS",
    pay_wallet_short: "💳 Saldo",
    wallet_insufficient: "❌ Saldo kamu tidak cukup untuk order ini. Isi saldo dulu atau bayar via QRIS.",
    wallet_paid: id_wallet_paid,
    wallet_refunded: "Saldo sudah dikembalikan ke dompet kamu.",
    qris_link: id_qris_link,
    qris_failed: id_qris_failed,
    qris_check_button: "✅ Saya Sudah Bayar",
    qris_check_found_order: id_qris_check_found_order,
    qris_check_found_topup: id_qris_check_found_topup,
    qris_check_pending: "⏳ Pembayaran belum masuk. Kalau kamu baru saja bayar, tunggu beberapa detik lalu tap tombolnya lagi.",
    qris_check_expired: "❌ QR sudah kedaluwarsa (5 menit). Buat order baru dari katalog.",
    qris_check_no_session: "⚠️ Sistem belum bisa mengecek pembayaran otomatis — sesi GoBiz belum login. Hubungi admin.",
    qris_check_failed: "❌ Gagal mengecek pembayaran. Coba lagi sebentar.",
    qris_check_too_soon: "⏳ Baru saja dicek — tunggu beberapa detik lagi.",
    join_channel_text: "🔒 <b>Gabung komunitas dulu ya!</b>\n\nSebelum belanja, join dulu channel komunitas Unicodes Digital — di sana ada info order terbaru, promo, dan pengumuman.\n\nTap tombol Gabung di bawah, lalu tap '✅ Saya Sudah Join'.",
    join_channel_button: "📢 Gabung Komunitas",
    join_channel_check_button: "✅ Saya Sudah Join",
    join_channel_not_yet: "⏳ Belum terdeteksi — pastikan kamu sudah join channel-nya, lalu tap tombol ini lagi.",
    pay_midtrans: "⚡ QRIS",
    midtrans_open_button: "💳 Buka Pembayaran",
    midtrans_link: id_midtrans_link,
    midtrans_topup_created: id_midtrans_topup_created,
    midtrans_failed: id_midtrans_failed,
    midtrans_check_no_config: "⚠️ Pembayaran Midtrans belum dikonfigurasi — hubungi admin.",
    free_order_done: id_free_order_done,
    wallet_title: id_wallet_title,
    topup_title: "➕ <b>Isi Saldo</b>\n\nPilih nominal, atau ketik nominal sendiri (minimal Rp10.000).",
    topup_custom_prompt: "Ketik nominal isi saldo (angka, minimal 10000), atau /cancel untuk batal.",
    topup_invalid: "Nominal harus angka minimal 10000. Coba lagi, atau /cancel untuk batal.",
    topup_created: id_topup_created,
    topup_failed: "❌ QR pembayaran belum bisa dibuat. Coba lagi nanti.",
    txns_title: "📜 <b>Riwayat Dompet</b>\n\n10 transaksi terakhir:",
    txns_empty: "Belum ada transaksi dompet.",
    txn_line: id_txn_line,
    orders_title: "📦 <b>Pesanan Saya</b>\n\nTap order untuk lihat detail:",
    orders_empty: "Kamu belum punya order. Yuk cek katalog! 🛍️",
    order_detail: id_order_detail,
    status_label: id_status_label,
    referral_title: id_referral_title,
    top_referrers_title: "🏆 <b>Top Referral</b>",
    top_referrers_empty: "Belum ada yang mengundang teman. Jadilah yang pertama! 🎁",
    top_referrer_line: id_top_referrer_line,
    promo_title: "🎟️ <b>Promo</b>\n\nPunya kode promo? Masukkan kodenya saat checkout — diskon langsung memotong total bayar. Satu kode hanya bisa dipakai sekali per pengguna.",
    support_title: "💬 <b>Support</b>\n\nAda kendala order / pembayaran? Buat tiket support — admin akan membalas langsung di chat ini.\n\nTap tombol di bawah untuk kirim pesan pertama kamu.",
    support_ask: "✉️ Jelaskan kendala kamu dalam satu pesan — sebutkan juga nomor order kalau ada (cth: UD-XXXXXX). Admin akan membalas lewat tiket ini.\n\nAtau /cancel untuk batal.",
    ticket_created: id_ticket_created,
    ticket_appended: id_ticket_appended,
    my_tickets_title: "🎫 <b>Tiket Support Saya</b>\n\n",
    my_tickets_empty: "Kamu belum punya tiket support. Tap tombol di bawah untuk buat tiket baru.",
    my_ticket_line: id_my_ticket_line,
    support_pick_order_title: "📦 <b>Pilih Order</b>\n\nPilih order yang mau kamu laporkan. Kalau kendalamu bukan soal order tertentu, pilih \"Tanpa order\".",
    support_pick_no_order: "Tanpa order / Lainnya",
    support_order_processing: "Diproses",
    support_order_row: id_support_order_row,
    support_ask_with_order: id_support_ask_with_order,
    policy_title: "📜 <b>Kebijakan Toko</b>\n\n🚀 <b>Pengiriman</b>\nProduk digital dikirim instan setelah pembayaran lunas. Proses ke supplier maksimal 10 menit — kalau lebih dari itu, buka tiket support lewat tombol 💬 Support.\n\n↩️ <b>Refund</b>\nKalau fulfillment gagal setelah kamu bayar: bayar saldo kembali ke dompet otomatis; bayar QRIS ditransfer balik manual oleh admin (maks. 1 hari kerja) — kalau belum masuk, buka tiket support lewat tombol 💬 Support.\n\n🛡️ <b>Garansi</b>\nAkun bergaransi penggantian 1 hari sejak diterima, selama dipakai sesuai aturan produk. Token yang sudah dipakai tidak bisa digaransi.\n\n💳 <b>Pembayaran</b>\nQRIS dinamis (GoPay, OVO, DANA, m-banking apa pun) atau saldo dompet.\n\n📩 <b>Bantuan</b>\nButuh bantuan? Buka tiket support lewat tombol 💬 Support di menu utama — admin akan membalas langsung di chat ini.",
    reseller_title: "🤝 <b>Reseller & API</b>\n\nProgram reseller & API akses masih disiapkan — segera hadir. Pantau terus info terbaru di toko ini.",
    lang_title: "🌐 <b>Bahasa / Language</b>\n\nPilih bahasa bot:",
    lang_set: "✅ Bahasa diubah ke Bahasa Indonesia.",
    order_not_found: "Order tidak ditemukan.",
    order_usage: "Pakai: /order <id> — contoh: /order UD-XXXX",
    fulfill_usage: "Pakai: /fulfill <id>",
    fulfill_processing: id_fulfill_processing,
    admin_only: "Perintah ini khusus admin.",
    promo_add_usage: "Pakai: /promo_add <KODE> <percent|fixed> <nilai> [max_uses] [days_valid]\nContoh: /promo_add HEMAT10 percent 10 5 30",
    promo_added: id_promo_added,
    promo_list_title: "🎟️ <b>Daftar Promo</b>",
    promo_list_empty: "Belum ada kode promo.",
    unknown_command: "Perintah tidak dikenal. Ketik /menu untuk lihat menu.",
    cancelled: "Dibatalkan. Ketik /menu untuk kembali ke menu.",
};

pub static STR_EN: Strings = Strings {
    welcome: en_welcome,
    menu_prompt: "Pick a menu below:",
    catalog: "🛍️ View Products",
    my_balance: "💳 Balance",
    my_orders: "📦 Orders",
    referral: "🎁 Referral",
    promo: "🎟️ Promo",
    support: "💬 Support",
    policy: "📜 Store Policy",
    reseller: "🤝 Reseller & API",
    language: "🌐 Language",
    follow_channel: "📢 Follow the Unicodes Digital Channel",
    back_menu: "⬅️ Main Menu",
    catalog_title: "🛍️ <b>Product Catalog</b>\n\nPick a product to see details & buy. Prices are all-in.",
    catalog_pick_category: "🛍️ <b>Product Catalog</b>\n\nPick a category first:",
    category_all: "All",
    product_detail: en_product_detail,
    coming_soon: "⏳ Coming soon — this product can't be bought yet. Check back later!",
    out_of_stock: en_out_of_stock,
    code_used_up: en_code_used_up,
    stock_label: en_stock_label,
    qty_prompt: "Choose how many you want to buy:",
    qty_custom_prompt: "Type the quantity (a number 1–30), or /cancel to abort.",
    qty_invalid: "Quantity must be a number 1–30. Try again, or /cancel to abort.",
    target_prompt: en_target_prompt,
    code_prompt: "⚡ Enter your Muse Invite Code\n\n1️⃣ Open the Muse app → Invite Friends / Referrals menu, copy the share text or your invite code.\n2️⃣ Send it here — just the code works (e.g. UVI0QD), or the full link/text (the bot auto-detects the code).\n3️⃣ Give it a moment — tokens land in your Muse account.\n\n💡 One code can be redeemed 30 times (1B per redemption, 30B ceiling per account).\n\nExample:\nUVI0QD",
    code_not_found: "❌ Wrong code format — no code found. Send your invite code (e.g. UVI0QD) or the full invite link.",
    qty_title: en_qty_title,
    promo_ask: "Have a promo code? Type it here, or tap “Skip” to continue without one.",
    promo_enter_prompt: "Type your promo code (one message), or /cancel to abort.",
    promo_applied: en_promo_applied,
    promo_invalid: en_promo_invalid,
    promo_skipped: "Continuing without a promo.",
    pay_title: en_pay_title,
    pay_wallet: "💳 Pay from Balance",
    pay_qris: "💳 Pay via QRIS",
    pay_wallet_short: "💳 Balance",
    wallet_insufficient: "❌ Your balance isn't enough for this order. Top up first or pay via QRIS.",
    wallet_paid: en_wallet_paid,
    wallet_refunded: "Your balance has been refunded to your wallet.",
    qris_link: en_qris_link,
    qris_failed: en_qris_failed,
    qris_check_button: "✅ I've Paid",
    qris_check_found_order: en_qris_check_found_order,
    qris_check_found_topup: en_qris_check_found_topup,
    qris_check_pending: "⏳ Payment hasn't arrived yet. If you just paid, wait a few seconds and tap the button again.",
    qris_check_expired: "❌ This QR has expired (5 minutes). Create a new order from the catalog.",
    qris_check_no_session: "⚠️ The system can't check payments automatically yet — the GoBiz session isn't logged in. Contact the admin.",
    qris_check_failed: "❌ Couldn't check the payment. Try again in a moment.",
    qris_check_too_soon: "⏳ Just checked — wait a few more seconds.",
    join_channel_text: "🔒 <b>Join the community first!</b>\n\nBefore shopping, please join the Unicodes Digital community channel — the latest orders, promos, and announcements live there.\n\nTap Join below, then tap '✅ I've Joined'.",
    join_channel_button: "📢 Join Community",
    join_channel_check_button: "✅ I've Joined",
    join_channel_not_yet: "⏳ Not detected yet — make sure you've joined the channel, then tap this button again.",
    pay_midtrans: "⚡ QRIS",
    midtrans_open_button: "💳 Open Payment",
    midtrans_link: en_midtrans_link,
    midtrans_topup_created: en_midtrans_topup_created,
    midtrans_failed: en_midtrans_failed,
    midtrans_check_no_config: "⚠️ Midtrans payments aren't configured yet — contact the admin.",
    free_order_done: en_free_order_done,
    wallet_title: en_wallet_title,
    topup_title: "➕ <b>Top Up</b>\n\nPick an amount, or type your own (minimum Rp10.000).",
    topup_custom_prompt: "Type the top-up amount (a number, minimum 10000), or /cancel to abort.",
    topup_invalid: "Amount must be a number of at least 10000. Try again, or /cancel to abort.",
    topup_created: en_topup_created,
    topup_failed: "❌ Payment QR couldn't be created. Try again later.",
    txns_title: "📜 <b>Wallet History</b>\n\nLast 10 transactions:",
    txns_empty: "No wallet transactions yet.",
    txn_line: en_txn_line,
    orders_title: "📦 <b>My Orders</b>\n\nTap an order to see details:",
    orders_empty: "You don't have any orders yet. Check the catalog! 🛍️",
    order_detail: en_order_detail,
    status_label: en_status_label,
    referral_title: en_referral_title,
    top_referrers_title: "🏆 <b>Top Referrals</b>",
    top_referrers_empty: "Nobody has invited friends yet. Be the first! 🎁",
    top_referrer_line: en_top_referrer_line,
    promo_title: "🎟️ <b>Promo</b>\n\nHave a promo code? Enter it at checkout — the discount cuts your total right away. Each code can be used once per user.",
    support_title: "💬 <b>Support</b>\n\nOrder / payment issues? Open a support ticket — the admin will reply right here in this chat.\n\nTap the button below to send your first message.",
    support_ask: "✉️ Describe your issue in one message — include your order number if you have one (e.g. UD-XXXXXX). The admin will reply via this ticket.\n\nOr /cancel to abort.",
    ticket_created: en_ticket_created,
    ticket_appended: en_ticket_appended,
    my_tickets_title: "🎫 <b>My Support Tickets</b>\n\n",
    my_tickets_empty: "You don't have any support tickets yet. Tap the button below to open a new one.",
    my_ticket_line: en_my_ticket_line,
    support_pick_order_title: "📦 <b>Pick an Order</b>\n\nPick the order you want to report. If your issue isn't about a specific order, pick \"No order\".",
    support_pick_no_order: "No order / Other",
    support_order_processing: "Processing",
    support_order_row: en_support_order_row,
    support_ask_with_order: en_support_ask_with_order,
    policy_title: "📜 <b>Store Policy</b>\n\n🚀 <b>Delivery</b>\nDigital products are delivered instantly after payment. Supplier processing takes at most 10 minutes — if it takes longer, open a support ticket via the 💬 Support button.\n\n↩️ <b>Refund</b>\nIf fulfillment fails after you paid: balance payments go back to your wallet automatically; QRIS payments are refunded manually by the admin (max 1 business day) — if it hasn't arrived, open a support ticket via the 💬 Support button.\n\n🛡️ <b>Warranty</b>\nAccounts come with a 1-day replacement warranty from delivery, when used per the product rules. Tokens already used can't be warrantied.\n\n💳 <b>Payment</b>\nDynamic QRIS (GoPay, OVO, DANA, any mobile banking) or wallet balance.\n\n📩 <b>Help</b>\nNeed help? Open a support ticket via the 💬 Support button in the main menu — the admin will reply right here in this chat.",
    reseller_title: "🤝 <b>Reseller & API</b>\n\nThe reseller & API program is still being prepared — coming soon. Stay tuned for updates here in the store.",
    lang_title: "🌐 <b>Language</b>\n\nChoose the bot language:",
    lang_set: "✅ Language changed to English.",
    order_not_found: "Order not found.",
    order_usage: "Usage: /order <id> — e.g. /order UD-XXXX",
    fulfill_usage: "Usage: /fulfill <id>",
    fulfill_processing: en_fulfill_processing,
    admin_only: "This command is admin-only.",
    promo_add_usage: "Usage: /promo_add <CODE> <percent|fixed> <value> [max_uses] [days_valid]\nExample: /promo_add SAVE10 percent 10 5 30",
    promo_added: en_promo_added,
    promo_list_title: "🎟️ <b>Promo List</b>",
    promo_list_empty: "No promo codes yet.",
    unknown_command: "Unknown command. Type /menu to see the menu.",
    cancelled: "Cancelled. Type /menu to go back to the menu.",
};

/// Ambil string sesuai bahasa (default ID kalau bahasa tidak dikenal).
pub fn t(lang: Lang) -> &'static Strings {
    match lang {
        Lang::Id => &STR_ID,
        Lang::En => &STR_EN,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn welcome_params() -> WelcomeParams {
        WelcomeParams {
            name: "Budi".to_string(),
            tg_id: 111,
            username: Some("budi".to_string()),
            total_spent_idr: 0,
            balance_idr: 0,
            total_users: 1,
            delivered_orders: 0,
            channel_url: String::new(),
            hour_wib: 9,
        }
    }

    #[test]
    fn rupiah_formats() {
        assert_eq!(rupiah(0), "Rp0");
        assert_eq!(rupiah(2000), "Rp2.000");
        assert_eq!(rupiah(10000), "Rp10.000");
        assert_eq!(rupiah(25000), "Rp25.000");
        assert_eq!(rupiah(1500000), "Rp1.500.000");
        assert_eq!(rupiah(-500), "Rp-500");
    }

    #[test]
    fn wib_time_converts_utc_to_wib() {
        // 02:00 UTC = 09:00 WIB.
        assert_eq!(wib_time("2026-11-21T02:00:00.000Z"), "09:00");
        assert_eq!(wib_time("2026-11-21T17:30:00.000Z"), "00:30");
        assert_eq!(wib_time("bukan-iso"), "--:--");
        assert_eq!(wib_time(""), "--:--");
    }

    #[test]
    fn welcome_id_matches_harness_s1() {
        // S1: header + sapaan waktu + INFORMASI AKUN + STATISTIK TOKO,
        // tanpa referral, tanpa baris channel (channel belum diset).
        let s = t(Lang::Id);
        let text = (s.welcome)(&welcome_params());
        assert!(text.contains("✦ UNICODES DIGITAL"));
        assert!(text.contains("Selamat pagi, Budi!"));
        assert!(text.contains("Selamat datang di toko digital kami."));
        assert!(text.contains("👤 INFORMASI AKUN"));
        assert!(text.contains("┌ ID Telegram : 111"));
        assert!(text.contains("├ Username    : @budi"));
        assert!(text.contains("├ Total Belanja : Rp0"));
        assert!(text.contains("└ Saldo Aktif : Rp0"));
        assert!(text.contains("📊 STATISTIK TOKO"));
        assert!(text.contains("├ 👥 Total Pengguna : 1"));
        assert!(text.contains("└ ✅ Pesanan Sukses : 0"));
        assert!(text.contains("Semua kebutuhan digitalmu"));
        assert!(text.contains("Silakan pilih menu di bawah untuk mulai"));
        assert!(!text.contains("channel resmi"));
        assert!(!text.to_lowercase().contains("referral"));
        // Sapaan berubah menurut jam WIB: 14 = siang, 16 = sore, 21 = malam.
        for (hour, greet) in [(14, "Selamat siang"), (16, "Selamat sore"), (21, "Selamat malam")] {
            let mut p = welcome_params();
            p.hour_wib = hour;
            assert!((s.welcome)(&p).contains(greet), "jam {hour} -> {greet}");
        }
    }

    #[test]
    fn welcome_id_with_channel_and_spent() {
        // S1 lanjutan + S33: channel diset -> baris promo channel muncul;
        // S7: setelah ada order, total belanja tampil.
        let s = t(Lang::Id);
        let mut p = welcome_params();
        p.channel_url = "https://t.me/unicodesdigital".to_string();
        p.total_spent_idr = 2000;
        p.balance_idr = 23000;
        p.total_users = 19;
        p.delivered_orders = 3;
        let text = (s.welcome)(&p);
        assert!(text.contains("channel resmi"));
        assert!(text.contains("├ Total Belanja : Rp2.000"));
        assert!(text.contains("└ Saldo Aktif : Rp23.000"));
        assert!(text.contains("├ 👥 Total Pengguna : 19"));
        assert!(text.contains("└ ✅ Pesanan Sukses : 3"));
    }

    #[test]
    fn welcome_en_matches_harness_s24() {
        // S24: menu EN — ACCOUNT INFO + STORE STATS, tanpa referral.
        let s = t(Lang::En);
        let text = (s.welcome)(&welcome_params());
        assert!(text.contains("Good morning, Budi!"));
        assert!(text.contains("ACCOUNT INFO"));
        assert!(text.contains("STORE STATS"));
        assert!(text.contains("┌ Telegram ID : 111"));
        assert!(!text.contains("Referral code"));
        assert!(!text.to_lowercase().contains("referral"));
    }

    #[test]
    fn product_detail_and_stock_labels() {
        let s = t(Lang::Id);
        let detail = (s.product_detail)(&ProductDetailParams {
            name: "Muse AI Token 1B".to_string(),
            desc: "Recharge 1 miliar token Muse ke akun kamu.".to_string(),
            price: 2000,
            unit: Some("per 1B token".to_string()),
            badge: Some("Terlaris".to_string()),
            features: vec!["Masuk ke akun kamu dalam hitungan menit".to_string()],
            stock: "Tersedia".to_string(),
            target_note: None,
        });
        assert!(detail.contains("Muse AI Token 1B"));
        assert!(detail.contains("Rp2.000"));
        assert!(detail.contains("/unit"));
        assert!(detail.contains("Detail produk"));
        assert_eq!((s.stock_label)(StockState::Available), "Tersedia");
        assert_eq!((s.stock_label)(StockState::OutOfStock), "Stok Habis");
        assert_eq!((s.stock_label)(StockState::ComingSoon), "Segera Hadir");
        // S19: template stok habis persis kata Dim.
        let oos = (s.out_of_stock)("Muse AI Token 1B");
        assert!(oos.contains("❌ Stok Habis"));
        assert!(oos.contains("Muse AI Token 1B"));
        assert!(oos.contains("Produk ini sedang habis dan belum bisa dipesan"));
    }

    #[test]
    fn code_used_up_and_qty_title() {
        // S4b: "sudah dipakai 30x".
        let s = t(Lang::Id);
        assert!((s.code_used_up)("CAP30X").contains("sudah dipakai 30x"));
        // S4: layar qty — kode + harga dasar + stok live.
        let qty = (s.qty_title)(&QtyTitleParams {
            code: "UVI0QD".to_string(),
            price: 2000,
            stock: "30B".to_string(),
        });
        assert!(qty.contains("UVI0QD"));
        assert!(qty.contains("Rp2.000"));
        assert!(qty.contains("Stok ready: 30B"));
    }

    #[test]
    fn pay_title_with_promo_totals() {
        // S4: ringkasan 5x2000 = Rp10.000; S9: qty 10 + promo 10% -> diskon
        // Rp2.000, total Rp18.000.
        let s = t(Lang::Id);
        let plain = (s.pay_title)(&PayTitleParams {
            name: "Muse AI Token 1B".to_string(),
            qty: 5,
            subtotal: 10000,
            discount: 0,
            total: 10000,
            balance: 25000,
            code: Some("UVI0QD".to_string()),
        });
        assert!(plain.contains("RINGKASAN PEMBAYARAN"));
        assert!(plain.contains("UVI0QD"));
        assert!(plain.contains("Rp10.000"));
        let promo = (s.pay_title)(&PayTitleParams {
            name: "Muse AI Token 1B".to_string(),
            qty: 10,
            subtotal: 20000,
            discount: 2000,
            total: 18000,
            balance: 23000,
            code: Some("UVI0QD".to_string()),
        });
        assert!(promo.contains("Rp2.000"));
        assert!(promo.contains("Rp18.000"));
    }

    #[test]
    fn promo_invalid_reasons_and_status_labels() {
        // S22: alasan promo — tidak ditemukan / kedaluwarsa / habis.
        let s = t(Lang::Id);
        assert!((s.promo_invalid)("not_found").contains("tidak ditemukan"));
        assert!((s.promo_invalid)("expired").contains("kedaluwarsa"));
        assert!((s.promo_invalid)("exhausted").contains("habis"));
        assert!((s.promo_invalid)("already_used").contains("sudah pernah kamu pakai"));
        assert_eq!((s.status_label)("delivered"), "🎉 Selesai / Terkirim");
        assert_eq!((s.status_label)("awaiting_payment"), "⏳ Menunggu pembayaran");
        assert_eq!((s.status_label)("paid"), "✅ Dibayar — antre proses");
        assert_eq!((s.status_label)("processing"), "⚙️ Sedang diproses");
        assert_eq!((s.status_label)("failed"), "❌ Gagal");
    }

    #[test]
    fn policy_and_support_texts_have_no_admin_handle() {
        // S10/S13: kebijakan tanpa @Dimas0227, arahkan ke tiket.
        let s = t(Lang::Id);
        assert!(!s.policy_title.contains("@Dimas0227"));
        assert!(s.policy_title.contains("tiket support"));
        assert!(s.policy_title.contains("Refund"));
        assert!(s.policy_title.contains("Garansi"));
        assert!(!s.support_title.contains("@Dimas0227"));
        assert!(s.support_title.contains("Support"));
        assert!(s.reseller_title.contains("segera hadir"));
    }
}
