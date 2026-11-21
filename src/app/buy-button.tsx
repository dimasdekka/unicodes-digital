"use client";

import { useEffect, useState } from "react";

/** Tombol beli: buat order + pembayaran QRIS dinamis (kartu standee komposit),
 *  tampilkan QR di halaman (bukan redirect), lalu pantau status order tiap
 *  10 detik sampai delivered/failed. Produk on-demand (mis. Muse) wajib
 *  target — inputnya tampil di kartu produk. */
export function BuyButton({
  productId,
  requiresTarget,
  targetLabel,
  disabled,
  disabledLabel,
}: {
  productId: string;
  requiresTarget?: boolean;
  targetLabel?: string;
  disabled?: boolean;
  /** Teks tombol saat disabled (mis. "Stok Habis" / "Segera hadir"). */
  disabledLabel?: string;
}) {
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [target, setTarget] = useState("");
  const [payment, setPayment] = useState<{
    redirectUrl: string;
    snapToken: string;
    expiresAt: string;
    orderId: string;
  } | null>(null);
  const [orderStatus, setOrderStatus] = useState<string | null>(null);
  const [orderResult, setOrderResult] = useState<string | null>(null);

  const rupiah = (n: number) => `Rp${n.toLocaleString("id-ID")}`;
  const wibTime = (iso: string) =>
    new Date(iso).toLocaleTimeString("id-ID", {
      timeZone: "Asia/Jakarta",
      hour: "2-digit",
      minute: "2-digit",
    });

  // Pantau status order sampai delivered / failed / refunded, atau sampai
  // QR kedaluwarsa. Berhenti sendiri.
  useEffect(() => {
    if (!payment) return;
    let stopped = false;
    const tick = async () => {
      try {
        const res = await fetch(`/api/orders/${payment.orderId}`);
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
          stopped = true;
        } else if (st === "failed" || st === "refunded") {
          setOrderResult(body.order.error ?? "Order gagal.");
          stopped = true;
        }
      } catch {
        // Gagal cek sekali — coba lagi di tick berikutnya.
      }
    };
    void tick();
    const timer = setInterval(() => {
      if (Date.now() > new Date(payment.expiresAt).getTime()) {
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
  }, [payment]);

  async function buy() {
    if (requiresTarget && !target.trim()) {
      setError(`${targetLabel ?? "Target"} wajib diisi.`);
      return;
    }
    setBusy(true);
    setError(null);
    try {
      const res = await fetch("/api/orders", {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({
          productId,
          quantity: 1,
          buyerRef: "web",
          target: requiresTarget ? target.trim() : undefined,
        }),
      });
      const data = (await res.json()) as {
        ok: boolean;
        error?: string;
        order?: { id: string; status: string };
        payment?: {
          redirectUrl: string;
          snapToken: string;
          expiresAt: string;
          reference: string;
        } | null;
      };
      if (!data.ok || !data.order) {
        throw new Error(data.error ?? "Gagal membuat order");
      }
      if (!data.payment) {
        // Order gratis — langsung diproses.
        setOrderStatus("paid");
        setOrderResult("Order gratis dibuat — sedang diproses.");
        return;
      }
      setPayment({
        redirectUrl: data.payment.redirectUrl,
        snapToken: data.payment.snapToken,
        expiresAt: data.payment.expiresAt,
        orderId: data.order.id,
      });
      setOrderStatus("awaiting_payment");
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
      setBusy(false);
    }
  }

  return (
    <>
      {requiresTarget && !payment && (
        <p style={{ margin: "8px 0" }}>
          <input
            type="text"
            value={target}
            onChange={(e) => setTarget(e.target.value)}
            placeholder={targetLabel ?? "Target"}
            disabled={busy || disabled}
            style={{
              width: "100%",
              padding: 10,
              boxSizing: "border-box",
              background: "#14092b",
              border: "1px solid #6d28d9",
              borderRadius: 8,
              color: "#ede9fe",
            }}
          />
        </p>
      )}
      {payment ? (
        <div style={{ textAlign: "center" }}>
          <a
            href={payment.redirectUrl}
            target="_blank"
            rel="noreferrer"
            style={{
              display: "block",
              padding: 12,
              borderRadius: 8,
              background: "linear-gradient(90deg, #7c3aed, #a855f7)",
              color: "#fff",
              textDecoration: "none",
              fontWeight: 600,
            }}
          >
            💳 Buka Pembayaran (QRIS)
          </a>
          <p style={{ fontSize: 14, margin: "8px 0 0" }}>
            Tap tombol di atas untuk bayar — link berlaku sampai {wibTime(payment.expiresAt)} WIB
            (30 menit).
          </p>
          <p style={{ fontSize: 13, color: "#b3a1e6", margin: "6px 0 0" }}>
            {orderStatus === "awaiting_payment" &&
              "Menunggu pembayaran… (dicek otomatis tiap 10 detik)"}
            {orderStatus === "paid" && "✅ Lunas — sedang diproses ke supplier…"}
            {orderStatus === "processing" && "⏳ Sedang diproses ke supplier…"}
            {orderStatus === "delivered" && `✅ Selesai!\n${orderResult ?? ""}`}
            {(orderStatus === "failed" || orderStatus === "refunded") &&
              `❌ ${orderResult ?? "Order gagal."}`}
          </p>
        </div>
      ) : (
        <button
          type="button"
          onClick={buy}
          disabled={busy || disabled}
          style={{
            width: "100%",
            background: disabled
              ? "#4c4466"
              : "linear-gradient(90deg, #7c3aed, #a855f7)",
            color: "#fff",
            border: "none",
            borderRadius: 10,
            padding: "12px 20px",
            fontSize: 15,
            fontWeight: 700,
            cursor: disabled ? "not-allowed" : "pointer",
            boxShadow: disabled ? "none" : "0 4px 16px #7c3aed66",
          }}
        >
          {disabled ? (disabledLabel ?? "Segera hadir") : busy ? "Memproses..." : "Beli Sekarang"}
        </button>
      )}
      {error && <p style={{ color: "#f87171", fontSize: 13 }}>{error}</p>}
      {!payment && orderResult && (
        <p style={{ color: "#4ade80", fontSize: 13 }}>{orderResult}</p>
      )}
    </>
  );
}
