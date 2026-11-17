import { listActiveProducts } from "@/lib/catalog";
import { listStoreOrders } from "@/lib/orders";
import { getSupplier } from "@/lib/suppliers";
import type { SupplierId } from "@/lib/suppliers/types";

// TODO: lindungi halaman ini sebelum dibuka ke publik
// (mis. middleware + password admin / session login). Saat ini terbuka.
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

export default async function AdminPage() {
  const [orders, sondev, lahastore] = await Promise.all([
    listStoreOrders(50),
    supplierBalance("sondev"),
    supplierBalance("lahastore"),
  ]);
  const products = listActiveProducts();

  return (
    <>
      <h1>Admin — Unicodes Digital</h1>

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
