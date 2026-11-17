"use client";

import { useState } from "react";

/** Tombol beli: buat order + transaksi Midtrans, lalu arahkan ke halaman bayar.
 *  Produk on-demand (mis. Muse) wajib target — inputnya tampil di kartu produk. */
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
        payment?: { redirectUrl: string };
      };
      if (!data.ok || !data.payment?.redirectUrl) {
        throw new Error(data.error ?? "Gagal membuat order");
      }
      window.location.href = data.payment.redirectUrl;
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
      setBusy(false);
    }
  }

  return (
    <>
      {requiresTarget && (
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
      {error && <p style={{ color: "#f87171", fontSize: 13 }}>{error}</p>}
    </>
  );
}
