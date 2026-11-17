// Telegram Web App (Mini App) — toko di dalam Telegram.
// Route: /tg — dibuka dari tombol "🌐 Buka Toko" di bot (type web_app)
// atau dari Menu Button bot (setelah deploy, lihat src/bot/menu-button.ts).
//
// Cara kerja:
// - Script https://telegram.org/js/telegram-web-app.js menyediakan
//   window.Telegram.WebApp (initData, user, themeParams).
// - Katalog diambil dari GET /api/products (sama seperti storefront web).
// - Checkout POST /api/orders dengan initData — server memvalidasi initData
//   (src/lib/telegram-webapp.ts) lalu membuat order + pembayaran QRIS
//   dinamis (kartu standee komposit, base64). QR tampil DI DALAM WebView —
//   tidak ada redirect keluar.
// - Status order dipantau lewat GET /api/orders/[id] tiap 10 detik sampai
//   delivered/failed; setelah lunas, poller feed GoBiz -> markPaid ->
//   fulfillOrder -> bot mengirim hasil order ke chat user (src/lib/notify.ts).
// - Dibuka di luar Telegram: katalog tetap bisa dilihat, checkout menampilkan
//   arahan buka lewat bot. Halaman ini TIDAK mengubah storefront web biasa.
"use client";

import { useEffect, useRef, useState } from "react";
import type { CatalogProduct } from "../../lib/catalog";

interface TgUser {
  id: number;
  first_name?: string;
  username?: string;
}

interface TelegramWebApp {
  initData: string;
  initDataUnsafe?: { user?: TgUser };
  themeParams?: Record<string, string | undefined>;
  ready: () => void;
  expand: () => void;
  setBackgroundColor?: (color: string) => void;
  setHeaderColor?: (color: string) => void;
}

declare global {
  interface Window {
    Telegram?: { WebApp?: TelegramWebApp };
  }
}

const FALLBACK_THEME = {
  bg: "#14092b",
  text: "#ede9fe",
  button: "#7c3aed",
  buttonText: "#ffffff",
  secondaryBg: "#221244",
  hint: "#b3a1e6",
};

interface Theme {
  bg: string;
  text: string;
  button: string;
  buttonText: string;
  secondaryBg: string;
  hint: string;
}

const rupiah = (n: number) => `Rp${n.toLocaleString("id-ID")}`;
const QTY_PACKS = [1, 5, 10, 30];

export default function TgShopPage() {
  const [tg, setTg] = useState<TelegramWebApp | null>(null);
  const [theme, setTheme] = useState<Theme>(FALLBACK_THEME);
  const [products, setProducts] = useState<CatalogProduct[]>([]);
  const [loading, setLoading] = useState(true);
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [qty, setQty] = useState(1);
  const [customQty, setCustomQty] = useState("");
  const [target, setTarget] = useState("");
  const [paying, setPaying] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [doneOrderId, setDoneOrderId] = useState<string | null>(null);
  const [payment, setPayment] = useState<{
    qrPngBase64: string;
    uniqueAmountIdr: number;
    expiresAt: string;
    orderId: string;
  } | null>(null);
  const [orderStatus, setOrderStatus] = useState<string | null>(null);
  const [orderResult, setOrderResult] = useState<string | null>(null);
  const inited = useRef(false);

  // Inisialisasi Telegram WebApp (dipanggil saat mount + saat script selesai load).
  useEffect(() => {
    const init = () => {
      if (inited.current) return;
      const wa = window.Telegram?.WebApp;
      if (!wa) return;
      inited.current = true;
      setTg(wa);
      const tp = wa.themeParams ?? {};
      const next: Theme = {
        bg: tp.bg_color ?? FALLBACK_THEME.bg,
        text: tp.text_color ?? FALLBACK_THEME.text,
        button: tp.button_color ?? FALLBACK_THEME.button,
        buttonText: tp.button_text_color ?? FALLBACK_THEME.buttonText,
        secondaryBg: tp.secondary_bg_color ?? FALLBACK_THEME.secondaryBg,
        hint: tp.hint_color ?? FALLBACK_THEME.hint,
      };
      setTheme(next);
      try {
        wa.ready();
        wa.expand();
        wa.setBackgroundColor?.(next.bg);
        wa.setHeaderColor?.(next.bg);
      } catch {
        // WebApp API tidak tersedia penuh (mis. Telegram lama) — lanjut saja.
      }
    };
    init();
    // Script mungkin belum selesai load saat mount — cek ulang berkala sebentar.
    const timer = setInterval(init, 500);
    const stop = setTimeout(() => clearInterval(timer), 5000);
    return () => {
      clearInterval(timer);
      clearTimeout(stop);
    };
  }, []);

  // Katalog — sama seperti storefront web.
  useEffect(() => {
    let cancelled = false;
    fetch("/api/products")
      .then((r) => r.json())
      .then((j) => {
        if (!cancelled && Array.isArray(j.products)) setProducts(j.products);
      })
      .catch(() => {
        if (!cancelled) setError("Katalog gagal dimuat. Coba tutup lalu buka lagi.");
      })
      .finally(() => {
        if (!cancelled) setLoading(false);
      });
    return () => {
      cancelled = true;
    };
  }, []);

  const selected = products.find((p) => p.id === selectedId) ?? null;
  const user = tg?.initDataUnsafe?.user;
  const effectiveQty = selected?.id === "muse-token-1b" ? qty : 1;

  async function checkout() {
    if (!selected) return;
    if (!tg?.initData) {
      setError(
        "Checkout hanya bisa dari dalam Telegram — buka toko ini lewat tombol 🌐 Buka Toko di bot @unicodesdigital_bot.",
      );
      return;
    }
    if (selected.requiresTarget && !target.trim()) {
      setError("Isi target dulu (kode invite / link referral Muse).");
      return;
    }
    setError(null);
    setPaying(true);
    try {
      const res = await fetch("/api/orders", {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({
          productId: selected.id,
          quantity: effectiveQty,
          target: selected.requiresTarget ? target.trim() : undefined,
          initData: tg.initData,
        }),
      });
      const body = (await res.json().catch(() => ({}))) as Record<string, unknown>;
      if (!res.ok || body.ok !== true) {
        throw new Error(
          typeof body.error === "string" ? body.error : "Checkout gagal.",
        );
      }
      const order = body.order as { id: string; status: string };
      const pay = body.payment as {
        qrPngBase64: string;
        uniqueAmountIdr: number;
        expiresAt: string;
      } | null;
      setDoneOrderId(order.id);
      if (!pay) {
        // Order gratis — langsung diproses, pantau statusnya saja.
        setOrderStatus("paid");
        return;
      }
      setPayment({
        qrPngBase64: pay.qrPngBase64,
        uniqueAmountIdr: pay.uniqueAmountIdr,
        expiresAt: pay.expiresAt,
        orderId: order.id,
      });
      setOrderStatus("awaiting_payment");
    } catch (err) {
      setError(err instanceof Error ? err.message : "Checkout gagal.");
    } finally {
      setPaying(false);
    }
  }

  // Pantau status order tiap 10 detik sampai delivered / failed / refunded,
  // atau sampai QR kedaluwarsa. Berhenti sendiri — tidak polling selamanya.
  useEffect(() => {
    if (!doneOrderId) return;
    let stopped = false;
    const tick = async () => {
      try {
        const res = await fetch(`/api/orders/${doneOrderId}`);
        const body = (await res.json().catch(() => ({}))) as {
          ok?: boolean;
          order?: {
            status?: string;
            deliveredItems?: string[];
            resultLink?: string | null;
            error?: string | null;
          };
        };
        if (stopped || !body.ok || !body.order) return;
        const st = body.order.status ?? "";
        setOrderStatus(st);
        if (st === "delivered") {
          setOrderResult(
            (body.order.deliveredItems ?? []).join("\n") ||
              body.order.resultLink ||
              "Order selesai.",
          );
        } else if (st === "failed" || st === "refunded") {
          setOrderResult(body.order.error ?? "Order gagal.");
        }
        if (st === "delivered" || st === "failed" || st === "refunded") {
          stopped = true;
        }
      } catch {
        // Gagal cek sekali — coba lagi di tick berikutnya.
      }
    };
    void tick();
    const timer = setInterval(() => {
      if (payment && Date.now() > new Date(payment.expiresAt).getTime()) {
        setOrderStatus("failed");
        setOrderResult("QR kedaluwarsa sebelum dibayar — buat order baru.");
        stopped = true;
        clearInterval(timer);
        return;
      }
      void tick();
    }, 10_000);
    return () => {
      stopped = true;
      clearInterval(timer);
    };
  }, [doneOrderId, payment]);

  const cardStyle: React.CSSProperties = {
    background: theme.secondaryBg,
    border: `1px solid ${theme.button}`,
    borderRadius: 12,
    padding: 12,
  };
  const btnStyle: React.CSSProperties = {
    background: theme.button,
    color: theme.buttonText,
    border: "none",
    borderRadius: 10,
    padding: "10px 14px",
    fontWeight: 700,
    cursor: "pointer",
    width: "100%",
  };

  return (
    <div
      style={{
        background: theme.bg,
        color: theme.text,
        minHeight: "100vh",
        padding: 16,
        fontFamily: "system-ui, sans-serif",
      }}
    >
      {/* Script resmi Telegram Web App */}
      <script src="https://telegram.org/js/telegram-web-app.js" async />
      <h1 style={{ fontSize: 20, margin: "4px 0 2px" }}>
        💜 Unicodes Digital
      </h1>
      <p style={{ color: theme.hint, margin: "0 0 16px", fontSize: 13 }}>
        {user ? `Hai, ${user.first_name ?? "teman"}! ` : ""}
        Toko AI & produk digital — harga jujur, proses instan.
      </p>

      {!tg && !loading && (
        <p
          style={{
            background: theme.secondaryBg,
            borderRadius: 10,
            padding: 10,
            fontSize: 13,
          }}
        >
          Kamu membuka toko ini di luar Telegram. Katalog tetap bisa dilihat —
          untuk beli, buka lewat tombol 🌐 Buka Toko di bot @unicodesdigital_bot.
        </p>
      )}

      {loading && <p>Memuat katalog…</p>}
      {error && (
        <p style={{ background: "#7f1d1d", borderRadius: 10, padding: 10, fontSize: 13 }}>
          {error}
        </p>
      )}
      {doneOrderId && (
        <p style={{ background: "#14532d", borderRadius: 10, padding: 10, fontSize: 13 }}>
          ✅ Order {doneOrderId} dibuat! Selesaikan pembayaran di tab yang terbuka
          — hasilnya akan dikirim bot ke chat Telegram kamu.
        </p>
      )}

      {!selected && (
        <div style={{ display: "grid", gap: 12 }}>
          {products.map((p) => (
            <div key={p.id} style={cardStyle}>
              <div style={{ fontWeight: 700 }}>
                {p.badge ? `🏷️ ${p.badge} — ` : ""}
                {p.name}
              </div>
              <div style={{ fontSize: 13, color: theme.hint, margin: "4px 0" }}>
                {p.description}
              </div>
              <div style={{ fontWeight: 700, color: "#e8c874" }}>
                {rupiah(p.retailPriceIdr)}
                {p.unit ? ` ${p.unit}` : ""}
              </div>
              <button
                style={{ ...btnStyle, marginTop: 10 }}
                disabled={p.stockState !== "available"}
                onClick={() => {
                  setSelectedId(p.id);
                  setQty(1);
                  setCustomQty("");
                  setTarget("");
                  setError(null);
                  setDoneOrderId(null);
                }}
              >
                {p.stockState === "available"
                  ? "Beli"
                  : p.stockState === "out_of_stock"
                    ? "Stok Habis"
                    : "Segera hadir"}
              </button>
            </div>
          ))}
          {!loading && products.length === 0 && <p>Katalog kosong.</p>}
        </div>
      )}

      {selected && (
        <div style={cardStyle}>
          <div style={{ fontWeight: 700 }}>{selected.name}</div>
          <div style={{ fontSize: 13, color: theme.hint, margin: "4px 0" }}>
            {selected.description}
          </div>
          {(selected.features ?? []).map((f) => (
            <div key={f} style={{ fontSize: 13 }}>
              ✅ {f}
            </div>
          ))}
          <div style={{ fontWeight: 700, color: "#e8c874", margin: "8px 0" }}>
            {rupiah(selected.retailPriceIdr)}
            {selected.unit ? ` ${selected.unit}` : ""}
          </div>

          {selected.id === "muse-token-1b" && (
            <div style={{ margin: "8px 0" }}>
              <div style={{ fontSize: 13, marginBottom: 6 }}>Jumlah:</div>
              <div style={{ display: "flex", gap: 8, flexWrap: "wrap" }}>
                {QTY_PACKS.map((n) => (
                  <button
                    key={n}
                    onClick={() => {
                      setQty(n);
                      setCustomQty("");
                    }}
                    style={{
                      ...btnStyle,
                      width: "auto",
                      background: qty === n && !customQty ? theme.button : theme.secondaryBg,
                      border: `1px solid ${theme.button}`,
                    }}
                  >
                    {n}B
                  </button>
                ))}
                <input
                  type="number"
                  min={1}
                  max={30}
                  placeholder="Lainnya"
                  value={customQty}
                  onChange={(e) => {
                    const v = e.target.value;
                    setCustomQty(v);
                    const n = Number(v);
                    if (Number.isFinite(n) && n >= 1 && n <= 30) setQty(Math.floor(n));
                  }}
                  style={{
                    width: 90,
                    borderRadius: 10,
                    border: `1px solid ${theme.button}`,
                    background: theme.bg,
                    color: theme.text,
                    padding: "10px",
                  }}
                />
              </div>
            </div>
          )}

          {selected.requiresTarget && (
            <div style={{ margin: "8px 0" }}>
              <div style={{ fontSize: 13, marginBottom: 6 }}>
                {selected.targetLabel ?? "Target"}
              </div>
              <input
                type="text"
                placeholder="https://muse.ai/invite/..."
                value={target}
                onChange={(e) => setTarget(e.target.value)}
                style={{
                  width: "100%",
                  boxSizing: "border-box",
                  borderRadius: 10,
                  border: `1px solid ${theme.button}`,
                  background: theme.bg,
                  color: theme.text,
                  padding: "10px",
                }}
              />
            </div>
          )}

          <div style={{ fontWeight: 700, margin: "10px 0" }}>
            Total: {rupiah(selected.retailPriceIdr * effectiveQty)}
          </div>
          {payment ? (
            <div style={{ textAlign: "center" }}>
              {/* eslint-disable-next-line @next/next/no-img-element */}
              <img
                src={`data:image/png;base64,${payment.qrPngBase64}`}
                alt="QRIS pembayaran"
                style={{ width: "100%", maxWidth: 360, borderRadius: 12 }}
              />
              <div style={{ fontSize: 14, marginTop: 8 }}>
                Scan QR di atas — nominal{" "}
                <strong>{rupiah(payment.uniqueAmountIdr)}</strong> (harus pas).
                Berlaku sampai{" "}
                {new Date(payment.expiresAt).toLocaleTimeString("id-ID", {
                  timeZone: "Asia/Jakarta",
                  hour: "2-digit",
                  minute: "2-digit",
                })}{" "}
                WIB.
              </div>
              <div style={{ fontSize: 13, color: theme.hint, marginTop: 6 }}>
                {orderStatus === "awaiting_payment" &&
                  "Menunggu pembayaran… (dicek otomatis tiap 10 detik)"}
                {orderStatus === "paid" && "✅ Lunas — sedang diproses ke supplier…"}
                {orderStatus === "processing" && "⏳ Sedang diproses ke supplier…"}
                {orderStatus === "delivered" && `✅ Selesai!\n${orderResult ?? ""}`}
                {(orderStatus === "failed" || orderStatus === "refunded") &&
                  `❌ ${orderResult ?? "Order gagal."}`}
              </div>
            </div>
          ) : doneOrderId ? (
            <div style={{ fontSize: 14 }}>
              {orderStatus === "delivered"
                ? `✅ Selesai!\n${orderResult ?? ""}`
                : orderStatus === "failed" || orderStatus === "refunded"
                  ? `❌ ${orderResult ?? "Order gagal."}`
                  : "✅ Order dibuat — sedang diproses…"}
            </div>
          ) : (
            <button style={btnStyle} disabled={paying} onClick={checkout}>
              {paying ? "Memproses…" : `Bayar ${rupiah(selected.retailPriceIdr * effectiveQty)}`}
            </button>
          )}
          <button
            style={{
              ...btnStyle,
              background: "transparent",
              border: `1px solid ${theme.button}`,
              marginTop: 8,
            }}
            onClick={() => {
              setSelectedId(null);
              setError(null);
              setDoneOrderId(null);
              setPayment(null);
              setOrderStatus(null);
              setOrderResult(null);
            }}
          >
            ⬅️ Kembali ke katalog
          </button>
        </div>
      )}
    </div>
  );
}
