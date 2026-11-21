//! Template HTML (maud) — port tampilan `src/app/**` versi TS.
//!
//! Halaman buyer = tema ungu Scaramouche (sama seperti TS):
//! bg `#14092b`, kartu `#221244`, aksen violet `#7c3aed`,
//! teks lavender `#ede9fe`, harga emas `#e8c874`.
//! Halaman ADMIN memakai palet NETRAL (keterbacaan dulu — revisi
//! 2026-11-21 setelah Dim protes warna ungu pekat susah dibaca):
//! bg `#101014`, kartu abu gelap `#1b1e26`, teks `#f4f4f5` / `#a1a1aa`,
//! ungu `#7c3aed` hanya untuk tombol utama, link, dan garis aksen tipis.
//! Halaman buyer = KATALOG SAJA (tanpa nama supplier/saldo/deposit).

use maud::{html, Markup, PreEscaped, DOCTYPE};
use serde_json::json;

use crate::catalog::{CatalogProduct, StockState};
use crate::i18n::rupiah;
use crate::lib::orders::StoreOrder;
use crate::lib::support_tickets::{SupportMessage, SupportTicket, TicketStatus};
use crate::web::util::{wib_date, wib_datetime};

/// CSS bersama (layout + komponen). Gaya per-kartu tetap inline,
/// seperti TS.
const BASE_CSS: &str = r#"
body { font-family: system-ui, sans-serif; margin: 0; background: #14092b; color: #ede9fe; min-height: 100vh; }
header { padding: 16px 24px; background: linear-gradient(90deg, #2a1650, #1a0f38); border-bottom: 2px solid #d9b45b; display: flex; justify-content: space-between; align-items: center; flex-wrap: wrap; gap: 4px 12px; }
header strong { color: #ede9fe; letter-spacing: 0.5px; }
header span { font-size: 13px; color: #d9b45b; }
main { max-width: 960px; margin: 0 auto; padding: 20px 16px 40px; }
a { color: #d9b45b; }
input, textarea { font-family: inherit; }
table { border-collapse: collapse; }
th, td { padding: 6px 8px; text-align: left; }
.product-card { transition: transform 0.15s ease, box-shadow 0.15s ease; }
.product-card:hover { transform: translateY(-4px); box-shadow: 0 12px 32px rgba(124, 58, 237, 0.35); }
.btn { background: linear-gradient(90deg, #7c3aed, #a855f7); color: #fff; border: none; border-radius: 10px; padding: 12px 20px; font-size: 15px; font-weight: 700; cursor: pointer; box-shadow: 0 4px 16px #7c3aed66; }
.btn:disabled { background: #4c4466; cursor: not-allowed; box-shadow: none; }
.btn-sm { background: #7c3aed; color: #fff; border: none; border-radius: 6px; padding: 6px 12px; cursor: pointer; }
.input { width: 100%; padding: 10px; box-sizing: border-box; background: #14092b; border: 1px solid #6d28d9; border-radius: 8px; color: #ede9fe; }
.err { color: #f87171; font-size: 13px; }
.ok { color: #4ade80; font-size: 13px; }
.muted { color: #b3a1e6; font-size: 13px; }
/* ---- Admin: palet netral, keterbacaan dulu (mobile-first) ----
   Berlaku kalau <body class="admin"> (lihat admin_layout). Ungu #7c3aed
   hanya untuk tombol utama, link, dan garis aksen tipis — bukan untuk
   background kartu atau blok besar. */
body.admin { background: #101014; color: #f4f4f5; font-size: 15px; line-height: 1.6; }
body.admin header { background: #1b1e26; border-bottom: 2px solid #7c3aed; }
body.admin header strong { color: #f4f4f5; }
body.admin header span { color: #a1a1aa; }
body.admin a { color: #a78bfa; }
body.admin .muted { color: #a1a1aa; font-size: 14px; }
body.admin .err { color: #fca5a5; font-size: 14px; }
body.admin .ok { color: #86efac; font-size: 14px; }
body.admin .btn:disabled { background: #27272a; color: #a1a1aa; box-shadow: none; }
body.admin .input { background: #101014; border: 1px solid #3f3f46; color: #f4f4f5; font-size: 15px; padding: 12px; }
.admin-head { margin: 4px 0 4px; font-size: 26px; }
.admin-sub { color: #a1a1aa; font-size: 15px; margin: 0 0 20px; line-height: 1.6; }
.admin-sec { margin: 28px 0 12px; font-size: 21px; }
.admin-grid { display: grid; grid-template-columns: repeat(auto-fit, minmax(140px, 1fr)); gap: 16px; margin: 12px 0 8px; }
.stat-card { background: #1b1e26; border: 1px solid #2e323d; border-top: 3px solid #7c3aed; border-radius: 14px; padding: 18px; }
.stat-card .num { font-size: 30px; font-weight: 800; color: #f4f4f5; line-height: 1.15; overflow-wrap: anywhere; }
.stat-card .lbl { font-size: 14px; color: #a1a1aa; margin-top: 4px; }
.stat-card .sub { font-size: 13px; color: #a1a1aa; margin-top: 6px; }
.admin-card { background: #1b1e26; border: 1px solid #2e323d; border-radius: 14px; padding: 18px; margin-bottom: 16px; }
.admin-card h3 { margin: 0 0 8px; font-size: 18px; }
.supplier-grid { display: grid; grid-template-columns: repeat(auto-fit, minmax(250px, 1fr)); gap: 16px; margin: 12px 0 8px; }
.supplier-card { background: #1b1e26; border: 1px solid #2e323d; border-radius: 14px; padding: 18px; }
.supplier-card .bal { font-size: 16px; font-weight: 700; color: #f4f4f5; overflow-wrap: anywhere; }
.supplier-card .meta { font-size: 14px; color: #a1a1aa; margin-top: 8px; line-height: 1.6; }
.badge { display: inline-block; font-size: 13px; font-weight: 700; border-radius: 999px; padding: 3px 10px; white-space: nowrap; }
.badge-ok { background: #166534; color: #dcfce7; }
.badge-err { background: #7f1d1d; color: #fecaca; }
.badge-warn { background: #78350f; color: #fde68a; }
.table-scroll { overflow-x: auto; -webkit-overflow-scrolling: touch; border: 1px solid #2e323d; border-radius: 12px; background: #1b1e26; margin-bottom: 16px; }
.table-scroll table { width: 100%; min-width: 560px; }
.table-scroll th { color: #a1a1aa; font-size: 13px; text-transform: uppercase; letter-spacing: .4px; }
.table-scroll td, .table-scroll th { padding: 12px 14px; border-top: 1px solid #2e323d; white-space: nowrap; }
.table-scroll thead th { border-top: none; }
.table-scroll tbody tr:first-child td { border-top: none; }
.admin-row { display: flex; flex-wrap: wrap; gap: 10px; align-items: center; margin: 12px 0; }
.admin-row .input { flex: 1 1 200px; }
.admin-nav { display: flex; flex-wrap: wrap; gap: 6px 8px; align-items: center; margin-bottom: 20px; padding-bottom: 12px; border-bottom: 1px solid #2e323d; }
.admin-nav a.nav-link { color: #d4d4d8; font-size: 15px; font-weight: 600; text-decoration: none; border: 1px solid #3f3f46; border-radius: 999px; padding: 7px 14px; }
.admin-nav a.nav-link.active { color: #fff; background: #7c3aed; border-color: #7c3aed; }
.admin-nav a.nav-logout { margin-left: auto; color: #a1a1aa; font-size: 14px; }
.login-card { max-width: 380px; margin: 48px auto 0; background: #1b1e26; border: 1px solid #2e323d; border-radius: 16px; padding: 24px; }
.msg-bubble { border: 1px solid #2e323d; border-radius: 12px; padding: 14px 16px; margin-bottom: 14px; }
.msg-bubble.admin { background: #1b1e26; }
.msg-bubble.user { background: #23262f; }
@media (max-width: 560px) {
  main { padding: 16px 12px 32px; }
  header { padding: 12px 16px; }
  .stat-card .num { font-size: 26px; }
  .admin-card, .supplier-card { padding: 16px; }
  .admin-grid { grid-template-columns: repeat(2, 1fr); gap: 12px; }
}
"#;

/// Kerangka halaman (port `src/app/layout.tsx`).
/// Kerangka halaman (port `src/app/layout.tsx`).
/// `body_class`: None = halaman buyer (tema ungu), Some("admin") =
/// halaman admin (palet netral, lihat `body.admin` di BASE_CSS).
fn skeleton(title: &str, body_class: Option<&str>, content: Markup) -> Markup {
    html! {
        (DOCTYPE)
        html lang="id" {
            head {
                meta charset="utf-8";
                meta name="viewport" content="width=device-width, initial-scale=1";
                title { (title) " — Unicodes Digital" }
                style { (PreEscaped(BASE_CSS)) }
            }
            body class=(body_class.unwrap_or("")) {
                header {
                    strong { "◆ Unicodes Digital" }
                    span { "Reseller AI & Digital" }
                }
                main { (content) }
            }
        }
    }
}

pub fn layout(title: &str, content: Markup) -> Markup {
    skeleton(title, None, content)
}

/// Kerangka halaman ADMIN — sama seperti `layout`, tapi `<body
/// class="admin">` sehingga palet netral + tipografi admin berlaku.
/// Dipakai semua halaman /admin*; halaman buyer tetap memakai `layout`.
pub fn admin_layout(title: &str, content: Markup) -> Markup {
    skeleton(title, Some("admin"), content)
}

/// JSON satu produk — bentuk yang sama dengan `GET /api/products` TS.
pub fn product_json(p: &CatalogProduct) -> serde_json::Value {
    json!({
        "id": p.id,
        "name": p.name,
        "description": p.description,
        "supplier": p.supplier,
        "supplierProductCode": p.supplier_product_code,
        "fulfillment": p.fulfillment.as_str(),
        "category": p.category,
        "stockState": p.stock_state.as_str(),
        "retailPriceIdr": p.retail_price_idr,
        "active": p.active,
        "requiresTarget": p.requires_target,
        "targetLabel": p.target_label,
        "comingSoon": p.coming_soon,
        "badge": p.badge,
        "unit": p.unit,
        "features": p.features,
        "accent": p.accent,
    })
}

/// Label + warna badge stok (sama seperti `STOCK_BADGE` di page.tsx).
fn stock_badge(state: StockState) -> (&'static str, &'static str, &'static str, &'static str) {
    match state {
        StockState::Available => ("Tersedia", "#059669", "#ffffff", "1px solid #059669"),
        StockState::OutOfStock => ("Stok Habis", "#dc2626", "#ffffff", "1px solid #dc2626"),
        StockState::ComingSoon => ("Segera Hadir", "#14092b", "#e8c874", "1px solid #d9b45b"),
    }
}

fn enc(s: &str) -> String {
    s.replace(' ', "%20")
}

const TOKEN_SVG: &str = r##"<svg width="64" height="64" viewBox="0 0 64 64" aria-hidden="true"><circle cx="32" cy="32" r="26" fill="#e8c874" stroke="#fff7d6" stroke-width="2"/><circle cx="32" cy="32" r="19" fill="none" stroke="#8a6d1f" stroke-width="1.5" stroke-dasharray="3 3"/><text x="32" y="41" text-anchor="middle" font-size="26" font-weight="700" fill="#3d2a00">M</text></svg>"##;

const SPARK_SVG: &str = r##"<svg width="64" height="64" viewBox="0 0 64 64" aria-hidden="true"><path d="M32 6 L38 26 L58 32 L38 38 L32 58 L26 38 L6 32 L26 26 Z" fill="#e8c874" stroke="#fff7d6" stroke-width="1.5"/><path d="M50 8 L52.5 16 L60 18.5 L52.5 21 L50 29 L47.5 21 L40 18.5 L47.5 16 Z" fill="#ede9fe"/></svg>"##;

/// Satu kartu produk di storefront (port `page.tsx` + `buy-button.tsx`).
fn product_card(p: &CatalogProduct) -> Markup {
    let accent = p.accent.unwrap_or("#7c3aed");
    let is_token = p.id.starts_with("muse-");
    let (label, bg, color, border) = stock_badge(p.stock_state);
    let disabled = p.stock_state != StockState::Available;
    let opacity = if disabled { "0.85" } else { "1" };
    html! {
        li class="product-card" style=(format!("border: 1px solid #7c3aed; border-radius: 16px; background: #221244; overflow: hidden; display: flex; flex-direction: column; opacity: {opacity};")) {
            div style=(format!("height: 110px; background: radial-gradient(circle at 30% 30%, {accent}cc, #14092b 75%); display: flex; align-items: center; justify-content: center; position: relative;")) {
                @if is_token {
                    (PreEscaped(TOKEN_SVG))
                } @else {
                    (PreEscaped(SPARK_SVG))
                }
                @if let Some(badge) = p.badge {
                    span style="position: absolute; top: 10px; right: 10px; background: #e8c874; color: #3d2a00; font-size: 12px; font-weight: 700; border-radius: 999px; padding: 3px 10px;" {
                        (badge)
                    }
                }
                span style=(format!("position: absolute; top: 10px; left: 10px; background: {bg}; color: {color}; font-size: 12px; font-weight: 700; border: {border}; border-radius: 999px; padding: 3px 10px;")) {
                    (label)
                }
            }
            div style="padding: 16px; display: flex; flex-direction: column; gap: 8px; flex: 1;" {
                strong style="color: #ede9fe; font-size: 18px;" { (p.name) }
                p style="margin: 0; color: #c9b8f5; font-size: 14px;" { (p.description) }
                @if !p.features.is_empty() {
                    ul style="list-style: none; padding: 0; margin: 4px 0; display: grid; gap: 6px;" {
                        @for f in p.features {
                            li style="color: #b3a1e6; font-size: 13px;" {
                                span style="color: #e8c874;" { "✓" } " " (*f)
                            }
                        }
                    }
                }
                p style="margin: 8px 0 0;" {
                    strong style="color: #e8c874; font-size: 24px;" { (rupiah(p.retail_price_idr)) }
                    @if let Some(unit) = p.unit {
                        " "
                        span style="color: #b3a1e6; font-size: 13px;" { (unit) }
                    }
                }
                div style="margin-top: auto; padding-top: 8px;" data-product-card="" {
                    @if p.requires_target && !disabled {
                        p style="margin: 8px 0;" {
                            input type="text" data-target-input="" placeholder=(p.target_label.unwrap_or("Target")) class="input";
                        }
                    }
                    @if disabled {
                        button type="button" class="btn" disabled style="width: 100%;" { (label) }
                    } @else {
                        button type="button" class="btn" style="width: 100%;" data-buy="" data-product-id=(p.id) data-requires-target=(if p.requires_target { "1" } else { "0" }) data-target-label=(p.target_label.unwrap_or("Target")) { "Beli Sekarang" }
                    }
                    p class="err" data-error="" {}
                    p style="font-size: 13px; color: #b3a1e6; margin: 6px 0 0; white-space: pre-line;" data-status="" {}
                    div data-payment="" style="display: none; text-align: center; margin-top: 8px;" {
                        a data-pay-link="" href="#" target="_blank" rel="noreferrer" style="display: block; padding: 12px; border-radius: 8px; background: linear-gradient(90deg, #7c3aed, #a855f7); color: #fff; text-decoration: none; font-weight: 600;" { "💳 Buka Pembayaran (QRIS)" }
                        p style="font-size: 14px; margin: 8px 0 0;" {
                            "Tap tombol di atas untuk bayar — link berlaku sampai "
                            span data-pay-expiry="" {}
                            " WIB (30 menit)."
                        }
                    }
                }
            }
        }
    }
}

/// JS tombol beli storefront — port `buy-button.tsx`:
/// buat order (qty 1) + tampilkan link bayar + pantau status tiap 10 detik.
const BUY_JS: &str = r##"
document.querySelectorAll('[data-buy]').forEach(function (btn) {
  btn.addEventListener('click', async function () {
    var card = btn.closest('[data-product-card]');
    var productId = btn.getAttribute('data-product-id');
    var requiresTarget = btn.getAttribute('data-requires-target') === '1';
    var targetLabel = btn.getAttribute('data-target-label') || 'Target';
    var targetInput = card.querySelector('[data-target-input]');
    var target = targetInput ? targetInput.value.trim() : '';
    var errEl = card.querySelector('[data-error]');
    var statusEl = card.querySelector('[data-status]');
    var payEl = card.querySelector('[data-payment]');
    if (requiresTarget && !target) { errEl.textContent = targetLabel + ' wajib diisi.'; return; }
    btn.disabled = true; btn.textContent = 'Memproses...'; errEl.textContent = '';
    try {
      var res = await fetch('/api/orders', { method: 'POST', headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ productId: productId, quantity: 1, buyerRef: 'web', target: requiresTarget ? target : undefined }) });
      var data = await res.json();
      if (!data.ok || !data.order) throw new Error(data.error || 'Gagal membuat order');
      if (!data.payment) { statusEl.textContent = 'Order gratis dibuat — sedang diproses.'; return; }
      payEl.style.display = 'block';
      payEl.querySelector('[data-pay-link]').href = data.payment.redirectUrl;
      payEl.querySelector('[data-pay-expiry]').textContent = new Date(data.payment.expiresAt).toLocaleTimeString('id-ID', { timeZone: 'Asia/Jakarta', hour: '2-digit', minute: '2-digit' });
      statusEl.textContent = 'Menunggu pembayaran… (dicek otomatis tiap 10 detik)';
      var orderId = data.order.id; var expiresAt = data.payment.expiresAt; var stopped = false;
      var tick = async function () {
        try {
          var r = await fetch('/api/orders/' + encodeURIComponent(orderId));
          var b = await r.json();
          if (stopped || !b.ok || !b.order) return;
          var st = b.order.status || '';
          if (st === 'delivered') { statusEl.textContent = '✅ Selesai!\n' + ((b.order.deliveredItems || []).join('\n') || b.order.resultLink || 'Order selesai.'); stopped = true; }
          else if (st === 'failed' || st === 'refunded') { statusEl.textContent = '❌ ' + (b.order.error || 'Order gagal.'); stopped = true; }
          else if (st === 'paid') statusEl.textContent = '✅ Lunas — sedang diproses ke supplier…';
          else if (st === 'processing') statusEl.textContent = '⏳ Sedang diproses ke supplier…';
        } catch (e) {}
      };
      await tick();
      var timer = setInterval(function () {
        if (Date.now() > new Date(expiresAt).getTime()) { statusEl.textContent = '❌ QR kedaluwarsa sebelum dibayar — buat order baru.'; stopped = true; clearInterval(timer); return; }
        tick();
      }, 10000);
    } catch (e) { errEl.textContent = (e && e.message) || String(e); btn.disabled = false; btn.textContent = 'Beli Sekarang'; }
  });
});
"##;

/// Storefront `/` — port `src/app/page.tsx`.
/// Halaman pembeli = katalog saja (tanpa supplier/saldo/deposit).
pub fn storefront(products: &[CatalogProduct], categories: &[&str], active_cat: &str) -> Markup {
    let title_style = "color: #ede9fe; font-size: 32px; margin: 8px 0 4px; background: linear-gradient(90deg, #ede9fe 30%, #e8c874 70%); -webkit-background-clip: text; background-clip: text; -webkit-text-fill-color: transparent;";
    html! {
        style { (PreEscaped(".product-card { transition: transform 0.15s ease, box-shadow 0.15s ease; } .product-card:hover { transform: translateY(-4px); box-shadow: 0 12px 32px rgba(124, 58, 237, 0.35); }")) }
        h1 style=(title_style) { "Produk Digital" }
        p style="color: #b3a1e6; margin-top: 0; margin-bottom: 16px;" {
            "Produk digital instan — bayar lewat QRIS/e-wallet/VA, order langsung diproses dan hasilnya dikirim ke kamu."
        }
        div style="display: flex; gap: 8; flex-wrap: wrap; margin-bottom: 20px;" {
            @for c in std::iter::once(&"Semua").chain(categories.iter()) {
                @let active = *c == active_cat;
                a href=(if *c == "Semua" { "/".to_string() } else { format!("/?cat={}", enc(c)) }) style=(if active { "color: #fff; font-size: 13px; font-weight: 700; background: #7c3aed; border: 1px solid #7c3aed; border-radius: 999px; padding: 6px 14px; text-decoration: none;" } else { "color: #b3a1e6; font-size: 13px; font-weight: 700; background: transparent; border: 1px solid #6d28d9; border-radius: 999px; padding: 6px 14px; text-decoration: none;" }) {
                    (*c)
                }
            }
        }
        ul style="list-style: none; padding: 0; display: grid; grid-template-columns: repeat(auto-fit, minmax(280px, 1fr)); gap: 20px;" {
            @for p in products {
                (product_card(p))
            }
        }
        div style="margin-top: 28px; display: flex; flex-wrap: wrap; gap: 12px; justify-content: center;" {
            @for t in ["Proses instan setelah bayar", "Pembayaran aman — QRIS / e-wallet / VA", "Gagal diproses? Uang kembali"] {
                span style="color: #b3a1e6; font-size: 13px; border: 1px solid #6d28d9; border-radius: 999px; padding: 6px 14px;" {
                    span style="color: #e8c874;" { "✓" } " " (t)
                }
            }
        }
        script { (PreEscaped(BUY_JS)) }
    }
}

/// JS Mini App — port `src/app/tg/page.tsx`:
/// init Telegram WebApp + tema, katalog dari JSON yang diserver-render,
/// checkout (qty pack 1/5/10/30 + custom max 30, target) + pantau status.
const TG_JS: &str = r##"
var FALLBACK_THEME = { bg: '#14092b', text: '#ede9fe', button: '#7c3aed', buttonText: '#ffffff', secondaryBg: '#221244', hint: '#b3a1e6' };
var theme = Object.assign({}, FALLBACK_THEME);
var tg = null;
var products = [];
try { products = JSON.parse(document.getElementById('catalog-data').textContent || '[]'); } catch (e) { products = []; }
var selectedId = null, qty = 1, customQty = '', target = '', paying = false;
var doneOrderId = null, payment = null, orderStatus = null, orderResult = null;
var pollTimer = null;

function rupiah(n) { return 'Rp' + Number(n).toLocaleString('id-ID'); }
function esc(s) { return String(s == null ? '' : s).replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;').replace(/"/g, '&quot;'); }
function showError(msg) { var el = document.getElementById('tg-error'); if (msg) { el.style.display = 'block'; el.textContent = msg; } else { el.style.display = 'none'; el.textContent = ''; } }
function showDone() {
  var el = document.getElementById('tg-done');
  if (doneOrderId) { el.style.display = 'block'; el.textContent = '✅ Order ' + doneOrderId + ' dibuat! Selesaikan pembayaran di tab yang terbuka — hasilnya akan dikirim bot ke chat Telegram kamu.'; }
  else { el.style.display = 'none'; }
}
function selected() { return products.find(function (p) { return p.id === selectedId; }) || null; }
function effectiveQty() { var s = selected(); return s && s.id === 'muse-token-1b' ? qty : 1; }

function renderList() {
  var wrap = document.getElementById('tg-list');
  if (selectedId) { wrap.style.display = 'none'; document.getElementById('tg-detail').style.display = 'block'; renderDetail(); return; }
  wrap.style.display = 'block'; document.getElementById('tg-detail').style.display = 'none';
  if (!products.length) { wrap.innerHTML = '<p>Katalog kosong.</p>'; return; }
  wrap.innerHTML = products.map(function (p) {
    var btn = p.stockState === 'available' ? 'Beli' : (p.stockState === 'out_of_stock' ? 'Stok Habis' : 'Segera hadir');
    return '<div style="background:' + theme.secondaryBg + ';border:1px solid ' + theme.button + ';border-radius:12px;padding:12px;margin-bottom:12px;">'
      + '<div style="font-weight:700;">' + (p.badge ? '🏷️ ' + esc(p.badge) + ' — ' : '') + esc(p.name) + '</div>'
      + '<div style="font-size:13px;color:' + theme.hint + ';margin:4px 0;">' + esc(p.description) + '</div>'
      + '<div style="font-weight:700;color:#e8c874;">' + rupiah(p.retailPriceIdr) + (p.unit ? ' ' + esc(p.unit) : '') + '</div>'
      + '<button data-pick="' + esc(p.id) + '" ' + (p.stockState !== 'available' ? 'disabled' : '') + ' style="background:' + theme.button + ';color:' + theme.buttonText + ';border:none;border-radius:10px;padding:10px 14px;font-weight:700;cursor:pointer;width:100%;margin-top:10px;">' + btn + '</button></div>';
  }).join('');
  wrap.querySelectorAll('[data-pick]').forEach(function (b) {
    b.addEventListener('click', function () { selectedId = b.getAttribute('data-pick'); qty = 1; customQty = ''; target = ''; showError(null); doneOrderId = null; payment = null; orderStatus = null; orderResult = null; renderList(); });
  });
}

function renderDetail() {
  var s = selected(); if (!s) return;
  var el = document.getElementById('tg-detail');
  var total = s.retailPriceIdr * effectiveQty();
  var html = '<div style="background:' + theme.secondaryBg + ';border:1px solid ' + theme.button + ';border-radius:12px;padding:12px;">'
    + '<div style="font-weight:700;">' + esc(s.name) + '</div>'
    + '<div style="font-size:13px;color:' + theme.hint + ';margin:4px 0;">' + esc(s.description) + '</div>'
    + (s.features || []).map(function (f) { return '<div style="font-size:13px;">✅ ' + esc(f) + '</div>'; }).join('')
    + '<div style="font-weight:700;color:#e8c874;margin:8px 0;">' + rupiah(s.retailPriceIdr) + (s.unit ? ' ' + esc(s.unit) : '') + '</div>';
  if (s.id === 'muse-token-1b') {
    html += '<div style="margin:8px 0;"><div style="font-size:13px;margin-bottom:6px;">Jumlah:</div><div style="display:flex;gap:8px;flex-wrap:wrap;">'
      + [1, 5, 10, 30].map(function (n) {
          var active = qty === n && !customQty;
          return '<button data-qty="' + n + '" style="background:' + (active ? theme.button : theme.secondaryBg) + ';color:' + theme.buttonText + ';border:1px solid ' + theme.button + ';border-radius:10px;padding:10px 14px;font-weight:700;cursor:pointer;">' + n + 'B</button>';
        }).join('')
      + '<input id="tg-custom-qty" type="number" min="1" max="30" placeholder="Lainnya" value="' + esc(customQty) + '" style="width:90px;border-radius:10px;border:1px solid ' + theme.button + ';background:' + theme.bg + ';color:' + theme.text + ';padding:10px;"></div></div>';
  }
  if (s.requiresTarget) {
    html += '<div style="margin:8px 0;"><div style="font-size:13px;margin-bottom:6px;">' + esc(s.targetLabel || 'Target') + '</div>'
      + '<input id="tg-target" type="text" placeholder="https://muse.ai/invite/..." value="' + esc(target) + '" style="width:100%;box-sizing:border-box;border-radius:10px;border:1px solid ' + theme.button + ';background:' + theme.bg + ';color:' + theme.text + ';padding:10px;"></div>';
  }
  html += '<div style="font-weight:700;margin:10px 0;">Total: ' + rupiah(total) + '</div>';
  if (payment) {
    html += '<div style="text-align:center;"><a href="' + esc(payment.redirectUrl) + '" target="_blank" rel="noreferrer" style="display:block;background:' + theme.button + ';color:' + theme.buttonText + ';border-radius:10px;padding:10px 14px;font-weight:700;text-decoration:none;">💳 Buka Pembayaran (QRIS)</a>'
      + '<div style="font-size:14px;margin-top:8px;">Tap tombol di atas untuk bayar — link berlaku sampai ' + new Date(payment.expiresAt).toLocaleTimeString('id-ID', { timeZone: 'Asia/Jakarta', hour: '2-digit', minute: '2-digit' }) + ' WIB (30 menit).</div>'
      + '<div id="tg-pay-status" style="font-size:13px;color:' + theme.hint + ';margin-top:6px;"></div></div>';
  } else if (doneOrderId) {
    html += '<div id="tg-pay-status" style="font-size:14px;"></div>';
  } else {
    html += '<button id="tg-checkout" style="background:' + theme.button + ';color:' + theme.buttonText + ';border:none;border-radius:10px;padding:10px 14px;font-weight:700;cursor:pointer;width:100%;" ' + (paying ? 'disabled' : '') + '>' + (paying ? 'Memproses…' : 'Bayar ' + rupiah(total)) + '</button>';
  }
  html += '<button id="tg-back" style="background:transparent;color:' + theme.buttonText + ';border:1px solid ' + theme.button + ';border-radius:10px;padding:10px 14px;font-weight:700;cursor:pointer;width:100%;margin-top:8px;">⬅️ Kembali ke katalog</button></div>';
  el.innerHTML = html;
  el.querySelectorAll('[data-qty]').forEach(function (b) { b.addEventListener('click', function () { qty = Number(b.getAttribute('data-qty')); customQty = ''; renderDetail(); }); });
  var cq = document.getElementById('tg-custom-qty');
  if (cq) cq.addEventListener('input', function () { customQty = cq.value; var n = Number(cq.value); if (Number.isFinite(n) && n >= 1 && n <= 30) qty = Math.floor(n); renderDetail(); var cq2 = document.getElementById('tg-custom-qty'); if (cq2) { cq2.focus(); cq2.setSelectionRange(cq2.value.length, cq2.value.length); } });
  var ti = document.getElementById('tg-target');
  if (ti) ti.addEventListener('input', function () { target = ti.value; });
  var co = document.getElementById('tg-checkout');
  if (co) co.addEventListener('click', checkout);
  document.getElementById('tg-back').addEventListener('click', function () { selectedId = null; showError(null); doneOrderId = null; payment = null; orderStatus = null; orderResult = null; if (pollTimer) { clearInterval(pollTimer); pollTimer = null; } renderList(); });
  updatePayStatus();
}

function updatePayStatus() {
  var el = document.getElementById('tg-pay-status'); if (!el) return;
  if (orderStatus === 'awaiting_payment') el.textContent = 'Menunggu pembayaran… (dicek otomatis tiap 10 detik)';
  else if (orderStatus === 'paid') el.textContent = '✅ Lunas — sedang diproses ke supplier…';
  else if (orderStatus === 'processing') el.textContent = '⏳ Sedang diproses ke supplier…';
  else if (orderStatus === 'delivered') el.textContent = '✅ Selesai!\n' + (orderResult || '');
  else if (orderStatus === 'failed' || orderStatus === 'refunded') el.textContent = '❌ ' + (orderResult || 'Order gagal.');
  else if (doneOrderId) el.textContent = '✅ Order dibuat — sedang diproses…';
}

async function checkout() {
  var s = selected(); if (!s) return;
  if (!tg || !tg.initData) { showError('Checkout hanya bisa dari dalam Telegram — buka toko ini lewat tombol 🌐 Buka Toko di bot @unicodesdigital_bot.'); return; }
  var tgt = (document.getElementById('tg-target') ? document.getElementById('tg-target').value.trim() : target);
  if (s.requiresTarget && !tgt) { showError('Isi target dulu (kode invite / link referral Muse).'); return; }
  showError(null); paying = true; renderDetail();
  try {
    var res = await fetch('/api/orders', { method: 'POST', headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ productId: s.id, quantity: effectiveQty(), target: s.requiresTarget ? tgt : undefined, initData: tg.initData }) });
    var body = await res.json().catch(function () { return {}; });
    if (!res.ok || body.ok !== true) throw new Error(typeof body.error === 'string' ? body.error : 'Checkout gagal.');
    doneOrderId = body.order.id;
    showDone();
    if (!body.payment) { orderStatus = 'paid'; renderDetail(); startPoll(); return; }
    payment = { redirectUrl: body.payment.redirectUrl, snapToken: body.payment.snapToken, expiresAt: body.payment.expiresAt, orderId: body.order.id };
    orderStatus = 'awaiting_payment'; renderDetail(); startPoll();
  } catch (e) { showError((e && e.message) || 'Checkout gagal.'); }
  finally { paying = false; renderDetail(); }
}

function startPoll() {
  if (pollTimer) clearInterval(pollTimer);
  if (!doneOrderId) return;
  var stopped = false;
  var tick = async function () {
    try {
      var res = await fetch('/api/orders/' + encodeURIComponent(doneOrderId));
      var body = await res.json().catch(function () { return {}; });
      if (stopped || !body.ok || !body.order) return;
      var st = body.order.status || '';
      orderStatus = st;
      if (st === 'delivered') { orderResult = (body.order.deliveredItems || []).join('\n') || body.order.resultLink || 'Order selesai.'; stopped = true; }
      else if (st === 'failed' || st === 'refunded') { orderResult = body.order.error || 'Order gagal.'; stopped = true; }
      updatePayStatus();
      if (stopped) { clearInterval(pollTimer); pollTimer = null; }
    } catch (e) {}
  };
  tick();
  pollTimer = setInterval(function () {
    if (payment && Date.now() > new Date(payment.expiresAt).getTime()) { orderStatus = 'failed'; orderResult = 'QR kedaluwarsa sebelum dibayar — buat order baru.'; stopped = true; clearInterval(pollTimer); pollTimer = null; updatePayStatus(); return; }
    tick();
  }, 10000);
}

(function initTg() {
  var inited = false;
  var init = function () {
    if (inited) return;
    var wa = window.Telegram && window.Telegram.WebApp;
    if (!wa) return;
    inited = true; tg = wa;
    var tp = wa.themeParams || {};
    theme = { bg: tp.bg_color || FALLBACK_THEME.bg, text: tp.text_color || FALLBACK_THEME.text, button: tp.button_color || FALLBACK_THEME.button, buttonText: tp.button_text_color || FALLBACK_THEME.buttonText, secondaryBg: tp.secondary_bg_color || FALLBACK_THEME.secondaryBg, hint: tp.hint_color || FALLBACK_THEME.hint };
    document.body.style.background = theme.bg; document.body.style.color = theme.text;
    var u = (wa.initDataUnsafe && wa.initDataUnsafe.user) || null;
    document.getElementById('tg-greet').textContent = (u ? 'Hai, ' + (u.first_name || 'teman') + '! ' : '') + 'Toko AI & produk digital — harga jujur, proses instan.';
    document.getElementById('tg-outside').style.display = 'none';
    try { wa.ready(); wa.expand(); if (wa.setBackgroundColor) wa.setBackgroundColor(theme.bg); if (wa.setHeaderColor) wa.setHeaderColor(theme.bg); } catch (e) {}
    renderList();
  };
  init();
  var timer = setInterval(init, 500);
  setTimeout(function () { clearInterval(timer); }, 5000);
  renderList();
})();
"##;

/// Mini App `/tg` — port `src/app/tg/page.tsx`.
/// Katalog diserver-render sebagai JSON; checkout tetap lewat JS +
/// `Telegram.WebApp.initData` (validasi HMAC di server).
pub fn tg_page(products: &[CatalogProduct]) -> Markup {
    let products_json: Vec<serde_json::Value> =
        products.iter().map(product_json).collect();
    let mut json_str = serde_json::to_string(&products_json).unwrap_or_else(|_| "[]".to_string());
    // Jangan biarkan `</script>` lolos dari dalam JSON.
    json_str = json_str.replace('<', "\\u003c");
    html! {
        (DOCTYPE)
        html lang="id" {
            head {
                meta charset="utf-8";
                meta name="viewport" content="width=device-width, initial-scale=1";
                title { "Unicodes Digital — Toko Telegram" }
                style { (PreEscaped("body { font-family: system-ui, sans-serif; margin: 0; background: #14092b; color: #ede9fe; min-height: 100vh; padding: 16px; box-sizing: border-box; }")) }
                script src="https://telegram.org/js/telegram-web-app.js" async {}
            }
            body {
                h1 style="font-size: 20px; margin: 4px 0 2px;" { "💜 Unicodes Digital" }
                p id="tg-greet" style="color: #b3a1e6; margin: 0 0 16px; font-size: 13px;" {
                    "Toko AI & produk digital — harga jujur, proses instan."
                }
                p id="tg-outside" style="background: #221244; border-radius: 10px; padding: 10px; font-size: 13px;" {
                    "Kamu membuka toko ini di luar Telegram. Katalog tetap bisa dilihat — untuk beli, buka lewat tombol 🌐 Buka Toko di bot @unicodesdigital_bot."
                }
                p id="tg-error" style="display: none; background: #7f1d1d; border-radius: 10px; padding: 10px; font-size: 13px;" {}
                p id="tg-done" style="display: none; background: #14532d; border-radius: 10px; padding: 10px; font-size: 13px;" {}
                div id="tg-list" {}
                div id="tg-detail" style="display: none;" {}
                script id="catalog-data" type="application/json" { (PreEscaped(json_str)) }
                script { (PreEscaped(TG_JS)) }
            }
        }
    }
}

/// Navigasi antar halaman admin (port `admin/nav.tsx`).
pub fn admin_nav(active: &str) -> Markup {
    let link = |href: &str, label: &str| {
        let is_active = href == active;
        html! { a href=(href) class=(if is_active { "nav-link active" } else { "nav-link" }) { (label) } }
    };
    html! {
        nav class="admin-nav" {
            (link("/admin", "🏠 Dashboard"))
            (link("/admin/tickets", "🎫 Tiket Support"))
            a href="/api/admin/logout" class="nav-logout" { "Keluar" }
        }
    }
}

/// Form login admin (port `admin/page.tsx` saat belum login).
pub fn admin_login(configured: bool, error: bool) -> Markup {
    html! {
        div class="login-card" {
            h1 style="margin: 0 0 4px; font-size: 22px;" { "Admin — Unicodes Digital" }
            p class="muted" style="margin: 0 0 16px;" { "Masuk untuk mengelola toko, tiket, dan pembayaran." }
            @if !configured {
                p class="err" {
                    "ADMIN_PASSWORD belum diisi di env — halaman admin tidak bisa dibuka. Isi dulu di .env.local / Railway Variables."
                }
            } @else {
                @if error {
                    p class="err" { "Password salah, coba lagi." }
                }
                form action="/api/admin/login" method="post" {
                    p style="margin: 0 0 12px;" {
                        input type="password" name="password" placeholder="Password admin" required class="input";
                    }
                    button type="submit" class="btn" style="width: 100%;" { "Masuk" }
                }
            }
        }
    }
}

/// Satu baris pengguna untuk tabel admin.
#[derive(Debug, Clone)]
pub struct AdminUserRow {
    pub tg_id: i64,
    pub first_name: Option<String>,
    pub username: Option<String>,
    pub balance_idr: i64,
    pub created_at: String,
}

/// Semua data dashboard admin.
pub struct AdminDashboardData {
    pub total_users: i64,
    pub total_orders: i64,
    pub delivered_orders: i64,
    pub revenue_idr: i64,
    pub open_tickets: i64,
    pub sondev_balance: String,
    pub sondev_raw: serde_json::Value,
    pub lahastore_balance: String,
    pub products: Vec<(CatalogProduct, Option<i64>)>,
    pub users: Vec<AdminUserRow>,
    pub orders: Vec<StoreOrder>,
}

/// Dashboard `/admin` — port `admin/page.tsx`.
/// Statistik = kartu berjajar, supplier = kartu per supplier, tabel di dalam
/// `.table-scroll` (scroll horizontal di HP). Panel QRIS GoPay/GoBiz DIHAPUS
/// dari tampilan (2026-11-21 — pembayaran sudah Midtrans; backend QRIS/GoBiz
/// tetap ada, dormant). Hanya markup + CSS yang berubah.
pub fn admin_dashboard(d: &AdminDashboardData) -> Markup {
    let sondev_failed = d.sondev_balance.starts_with("belum bisa dicek");
    let lahastore_failed = d.lahastore_balance.starts_with("belum bisa dicek");
    html! {
        (admin_nav("/admin"))
        h1 class="admin-head" { "Admin — Unicodes Digital" }
        p class="admin-sub" { "Ringkasan toko, saldo supplier, produk, dan pengguna." }

        div class="admin-grid" {
            div class="stat-card" {
                div class="num" { (d.total_users) }
                div class="lbl" { "👥 Total pengguna" }
            }
            div class="stat-card" {
                div class="num" { (d.total_orders) }
                div class="lbl" { "📦 Total order" }
            }
            div class="stat-card" {
                div class="num" { (d.delivered_orders) }
                div class="lbl" { "✅ Terkirim" }
            }
            div class="stat-card" {
                div class="num" { (rupiah(d.revenue_idr)) }
                div class="lbl" { "💰 Revenue (order terkirim)" }
            }
            div class="stat-card" {
                div class="num" { (d.open_tickets) }
                div class="lbl" { "🎫 Tiket support terbuka" }
                div class="sub" { a href="/admin/tickets" { "Lihat tiket →" } }
            }
        }

        h2 class="admin-sec" { "Saldo supplier" }
        div class="supplier-grid" {
            div class="supplier-card" {
                div style="display: flex; justify-content: space-between; align-items: center; gap: 8px;" {
                    strong { "Sondev" }
                    span class=(if sondev_failed { "badge badge-err" } else { "badge badge-ok" }) {
                        (if sondev_failed { "Gagal dicek" } else { "Aktif" })
                    }
                }
                p class="muted" style="margin: 2px 0 8px;" { "Muse" }
                div class="bal" { (d.sondev_balance.as_str()) }
                div class="meta" {
                    @if let Some(v) = d.sondev_raw.get("is_reseller").and_then(|v| v.as_bool()) {
                        div { "Reseller: " (if v { "aktif" } else { "belum aktif" }) }
                    }
                    @if let Some(v) = d.sondev_raw.get("webhook_healthy").and_then(|v| v.as_bool()) {
                        div { "Webhook: " (if v { "sehat" } else { "tidak sehat" }) }
                    }
                    @if let Some(v) = d.sondev_raw.get("orders_today").and_then(|v| v.as_i64()) {
                        div { "Order hari ini: " (v) "/" (d.sondev_raw.get("daily_order_limit").and_then(|v| v.as_i64()).map(|n| n.to_string()).unwrap_or_else(|| "?".to_string())) }
                    }
                    @if let Some(v) = d.sondev_raw.get("rate_limit_per_min").and_then(|v| v.as_i64()) {
                        div { "Rate limit: " (v) "/menit" }
                    }
                }
            }
            div class="supplier-card" {
                div style="display: flex; justify-content: space-between; align-items: center; gap: 8px;" {
                    strong { "LahaStore" }
                    span class=(if lahastore_failed { "badge badge-err" } else { "badge badge-ok" }) {
                        (if lahastore_failed { "Gagal dicek" } else { "Aktif" })
                    }
                }
                p class="muted" style="margin: 2px 0 8px;" { "AI lain" }
                div class="bal" { (d.lahastore_balance.as_str()) }
            }
        }
        p class="muted" {
            "Isi saldo sebelum kehabisan — kalau saldo kurang, order baru akan gagal dan pembeli harus di-refund manual."
        }

        h2 class="admin-sec" { "Produk & harga retail" }
        div class="table-scroll" {
            table {
                thead { tr { th { "Produk" } th { "Harga" } th { "Supplier" } th { "Status" } th { "Ubah harga" } } }
                tbody {
                    @for (p, ov) in &d.products {
                        tr {
                            td { (p.name) }
                            td style="color: #f4f4f5; font-weight: 700;" { (rupiah(p.retail_price_idr)) }
                            td { (p.supplier) }
                            td {
                                @if !p.active { span class="badge badge-err" { "Nonaktif" } }
                                @else if ov.is_some() { span class="badge badge-warn" { "Harga diubah" } }
                                @else { span class="badge badge-ok" { "Aktif" } }
                            }
                            td {
                                form action=(format!("/api/admin/products/{}/price", p.id)) method="post" style="display: flex; gap: 6px; align-items: center;" {
                                    input type="number" name="price" value=(p.retail_price_idr) min="500" max="10000000" step="100" required class="input" style="width: 110px;";
                                    button type="submit" class="btn-sm" { "Ubah harga" }
                                }
                            }
                        }
                    }
                }
            }
        }
        p class="muted" {
            "Harga dasar ada di kode katalog — ubah di sini menyimpan override ke database (harga dasar tidak berubah)."
        }

        h2 class="admin-sec" { "Pengguna (" (d.users.len()) ")" }
        @if d.users.is_empty() {
            p { "Belum ada pengguna." }
        } @else {
            div class="table-scroll" {
                table {
                    thead { tr { th { "ID" } th { "Nama" } th { "Username" } th { "Saldo" } th { "Daftar" } } }
                    tbody {
                        @for u in &d.users {
                            tr {
                                td { (u.tg_id) }
                                td { (u.first_name.as_deref().unwrap_or("—")) }
                                td { @if let Some(un) = &u.username { "@" (un) } @else { "—" } }
                                td style="color: #f4f4f5; font-weight: 700;" { (rupiah(u.balance_idr)) }
                                td style="font-size: 13px; color: #a1a1aa;" { (wib_date(&u.created_at)) }
                            }
                        }
                    }
                }
            }
        }

        h2 class="admin-sec" { "Order terbaru (" (d.orders.len()) ")" }
        @if d.orders.is_empty() {
            p { "Belum ada order." }
        } @else {
            div class="table-scroll" {
                table {
                    thead { tr { th { "ID" } th { "Produk" } th { "Total" } th { "Status" } th { "Supplier" } th { "Error" } } }
                    tbody {
                        @for o in &d.orders {
                            tr {
                                td style="font-size: 13px;" { (o.id.as_str()) }
                                td { (o.catalog_product_id.as_str()) " x" (o.quantity) }
                                td style="color: #f4f4f5; font-weight: 700;" { (rupiah(o.retail_total_idr)) }
                                td { (o.status.as_str()) }
                                td { (o.supplier.as_str()) }
                                td style="color: #fca5a5; font-size: 13px;" { (o.error.as_deref().unwrap_or("")) }
                            }
                        }
                    }
                }
            }
        }
    }
}

fn status_label(status: TicketStatus) -> &'static str {
    match status {
        TicketStatus::Open => "🟢 Terbuka",
        TicketStatus::Pending => "🟡 Dibalas admin",
        TicketStatus::Closed => "🔒 Ditutup",
    }
}

/// Daftar tiket `/admin/tickets` — port `admin/tickets/page.tsx`.
/// Tiket terbuka dulu, lalu terbaru. Tabel dalam `.table-scroll` agar
/// bisa di-scroll horizontal di HP.
pub fn tickets_list(tickets: &[SupportTicket], open_count: i64) -> Markup {
    html! {
        (admin_nav("/admin/tickets"))
        h1 class="admin-head" { "🎫 Tiket Support (" (open_count) " terbuka)" }
        p class="admin-sub" { "Tiket terbuka dulu, lalu yang terbaru." }
        @if tickets.is_empty() {
            p { "Belum ada tiket support." }
        } @else {
            div class="table-scroll" {
                table {
                    thead {
                        tr {
                            th { "Tiket" } th { "User" } th { "Order" } th { "Pesan pertama" } th { "Status" } th { "Terakhir" }
                        }
                    }
                    tbody {
                        @for t in tickets {
                            tr {
                                td { a href=(format!("/admin/tickets/{}", t.id)) { (t.id.as_str()) } }
                                td {
                                    (t.first_name.as_deref().unwrap_or("—"))
                                    @if let Some(un) = &t.username { " (@" (un.as_str()) ")" }
                                    br;
                                    span style="color: #a1a1aa; font-size: 13px;" { "tg:" (t.tg_id) }
                                }
                                td style="font-size: 14px;" { (t.order_id.as_deref().unwrap_or("—")) }
                                td style="font-size: 14px;" { @if t.subject.is_empty() { "(tanpa judul)" } @else { (t.subject.as_str()) } }
                                td { (status_label(t.status)) }
                                td style="font-size: 13px; color: #a1a1aa;" { (wib_datetime(&t.last_message_at)) }
                            }
                        }
                    }
                }
            }
        }
    }
}

/// Order yang terikat ke satu tiket (kartu "Order terkait" di detail tiket).
pub struct TicketOrderView {
    pub id: String,
    pub product_name: String,
    pub qty: i64,
    pub status: String,
    pub total_rp: String,
}

/// Detail tiket `/admin/tickets/:id` — port `admin/tickets/[id]/page.tsx`.
/// `order` = order yang dipilih user saat buka tiket (kalau ada).
pub fn ticket_detail(
    ticket: &SupportTicket,
    messages: &[SupportMessage],
    order: Option<&TicketOrderView>,
) -> Markup {
    let closed = ticket.status == TicketStatus::Closed;
    html! {
        (admin_nav("/admin/tickets"))
        p { a href="/admin/tickets" style="font-size: 14px;" { "← Daftar tiket" } }
        h1 class="admin-head" { (ticket.id.as_str()) " — " (status_label(ticket.status)) }
        p class="admin-sub" {
            (ticket.first_name.as_deref().unwrap_or("—"))
            @if let Some(un) = &ticket.username { " (@" (un.as_str()) ")" }
            " · tg:" (ticket.tg_id) " · dibuat " (wib_datetime(&ticket.created_at))
        }
        div class="admin-card" {
            strong { "Topik:" } " " @if ticket.subject.is_empty() { "(tanpa judul)" } @else { (ticket.subject.as_str()) }
        }
        @if let Some(o) = order {
            div class="admin-card" {
                strong { "📦 Order terkait: " }
                (o.id.as_str()) " — " (o.product_name.as_str()) " ×" (o.qty)
                " · " (o.status.as_str()) " · " (o.total_rp.as_str())
            }
        }
        div style="margin: 20px 0;" {
            @for m in messages {
                div class=(if m.sender == "admin" { "msg-bubble admin" } else { "msg-bubble user" }) {
                    div style="font-size: 13px; color: #a1a1aa; margin-bottom: 4px;" {
                        @if m.sender == "admin" { "👨‍💼 Admin" } @else { "👤 User" }
                        " · " (wib_datetime(&m.created_at))
                    }
                    div style="white-space: pre-wrap; font-size: 15px;" { (m.text.as_str()) }
                }
            }
        }
        @if !closed {
            form action=(format!("/api/admin/tickets/{}/reply", ticket.id)) method="post" {
                p {
                    textarea name="text" rows="4" required placeholder="Tulis balasan ke user…" class="input" {}
                }
                button type="submit" class="btn-sm" style="padding: 10px 20px; font-weight: 600;" { "Kirim balasan ke Telegram" }
            }
            form action=(format!("/api/admin/tickets/{}/close", ticket.id)) method="post" style="margin-top: 12px;" {
                button type="submit" class="btn-sm" style="padding: 10px 20px; font-weight: 600; background: #7f1d1d;" { "🔒 Tutup tiket" }
            }
        } @else {
            form action=(format!("/api/admin/tickets/{}/reopen", ticket.id)) method="post" {
                button type="submit" class="btn-sm" style="padding: 10px 20px; font-weight: 600;" { "🔓 Buka lagi tiket ini" }
            }
        }
    }
}

/// Halaman "belum login" untuk /admin/tickets (port TS: tautan ke /admin).
pub fn tickets_login_prompt() -> Markup {
    html! {
        h1 { "Admin — Unicodes Digital" }
        p { "Kamu belum login. " a href="/admin" { "Buka halaman login admin" } "." }
    }
}

/// Halaman "tiket tidak ditemukan".
pub fn ticket_not_found() -> Markup {
    html! {
        (admin_nav("/admin/tickets"))
        h1 { "Tiket tidak ditemukan" }
        p { a href="/admin/tickets" { "← Kembali ke daftar tiket" } }
    }
}
