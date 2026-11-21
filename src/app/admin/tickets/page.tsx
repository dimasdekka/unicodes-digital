// Daftar tiket support — tiket terbuka dulu, lalu terbaru.
// Auth: cookie admin yang sama dengan /admin (401 / redirect ke login
// kalau belum login — halaman ini menampilkan form login yang sama).
import { isAdminPageAuthed } from "../admin-guard";
import AdminNav from "../nav";
import { countOpenTickets, listTickets } from "@/lib/support-tickets";

export const dynamic = "force-dynamic";

const wib = (iso: string) =>
  new Date(iso).toLocaleString("id-ID", {
    timeZone: "Asia/Jakarta",
    day: "2-digit",
    month: "short",
    hour: "2-digit",
    minute: "2-digit",
  });

const statusLabel: Record<string, string> = {
  open: "🟢 Terbuka",
  pending: "🟡 Dibalas admin",
  closed: "🔒 Ditutup",
};

export default async function TicketsPage() {
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

  const tickets = listTickets(100);
  const openCount = countOpenTickets();

  return (
    <>
      <AdminNav active="/admin/tickets" />
      <h1>🎫 Tiket Support ({openCount} terbuka)</h1>
      {tickets.length === 0 ? (
        <p>Belum ada tiket support.</p>
      ) : (
        <table cellPadding={8} style={{ borderCollapse: "collapse", width: "100%" }}>
          <thead>
            <tr style={{ textAlign: "left", color: "#b3a1e6", fontSize: 13 }}>
              <th>Tiket</th>
              <th>User</th>
              <th>Pesan pertama</th>
              <th>Status</th>
              <th>Terakhir</th>
            </tr>
          </thead>
          <tbody>
            {tickets.map((t) => (
              <tr key={t.id} style={{ borderTop: "1px solid #6d28d9" }}>
                <td>
                  <a href={`/admin/tickets/${t.id}`} style={{ color: "#d9b45b" }}>
                    {t.id}
                  </a>
                </td>
                <td>
                  {t.firstName ?? "—"}
                  {t.username ? ` (@${t.username})` : ""}
                  <br />
                  <span style={{ color: "#b3a1e6", fontSize: 12 }}>
                    tg:{t.tgId}
                  </span>
                </td>
                <td style={{ fontSize: 13 }}>{t.subject || "(tanpa judul)"}</td>
                <td>{statusLabel[t.status] ?? t.status}</td>
                <td style={{ fontSize: 12, color: "#b3a1e6" }}>
                  {wib(t.lastMessageAt)}
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      )}
    </>
  );
}
