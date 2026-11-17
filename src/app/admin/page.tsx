import { cookies } from "next/headers";
import { listActiveProducts } from "@/lib/catalog";
import { listStoreOrders } from "@/lib/orders";
import { getSupplier } from "@/lib/suppliers";
import type { SupplierId } from "@/lib/suppliers/types";
import { ADMIN_COOKIE, adminPasswordHash, isAdminCookieValid } from "@/lib/admin-auth";

// Halaman admin dilindungi login password (cookie, lihat /api/admin/login).
// Tanpa ADMIN_PASSWORD di env, halaman tidak bisa dibuka siapa pun.
export const dynamic = "force-dynamic";

const rupiah = (n: number) => `Rp${n.toLocaleString("id-ID")}`;

async function supplierBalance(id: SupplierId): Promise<{ text: string; raw: Record<string, unknown> }> {
  try {
    const b = await getSupplier(id).getBalance();
    const parts = [`USD ${b.usd}`, `IDR ${b.idr}`];
    if (b.credit) parts.push(`credit ${b.credit}`);
    return { text: parts.join(" · "), raw: (b.raw ?? {}) as Record<string, unknown> };
  } catch (err) {
    return {
      text: `belum bisa dicek (${err instanceof Error ? err.message : "key belum diisi?"})`,
      raw: {},
    };
  }
}

export default async function AdminPage({
  searchParams,
}: {
  searchParams?: Promise<{ error?: string }>;
}) {
  const params = await searchParams;
  const jar = await cookies();
  const authed = isAdminCookieValid(jar.get(ADMIN_COOKIE)?.value);

  if (!authed) {
    const configured = adminPasswordHash() !== null;
    return (
      <>
        <h1>Admin — Unicodes Digital</h1>
        {!configured ? (
          <p style={{ color: "#f87171" }}>
            ADMIN_PASSWORD belum diisi di env — halaman admin tidak bisa
            dibuka. Isi dulu di .env.local / Railway Variables.
          </p>
        ) : (
          <>
            {params?.error && (
              <p style={{ color: "#f87171" }}>Password salah, coba lagi.</p>
            )}
            <form action="/api/admin/login" method="post">
              <p>
                <input
                  type="password"
                  name="password"
                  placeholder="Password admin"
                  required
                  style={{
                    padding: 10,
                    background: "#14092b",
                    border: "1px solid #6d28d9",
                    borderRadius: 8,
                    color: "#ede9fe",
                  }}
                />
              </p>
              <button
                type="submit"
                style={{
                  background: "#7c3aed",
                  color: "#fff",
                  border: "none",
                  borderRadius: 8,
                  padding: "10px 20px",
                  fontWeight: 600,
                  cursor: "pointer",
                }}
              >
                Masuk
              </button>
            </form>
          </>
        )}
      </>
    );
  }

  const [orders, sondev, lahastore] = await Promise.all([
    listStoreOrders(50),
    supplierBalance("sondev"),
    supplierBalance("lahastore"),
  ]);
  const products = listActiveProducts();

  return (
    <>
      <h1>Admin — Unicodes Digital</h1>
      <p>
        <a href="/api/admin/logout" style={{ color: "#b3a1e6", fontSize: 13 }}>
          Keluar
        </a>
      </p>

      <h2>Saldo supplier</h2>
      <ul>
        <li>
          <strong>Sondev (Muse):</strong> {sondev.text}
          {typeof sondev.raw.is_reseller === "boolean" && (
            <> — reseller: {sondev.raw.is_reseller ? "aktif" : "belum aktif"}</>
          )}
          {typeof sondev.raw.webhook_healthy === "boolean" && (
            <> — webhook: {sondev.raw.webhook_healthy ? "sehat" : "tidak sehat"}</>
          )}
          {typeof sondev.raw.orders_today === "number" && (
            <> — order hari ini: {sondev.raw.orders_today}/{String(sondev.raw.daily_order_limit ?? "?")}</>
          )}
          {typeof sondev.raw.rate_limit_per_min === "number" && (
            <> — rate limit: {sondev.raw.rate_limit_per_min}/menit</>
          )}
        </li>
        <li>
          <strong>LahaStore (AI lain):</strong> {lahastore.text}
        </li>
      </ul>
      <p style={{ color: "#b3a1e6", fontSize: 13 }}>
        Isi saldo sebelum kehabisan — kalau saldo kurang, order baru akan gagal
        dan pembeli harus di-refund manual.
      </p>

      <h2>Produk &amp; harga retail</h2>
      <ul>
        {products.map((p) => (
          <li key={p.id}>
            {p.name} — {rupiah(p.retailPriceIdr)} (via {p.supplier})
            {!p.active ? " [nonaktif]" : ""}
          </li>
        ))}
      </ul>
      <p style={{ color: "#b3a1e6", fontSize: 13 }}>
        Harga retail masih diatur di <code>src/lib/catalog.ts</code> — panel
        ubah harga dari admin masih TODO.
      </p>

      <h2>Order terbaru ({orders.length})</h2>
      {orders.length === 0 ? (
        <p>Belum ada order.</p>
      ) : (
        <table cellPadding={6} style={{ borderCollapse: "collapse" }}>
          <thead>
            <tr>
              <th align="left">ID</th>
              <th align="left">Produk</th>
              <th align="left">Total</th>
              <th align="left">Status</th>
              <th align="left">Supplier</th>
              <th align="left">Error</th>
            </tr>
          </thead>
          <tbody>
            {orders.map((o) => (
              <tr key={o.id} style={{ borderTop: "1px solid #6d28d9" }}>
                <td>{o.id}</td>
                <td>{o.catalogProductId} x{o.quantity}</td>
                <td>{rupiah(o.retailTotalIdr)}</td>
                <td>{o.status}</td>
                <td>{o.supplier}</td>
                <td style={{ color: "#f87171", fontSize: 12 }}>{o.error ?? ""}</td>
              </tr>
            ))}
          </tbody>
        </table>
      )}
    </>
  );
}
