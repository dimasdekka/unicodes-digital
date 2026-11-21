import { cookies } from "next/headers";
import { listActiveProducts } from "@/lib/catalog";
import { getDb } from "@/lib/db";
import { getStoreStats, listStoreOrders } from "@/lib/orders";
import { getPriceOverride } from "@/lib/product-prices";
import { getSupplier } from "@/lib/suppliers";
import type { SupplierId } from "@/lib/suppliers/types";
import { countOpenTickets } from "@/lib/support-tickets";
import { ADMIN_COOKIE, adminPasswordHash, isAdminCookieValid } from "@/lib/admin-auth";
import {
  extractMerchantInfo,
  getGopayStaticQris,
} from "@/lib/payments/qris-settings";
import AdminNav from "./nav";
import QrisPanel, { type QrisStatus } from "./qris-panel";

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
  const store = getStoreStats();
  const orderTotals = getDb()
    .prepare(
      `SELECT COUNT(*) AS total,
        COALESCE(SUM(CASE WHEN status = 'delivered' THEN retail_total_idr ELSE 0 END), 0) AS revenue
        FROM store_orders`,
    )
    .get() as { total: number; revenue: number };
  const openTickets = countOpenTickets();
  const users = getDb()
    .prepare(
      `SELECT u.tg_id AS tgId, u.first_name AS firstName, u.username AS username,
          COALESCE(w.balance_idr, 0) AS balanceIdr, u.created_at AS createdAt
        FROM users u LEFT JOIN wallets w ON w.tg_id = u.tg_id
        ORDER BY u.created_at DESC LIMIT 50`,
    )
    .all() as {
    tgId: number;
    firstName: string | null;
    username: string | null;
    balanceIdr: number;
    createdAt: string;
  }[];

  // Status awal QRIS GoPay untuk panel (string lengkap tidak dikirim ke client).
  const storedQris = getGopayStaticQris();
  let qrisStatus: QrisStatus = {
    configured: false,
    merchant_name: null,
    merchant_city: null,
    merchant_id: null,
  };
  if (storedQris) {
    try {
      const info = extractMerchantInfo(storedQris);
      qrisStatus = {
        configured: true,
        merchant_name: info.merchantName,
        merchant_city: info.merchantCity,
        merchant_id: info.merchantId,
      };
    } catch {
      qrisStatus = {
        configured: true,
        merchant_name: null,
        merchant_city: null,
        merchant_id: null,
      };
    }
  }

  return (
    <>
      <AdminNav active="/admin" />
      <h1>Admin — Unicodes Digital</h1>

      <h2>Dashboard</h2>
      <ul>
        <li>
          👥 Total pengguna: <strong>{store.totalUsers}</strong>
        </li>
        <li>
          📦 Total order: <strong>{orderTotals.total}</strong> · ✅ Terkirim:{" "}
          <strong>{store.deliveredOrders}</strong>
        </li>
        <li>
          💰 Revenue (order terkirim):{" "}
          <strong>{rupiah(orderTotals.revenue)}</strong>
        </li>
        <li>
          🎫 Tiket support terbuka: <strong>{openTickets}</strong>{" "}
          <a href="/admin/tickets" style={{ color: "#d9b45b" }}>
            Lihat tiket →
          </a>
        </li>
      </ul>

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

      <h2>Pembayaran — QRIS GoPay</h2>
      <QrisPanel initialStatus={qrisStatus} />

      <h2>Produk &amp; harga retail</h2>
      <ul>
        {products.map((p) => {
          const override = getPriceOverride(p.id);
          return (
            <li key={p.id} style={{ marginBottom: 10 }}>
              {p.name} — {rupiah(p.retailPriceIdr)} (via {p.supplier})
              {!p.active ? " [nonaktif]" : ""}
              {override !== null ? " [harga diubah dari admin]" : ""}
              <form
                action={`/api/admin/products/${p.id}/price`}
                method="post"
                style={{ display: "inline", marginLeft: 12 }}
              >
                <input
                  type="number"
                  name="price"
                  defaultValue={p.retailPriceIdr}
                  min={500}
                  max={10000000}
                  step={100}
                  required
                  style={{
                    width: 110,
                    padding: 6,
                    background: "#14092b",
                    border: "1px solid #6d28d9",
                    borderRadius: 6,
                    color: "#ede9fe",
                  }}
                />{" "}
                <button
                  type="submit"
                  style={{
                    background: "#7c3aed",
                    color: "#fff",
                    border: "none",
                    borderRadius: 6,
                    padding: "6px 12px",
                    cursor: "pointer",
                  }}
                >
                  Ubah harga
                </button>
              </form>
            </li>
          );
        })}
      </ul>
      <p style={{ color: "#b3a1e6", fontSize: 13 }}>
        Harga dasar ada di <code>src/lib/catalog.ts</code> — ubah di sini
        menyimpan override ke database (harga dasar tidak berubah).
      </p>

      <h2>Pengguna ({users.length})</h2>
      {users.length === 0 ? (
        <p>Belum ada pengguna.</p>
      ) : (
        <table cellPadding={6} style={{ borderCollapse: "collapse" }}>
          <thead>
            <tr>
              <th align="left">ID</th>
              <th align="left">Nama</th>
              <th align="left">Username</th>
              <th align="left">Saldo</th>
              <th align="left">Daftar</th>
            </tr>
          </thead>
          <tbody>
            {users.map((u) => (
              <tr key={u.tgId} style={{ borderTop: "1px solid #6d28d9" }}>
                <td>{u.tgId}</td>
                <td>{u.firstName ?? "—"}</td>
                <td>{u.username ? `@${u.username}` : "—"}</td>
                <td>{rupiah(u.balanceIdr)}</td>
                <td style={{ fontSize: 12, color: "#b3a1e6" }}>
                  {new Date(u.createdAt).toLocaleDateString("id-ID", {
                    timeZone: "Asia/Jakarta",
                  })}
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      )}

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
