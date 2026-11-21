// Detail tiket: thread lengkap + balas + tutup / buka lagi.
// Balasan dikirim ke user via Bot API Telegram (POST /api/admin/tickets/[id]/reply).
import { isAdminPageAuthed } from "../../admin-guard";
import AdminNav from "../../nav";
import { getTicket, getTicketMessages } from "@/lib/support-tickets";

export const dynamic = "force-dynamic";

const wib = (iso: string) =>
  new Date(iso).toLocaleString("id-ID", {
    timeZone: "Asia/Jakarta",
    day: "2-digit",
    month: "short",
    hour: "2-digit",
    minute: "2-digit",
  });

const inputStyle = {
  width: "100%",
  padding: 10,
  background: "#14092b",
  border: "1px solid #6d28d9",
  borderRadius: 8,
  color: "#ede9fe",
  fontFamily: "inherit",
  boxSizing: "border-box" as const,
};

const buttonStyle = {
  background: "#7c3aed",
  color: "#fff",
  border: "none",
  borderRadius: 8,
  padding: "10px 20px",
  fontWeight: 600,
  cursor: "pointer",
} as const;

const statusLabel: Record<string, string> = {
  open: "🟢 Terbuka",
  pending: "🟡 Dibalas admin",
  closed: "🔒 Ditutup",
};

export default async function TicketDetailPage({
  params,
}: {
  params: Promise<{ id: string }>;
}) {
  const { id } = await params;
  if (!(await isAdminPageAuthed())) {
    return (
      <>
        <h1>Admin — Unicodes Digital</h1>
        <p>
          Kamu belum login. <a href="/admin">Buka halaman login admin</a>.
        </p>
      </>
    );
  }

  const ticket = getTicket(id);
  if (!ticket) {
    return (
      <>
        <AdminNav active="/admin/tickets" />
        <h1>Tiket tidak ditemukan</h1>
        <p>
          <a href="/admin/tickets">← Kembali ke daftar tiket</a>
        </p>
      </>
    );
  }
  const messages = getTicketMessages(id);

  return (
    <>
      <AdminNav active="/admin/tickets" />
      <p>
        <a href="/admin/tickets" style={{ color: "#b3a1e6", fontSize: 13 }}>
          ← Daftar tiket
        </a>
      </p>
      <h1>
        {ticket.id} — {statusLabel[ticket.status] ?? ticket.status}
      </h1>
      <p style={{ color: "#b3a1e6", fontSize: 13 }}>
        {ticket.firstName ?? "—"}
        {ticket.username ? ` (@${ticket.username})` : ""} · tg:{ticket.tgId} ·
        dibuat {wib(ticket.createdAt)}
      </p>
      <p style={{ fontSize: 14 }}>
        <strong>Topik:</strong> {ticket.subject || "(tanpa judul)"}
      </p>

      <div style={{ margin: "20px 0" }}>
        {messages.map((m) => (
          <div
            key={m.id}
            style={{
              border: "1px solid #6d28d9",
              borderRadius: 8,
              padding: "10px 12px",
              marginBottom: 10,
              background: m.sender === "admin" ? "#221244" : "#1a0f38",
            }}
          >
            <div style={{ fontSize: 12, color: "#b3a1e6", marginBottom: 4 }}>
              {m.sender === "admin" ? "👨‍💼 Admin" : "👤 User"} · {wib(m.createdAt)}
            </div>
            <div style={{ whiteSpace: "pre-wrap", fontSize: 14 }}>{m.text}</div>
          </div>
        ))}
      </div>

      {ticket.status !== "closed" ? (
        <>
          <form action={`/api/admin/tickets/${ticket.id}/reply`} method="post">
            <p>
              <textarea
                name="text"
                rows={4}
                required
                placeholder="Tulis balasan ke user…"
                style={inputStyle}
              />
            </p>
            <button type="submit" style={buttonStyle}>
              Kirim balasan ke Telegram
            </button>
          </form>
          <form
            action={`/api/admin/tickets/${ticket.id}/close`}
            method="post"
            style={{ marginTop: 12 }}
          >
            <button
              type="submit"
              style={{ ...buttonStyle, background: "#4c1d95" }}
            >
              🔒 Tutup tiket
            </button>
          </form>
        </>
      ) : (
        <form action={`/api/admin/tickets/${ticket.id}/reopen`} method="post">
          <button type="submit" style={buttonStyle}>
            🔓 Buka lagi tiket ini
          </button>
        </form>
      )}
    </>
  );
}
