// Panel "Pembayaran — QRIS GoPay" di /admin (client component).
// Dim menempel string QRIS statis GoBiz-nya sendiri di sini — tidak perlu
// kirim lewat chat. Login OTP GoBiz (Fase 2) nanti ditambah di seksi ini
// sebagai status sesi + tombol login.
"use client";

import { useCallback, useEffect, useState } from "react";

export interface QrisStatus {
  configured: boolean;
  merchant_name: string | null;
  merchant_city: string | null;
  merchant_id: string | null;
}

interface TestQr {
  qr_png_base64: string;
  qr_string: string;
  unique_amount: number;
  expires_at: string;
  reference: string;
  amount_idr: number;
}

interface QrisPaymentRow {
  reference: string;
  amount: number;
  unique_amount: number;
  status: string;
  created_at: string;
}

const rupiah = (n: number) => `Rp${n.toLocaleString("id-ID")}`;

const wib = (iso: string) =>
  new Date(iso).toLocaleString("id-ID", { timeZone: "Asia/Jakarta" });

interface GopaySession {
  authenticated: boolean;
  phone: string | null;
  expiresAt: string | null;
  merchantId: string | null;
  /** Nomor HP GoBiz default (env GOBIZ_PHONE) — untuk prefill input OTP. */
  defaultPhone?: string | null;
}

interface GopayMerchant {
  merchantName: string | null;
  outletName: string | null;
  outlets: { popId: string; name: string; status: string; qrString: string | null }[];
}

/** Seksi sesi GoBiz: login OTP + profil merchant + tombol cek pembayaran.
 *  OTP diketik Dim di sini (bukan di chat). Token TIDAK PERNAH tampil. */
function GopaySessionPanel() {
  const [session, setSession] = useState<GopaySession | null>(null);
  const [sessionErr, setSessionErr] = useState<string | null>(null);
  const [merchant, setMerchant] = useState<{
    profile: GopayMerchant;
    staticQrisMatches: boolean;
    warnings: string[];
  } | null>(null);
  const [merchantErr, setMerchantErr] = useState<string | null>(null);
  const [phone, setPhone] = useState("");
  const [otp, setOtp] = useState("");
  const [busy, setBusy] = useState(false);
  const [msg, setMsg] = useState<string | null>(null);
  const [err, setErr] = useState<string | null>(null);
  const [pollRes, setPollRes] = useState<string | null>(null);

  const loadSession = useCallback(async () => {
    try {
      const res = await fetch("/api/admin/payments/gopay/session");
      const data = await res.json();
      if (!res.ok) throw new Error(data.error ?? "Gagal memuat sesi");
      setSession(data as GopaySession);
      setSessionErr(null);
      // Prefill nomor HP dari default (GOBIZ_PHONE) kalau input masih kosong.
      if (typeof data.defaultPhone === "string" && data.defaultPhone) {
        setPhone((prev) => prev || data.defaultPhone);
      }
    } catch (e) {
      setSessionErr(e instanceof Error ? e.message : "Gagal memuat sesi");
    }
  }, []);

  const loadMerchant = useCallback(async () => {
    try {
      const res = await fetch("/api/admin/payments/gopay/merchant");
      const data = await res.json();
      if (!res.ok) throw new Error(data.error ?? "Gagal memuat profil");
      setMerchant({
        profile: data.profile as GopayMerchant,
        staticQrisMatches: !!data.staticQrisMatches,
        warnings: (data.warnings ?? []) as string[],
      });
      setMerchantErr(null);
    } catch (e) {
      setMerchantErr(e instanceof Error ? e.message : "Gagal memuat profil");
    }
  }, []);

  useEffect(() => {
    void loadSession();
  }, [loadSession]);

  useEffect(() => {
    if (session?.authenticated) void loadMerchant();
  }, [session?.authenticated, loadMerchant]);

  async function requestOtp() {
    setBusy(true);
    setMsg(null);
    setErr(null);
    try {
      const res = await fetch("/api/admin/payments/gopay/otp", {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ phone }),
      });
      const data = await res.json();
      if (!res.ok) throw new Error(data.error ?? "Gagal minta OTP");
      setMsg(`OTP dikirim ke ${data.phone ?? phone} — cek SMS/WhatsApp GoBiz, lalu ketik kodenya di bawah (berlaku 10 menit).`);
    } catch (e) {
      setErr(e instanceof Error ? e.message : "Gagal minta OTP");
    } finally {
      setBusy(false);
    }
  }

  async function verifyOtp() {
    setBusy(true);
    setMsg(null);
    setErr(null);
    try {
      const res = await fetch("/api/admin/payments/gopay/verify", {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ otp }),
      });
      const data = await res.json();
      if (!res.ok) throw new Error(data.error ?? "Verifikasi gagal");
      setSession(data.session as GopaySession);
      setOtp("");
      setMsg("Login GoBiz berhasil — poller feed sekarang aktif.");
    } catch (e) {
      setErr(e instanceof Error ? e.message : "Verifikasi gagal");
    } finally {
      setBusy(false);
    }
  }

  async function logout() {
    setBusy(true);
    try {
      await fetch("/api/admin/payments/gopay/logout", { method: "POST" });
      setSession({ authenticated: false, phone: null, expiresAt: null, merchantId: null });
      setMerchant(null);
      setMsg("Sesi GoBiz dihapus — poller berhenti sampai login lagi.");
    } finally {
      setBusy(false);
    }
  }

  async function pollNow() {
    setBusy(true);
    setPollRes(null);
    setErr(null);
    try {
      const res = await fetch("/api/admin/payments/gopay/poll", { method: "POST" });
      const data = await res.json();
      if (!res.ok) throw new Error(data.error ?? "Poll gagal");
      setPollRes(
        `Dicek ${data.checked} pembayaran pending — lunas ${data.matched}, kedaluwarsa ${data.expired}.`,
      );
    } catch (e) {
      setErr(e instanceof Error ? e.message : "Poll gagal");
    } finally {
      setBusy(false);
    }
  }

  return (
    <>
      <h3>Sesi GoBiz (login merchant)</h3>
      <p style={{ color: "#b3a1e6", fontSize: 13 }}>
        Bot membaca pembayaran masuk lewat feed GoBiz — butuh sesi login
        merchant (OTP sekali, lalu refresh otomatis). Sesi tersimpan di
        database server, bukan di file.
      </p>
      {sessionErr && <p style={{ color: "#f87171" }}>{sessionErr}</p>}
      {session && (
        <p style={{ color: "#b3a1e6", fontSize: 13 }}>
          Status:{" "}
          {session.authenticated ? (
            <>
              <strong style={{ color: "#4ade80" }}>login</strong>
              {session.phone && <> — HP {session.phone}</>}
              {session.expiresAt && <> — token berlaku s.d. {wib(session.expiresAt)} WIB</>}
            </>
          ) : (
            <strong style={{ color: "#f87171" }}>
              belum login / sesi kedaluwarsa — minta OTP di bawah
            </strong>
          )}
        </p>
      )}
      {merchant && (
        <p style={{ color: "#b3a1e6", fontSize: 13 }}>
          Merchant: {merchant.profile.merchantName ?? "?"} ({merchant.profile.outletName ?? "?"})
          {" — "}QRIS statis outlet{" "}
          {merchant.staticQrisMatches ? (
            <strong style={{ color: "#4ade80" }}>cocok dengan panel</strong>
          ) : (
            <strong style={{ color: "#f87171" }}>BERBEDA dari panel — perbarui string QRIS</strong>
          )}
        </p>
      )}
      {merchant?.warnings.map((w) => (
        <p key={w} style={{ color: "#fbbf24", fontSize: 13 }}>
          ⚠️ {w}
        </p>
      ))}
      {merchantErr && <p style={{ color: "#f87171" }}>{merchantErr}</p>}
      <p>
        <input
          type="tel"
          value={phone}
          onChange={(e) => setPhone(e.target.value)}
          placeholder="Nomor HP GoBiz (default: nomor tersimpan)"
          style={{ ...inputStyle, width: 260 }}
        />{" "}
        <button type="button" onClick={requestOtp} disabled={busy || !phone.trim()} style={buttonStyle}>
          Minta OTP
        </button>
      </p>
      <p>
        <input
          type="text"
          inputMode="numeric"
          value={otp}
          onChange={(e) => setOtp(e.target.value)}
          placeholder="Kode OTP (4–8 digit)"
          style={{ ...inputStyle, width: 200 }}
        />{" "}
        <button type="button" onClick={verifyOtp} disabled={busy || !otp.trim()} style={buttonStyle}>
          Verifikasi OTP
        </button>{" "}
        <button
          type="button"
          onClick={logout}
          disabled={busy || !session?.authenticated}
          style={{ ...buttonStyle, background: "#4c4466" }}
        >
          Keluar
        </button>
      </p>
      <p>
        <button
          type="button"
          onClick={pollNow}
          disabled={busy || !session?.authenticated}
          style={buttonStyle}
        >
          Cek pembayaran masuk sekarang
        </button>
      </p>
      {msg && <p style={{ color: "#4ade80" }}>{msg}</p>}
      {err && <p style={{ color: "#f87171" }}>{err}</p>}
      {pollRes && <p style={{ color: "#4ade80" }}>{pollRes}</p>}
    </>
  );
}

const inputStyle: React.CSSProperties = {
  padding: 10,
  background: "#14092b",
  border: "1px solid #6d28d9",
  borderRadius: 8,
  color: "#ede9fe",
  width: "100%",
  boxSizing: "border-box",
};

const buttonStyle: React.CSSProperties = {
  background: "#7c3aed",
  color: "#fff",
  border: "none",
  borderRadius: 8,
  padding: "10px 20px",
  fontWeight: 600,
  cursor: "pointer",
};

export default function QrisPanel({
  initialStatus,
}: {
  initialStatus: QrisStatus;
}) {
  const [status, setStatus] = useState<QrisStatus>(initialStatus);
  const [staticQris, setStaticQris] = useState("");
  const [saving, setSaving] = useState(false);
  const [saveMsg, setSaveMsg] = useState<string | null>(null);
  const [saveErr, setSaveErr] = useState<string | null>(null);

  const [testAmount, setTestAmount] = useState("1500");
  const [testLoading, setTestLoading] = useState(false);
  const [testQr, setTestQr] = useState<TestQr | null>(null);
  const [testErr, setTestErr] = useState<string | null>(null);

  const [payments, setPayments] = useState<QrisPaymentRow[]>([]);
  const [paymentsErr, setPaymentsErr] = useState<string | null>(null);

  const loadPayments = useCallback(async () => {
    try {
      const res = await fetch("/api/admin/payments/qris/payments");
      const data = await res.json();
      if (!res.ok) throw new Error(data.error ?? "Gagal memuat pembayaran");
      setPayments(data.payments ?? []);
    } catch (err) {
      setPaymentsErr(err instanceof Error ? err.message : "Gagal memuat");
    }
  }, []);

  useEffect(() => {
    void loadPayments();
  }, [loadPayments]);

  async function saveQris() {
    setSaving(true);
    setSaveMsg(null);
    setSaveErr(null);
    try {
      const res = await fetch("/api/admin/payments/qris", {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ static_qris: staticQris }),
      });
      const data = await res.json();
      if (!res.ok) throw new Error(data.error ?? "Gagal menyimpan");
      setStatus({
        configured: true,
        merchant_name: data.merchant_name,
        merchant_city: data.merchant_city,
        merchant_id: data.merchant_id,
      });
      setSaveMsg(
        `Tersimpan — merchant: ${data.merchant_name ?? "?"}${data.merchant_city ? ` (${data.merchant_city})` : ""}`,
      );
      setStaticQris("");
    } catch (err) {
      setSaveErr(err instanceof Error ? err.message : "Gagal menyimpan");
    } finally {
      setSaving(false);
    }
  }

  async function makeTestQr() {
    setTestLoading(true);
    setTestQr(null);
    setTestErr(null);
    try {
      const res = await fetch(
        `/api/admin/payments/qris/test?amount=${encodeURIComponent(testAmount)}`,
      );
      const data = await res.json();
      if (!res.ok) throw new Error(data.error ?? "Gagal membuat QR tes");
      setTestQr(data as TestQr);
      void loadPayments();
    } catch (err) {
      setTestErr(err instanceof Error ? err.message : "Gagal membuat QR tes");
    } finally {
      setTestLoading(false);
    }
  }

  return (
    <>
      <GopaySessionPanel />
      <p style={{ color: "#b3a1e6", fontSize: 13 }}>
        Status:{" "}
        {status.configured ? (
          <>
            <strong style={{ color: "#4ade80" }}>terpasang</strong>
            {status.merchant_name && <> — {status.merchant_name}</>}
            {status.merchant_city && <> ({status.merchant_city})</>}
            {status.merchant_id && <> — ID {status.merchant_id}</>}
          </>
        ) : (
          <strong style={{ color: "#f87171" }}>
            belum dipasang — tempel string QRIS statis GoBiz di bawah
          </strong>
        )}
      </p>

      <p style={{ color: "#b3a1e6", fontSize: 13 }}>
        Cara ambil string-nya: buka aplikasi GoBiz → menu QRIS / kode QR
        toko → salin string QRIS statis (bukan foto QR-nya). String ini
        tersimpan di database server, bukan di file — dan tidak pernah
        ditampilkan lagi di halaman ini.
      </p>

      <p>
        <textarea
          value={staticQris}
          onChange={(e) => setStaticQris(e.target.value)}
          placeholder="Tempel string QRIS statis GoBiz di sini (diawali 00020101021126...)"
          rows={3}
          style={{ ...inputStyle, fontFamily: "monospace", fontSize: 12 }}
        />
      </p>
      <p>
        <button type="button" onClick={saveQris} disabled={saving} style={buttonStyle}>
          {saving ? "Menyimpan..." : "Simpan QRIS GoPay"}
        </button>
      </p>
      {saveMsg && <p style={{ color: "#4ade80" }}>{saveMsg}</p>}
      {saveErr && <p style={{ color: "#f87171" }}>{saveErr}</p>}

      <h3>QR tes</h3>
      <p style={{ color: "#b3a1e6", fontSize: 13 }}>
        Buat QR dinamis sungguhan dari QRIS yang terpasang, lalu scan dari
        e-wallet kamu untuk melihat nama merchant + nominal sebelum bayar.
        Tidak perlu dibayar — QR kedaluwarsa sendiri dalam 5 menit.
      </p>
      <p>
        <input
          type="number"
          min={100}
          value={testAmount}
          onChange={(e) => setTestAmount(e.target.value)}
          placeholder="Nominal (Rp)"
          style={{ ...inputStyle, width: 200 }}
        />{" "}
        <button
          type="button"
          onClick={makeTestQr}
          disabled={testLoading || !status.configured}
          style={buttonStyle}
        >
          {testLoading ? "Membuat..." : "Buat QR Tes"}
        </button>
      </p>
      {testErr && <p style={{ color: "#f87171" }}>{testErr}</p>}
      {testQr && (
        <div style={{ marginTop: 8 }}>
          <img
            src={`data:image/png;base64,${testQr.qr_png_base64}`}
            alt="QRIS tes"
            width={256}
            height={256}
            style={{ background: "#fff", borderRadius: 8 }}
          />
          <p>
            Bayar <strong>{rupiah(testQr.unique_amount)}</strong> (nominal
            unik — dasar {rupiah(testQr.amount_idr)}) sebelum{" "}
            {wib(testQr.expires_at)} WIB.
            <br />
            <span style={{ color: "#b3a1e6", fontSize: 13 }}>
              Referensi: {testQr.reference}
            </span>
          </p>
        </div>
      )}

      <h3>Pembayaran QRIS terbaru</h3>
      <p>
        <button
          type="button"
          onClick={() => void loadPayments()}
          style={{ ...buttonStyle, padding: "6px 14px", fontSize: 13 }}
        >
          Muat ulang
        </button>
      </p>
      {paymentsErr && <p style={{ color: "#f87171" }}>{paymentsErr}</p>}
      {payments.length === 0 ? (
        <p>Belum ada pembayaran QRIS.</p>
      ) : (
        <table cellPadding={6} style={{ borderCollapse: "collapse" }}>
          <thead>
            <tr>
              <th align="left">Referensi</th>
              <th align="left">Nominal</th>
              <th align="left">Nominal unik</th>
              <th align="left">Status</th>
              <th align="left">Waktu</th>
            </tr>
          </thead>
          <tbody>
            {payments.map((p) => (
              <tr key={p.reference} style={{ borderTop: "1px solid #6d28d9" }}>
                <td style={{ fontSize: 12 }}>{p.reference}</td>
                <td>{rupiah(p.amount)}</td>
                <td>{rupiah(p.unique_amount)}</td>
                <td>{p.status}</td>
                <td style={{ fontSize: 12 }}>{wib(p.created_at)}</td>
              </tr>
            ))}
          </tbody>
        </table>
      )}
    </>
  );
}
