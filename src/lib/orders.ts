// Order service — SATU alur order untuk web & bot Telegram.
//   awaiting_payment -> paid -> processing -> delivered
//                                    \-> failed -> (refund manual / coba supplier lain)
//
// Penyimpanan: SQLite lokal via node:sqlite (src/lib/db.ts, file dev.db).
// Produksi: ganti driver di db.ts ke Postgres — kode di sini tidak berubah.
//
// ATURAN PENTING (dari docs kedua supplier):
// - Satu order pembeli = SATU idempotency key = ID order internal (order.id).
//   Key ini dipakai ulang sebagai shopOrderId/clientOrderId (LahaStore) dan
//   ref_id (Sondev). Retry aman, tidak akan ke-charge 2x.
// - Jangan pernah buat order supplier baru kalau status masih unknown /
//   timeout — cek dulu status order lamanya.
// - Poll status tiap 5–10 detik; berhenti saat success / failed / cancelled.
import type { SQLInputValue } from "node:sqlite";
import { getCatalogProduct } from "./catalog";
import { getDb } from "./db";
import { getSupplier } from "./suppliers";
import { LahaStoreAdapter } from "./suppliers/lahastore";
import { IDR_PER_USD, VND_PER_USD } from "./pricing";
import type {
  SupplierId,
  SupplierOrderResult,
} from "./suppliers/types";

export type StoreOrderStatus =
  | "awaiting_payment"
  | "paid"
  | "processing"
  | "delivered"
  | "failed"
  | "refunded";

export interface StoreOrder {
  id: string;
  catalogProductId: string;
  quantity: number;
  retailTotalIdr: number;
  status: StoreOrderStatus;
  supplier: SupplierId;
  supplierOrderId: string | null;
  deliveredItems: string[];
  resultLink: string | null;
  error: string | null;
  /** ID Telegram pembeli (bot) atau "web". */
  buyerRef: string | null;
  /** Khusus Xiaomi: akun pembeli "email|password|2FA" (1 baris per akun). */
  xiaomiAccounts?: string[];
  /** Khusus SheerID: URL verifikasi dari pembeli. */
  sheeridUrl?: string | null;
  /** Target pesanan (Sondev, produk on-demand): kode invite / email target. */
  target?: string | null;
  /** Diskon promo yang diterapkan (Rp) — total = harga*qty - discount. */
  discountIdr: number;
  /** Kode promo yang dipakai (kalau ada). */
  promoCode?: string | null;
  /** Cara bayar: "qris" | "wallet" | null (belum dibayar). */
  paidVia?: string | null;
  createdAt: string;
  updatedAt: string;
}

interface OrderRow {
  id: string;
  catalog_product_id: string;
  quantity: number;
  retail_total_idr: number;
  status: string;
  supplier: string;
  supplier_order_id: string | null;
  delivered_items: string;
  result_link: string | null;
  error: string | null;
  buyer_ref: string | null;
  xiaomi_accounts: string | null;
  sheerid_url: string | null;
  target: string | null;
  discount_idr: number;
  promo_code: string | null;
  paid_via: string | null;
  created_at: string;
  updated_at: string;
}

function toStoreOrder(row: OrderRow): StoreOrder {
  return {
    id: row.id,
    catalogProductId: row.catalog_product_id,
    quantity: row.quantity,
    retailTotalIdr: row.retail_total_idr,
    status: row.status as StoreOrderStatus,
    supplier: row.supplier as SupplierId,
    supplierOrderId: row.supplier_order_id,
    deliveredItems: JSON.parse(row.delivered_items || "[]") as string[],
    resultLink: row.result_link,
    error: row.error,
    buyerRef: row.buyer_ref,
    xiaomiAccounts: row.xiaomi_accounts
      ? (JSON.parse(row.xiaomi_accounts) as string[])
      : undefined,
    sheeridUrl: row.sheerid_url,
    target: row.target,
    discountIdr: row.discount_idr ?? 0,
    promoCode: row.promo_code,
    paidVia: row.paid_via,
    createdAt: row.created_at,
    updatedAt: row.updated_at,
  };
}

function newId(): string {
  return `UD-${Date.now().toString(36)}-${Math.random()
    .toString(36)
    .slice(2, 8)}`.toUpperCase();
}

/** Buat ID order baru (dipakai bot untuk menukar promo SEBELUM order dibuat,
 *  supaya redemption tercatat dengan ID order yang benar). */
export function newOrderId(): string {
  return newId();
}

export async function createStoreOrder(input: {
  id?: string;
  catalogProductId: string;
  quantity?: number;
  buyerRef?: string | null;
  xiaomiAccounts?: string[];
  sheeridUrl?: string | null;
  target?: string | null;
  discountIdr?: number;
  promoCode?: string | null;
}): Promise<StoreOrder> {
  const product = getCatalogProduct(input.catalogProductId);
  const quantity = input.quantity ?? 1;
  const discountIdr = Math.max(0, Math.min(input.discountIdr ?? 0, product.retailPriceIdr * quantity));
  const now = new Date().toISOString();
  const order: StoreOrder = {
    id: input.id ?? newId(),
    catalogProductId: product.id,
    quantity,
    retailTotalIdr: Math.max(0, product.retailPriceIdr * quantity - discountIdr),
    status: "awaiting_payment",
    supplier: product.supplier,
    supplierOrderId: null,
    deliveredItems: [],
    resultLink: null,
    error: null,
    buyerRef: input.buyerRef ?? null,
    xiaomiAccounts: input.xiaomiAccounts,
    sheeridUrl: input.sheeridUrl ?? null,
    target: input.target ?? null,
    discountIdr,
    promoCode: input.promoCode ?? null,
    paidVia: null,
    createdAt: now,
    updatedAt: now,
  };
  getDb().prepare(
    `INSERT INTO store_orders
      (id, catalog_product_id, quantity, retail_total_idr, status, supplier,
       supplier_order_id, delivered_items, result_link, error, buyer_ref,
       xiaomi_accounts, sheerid_url, target, discount_idr, promo_code, paid_via,
       created_at, updated_at)
     VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)`,
  ).run(
    order.id,
    order.catalogProductId,
    order.quantity,
    order.retailTotalIdr,
    order.status,
    order.supplier,
    order.supplierOrderId,
    JSON.stringify(order.deliveredItems),
    order.resultLink,
    order.error,
    order.buyerRef,
    order.xiaomiAccounts ? JSON.stringify(order.xiaomiAccounts) : null,
    order.sheeridUrl ?? null,
    order.target ?? null,
    order.discountIdr,
    order.promoCode ?? null,
    order.paidVia ?? null,
    order.createdAt,
    order.updatedAt,
  );
  return order;
}

export async function getStoreOrder(id: string): Promise<StoreOrder | null> {
  const row = getDb()
    .prepare("SELECT * FROM store_orders WHERE id = ?")
    .get(id) as OrderRow | undefined;
  return row ? toStoreOrder(row) : null;
}

export async function listStoreOrders(limit = 50): Promise<StoreOrder[]> {
  const rows = getDb()
    .prepare("SELECT * FROM store_orders ORDER BY created_at DESC LIMIT ?")
    .all(limit) as unknown as OrderRow[];
  return rows.map(toStoreOrder);
}

/** Order milik satu pembeli (bot: buyerRef `tg:<id>`), terbaru dulu. */
export async function listOrdersByBuyer(
  buyerRef: string,
  limit = 10,
): Promise<StoreOrder[]> {
  const rows = getDb()
    .prepare(
      "SELECT * FROM store_orders WHERE buyer_ref = ? ORDER BY created_at DESC LIMIT ?",
    )
    .all(buyerRef, limit) as unknown as OrderRow[];
  return rows.map(toStoreOrder);
}

/** Statistik pembeli untuk layar /start: jumlah order berbayar + total belanja. */
export async function getBuyerStats(
  buyerRef: string,
): Promise<{ orderCount: number; totalSpentIdr: number }> {
  const row = getDb()
    .prepare(
      `SELECT COUNT(*) AS n, COALESCE(SUM(retail_total_idr), 0) AS total
         FROM store_orders
        WHERE buyer_ref = ? AND status IN ('paid', 'processing', 'delivered')`,
    )
    .get(buyerRef) as { n: number; total: number };
  return { orderCount: row.n, totalSpentIdr: row.total };
}

async function updateOrder(
  id: string,
  data: Partial<StoreOrder>,
): Promise<StoreOrder> {
  const sets: string[] = ["updated_at = ?"];
  const values: SQLInputValue[] = [new Date().toISOString()];
  if (data.status) {
    sets.push("status = ?");
    values.push(data.status);
  }
  if (data.supplierOrderId !== undefined) {
    sets.push("supplier_order_id = ?");
    values.push(data.supplierOrderId);
  }
  if (data.deliveredItems) {
    sets.push("delivered_items = ?");
    values.push(JSON.stringify(data.deliveredItems));
  }
  if (data.resultLink !== undefined) {
    sets.push("result_link = ?");
    values.push(data.resultLink);
  }
  if (data.error !== undefined) {
    sets.push("error = ?");
    values.push(data.error);
  }
  if (data.target !== undefined) {
    sets.push("target = ?");
    values.push(data.target);
  }
  if (data.paidVia !== undefined) {
    sets.push("paid_via = ?");
    values.push(data.paidVia);
  }
  values.push(id);
  getDb().prepare(`UPDATE store_orders SET ${sets.join(", ")} WHERE id = ?`).run(
    ...values,
  );
  const updated = await getStoreOrder(id);
  if (!updated) throw new Error(`Order tidak ditemukan: ${id}`);
  return updated;
}

/** Isi/ubah target order (mis. dari bot Telegram setelah pembeli mengirimnya). */
export async function setOrderTarget(id: string, target: string): Promise<StoreOrder> {
  const order = await getStoreOrder(id);
  if (!order) throw new Error(`Order tidak ditemukan: ${id}`);
  return updateOrder(id, { target });
}

/** Catat cara bayar order (dipanggil sebelum markPaid dari jalur bot / poller). */
export async function setOrderPaidVia(
  id: string,
  paidVia: "qris" | "wallet" | "midtrans",
): Promise<StoreOrder> {
  return updateOrder(id, { paidVia });
}

/** Dipanggil poller QRIS setelah bayar lunas (feed GoBiz cocok). */
export async function markPaid(id: string): Promise<StoreOrder> {
  const order = await getStoreOrder(id);
  if (!order) throw new Error(`Order tidak ditemukan: ${id}`);
  if (order.status !== "awaiting_payment") return order; // idempotent
  return updateOrder(id, { status: "paid" });
}

/** Batalkan order yang QR-nya kedaluwarsa sebelum lunas (dipanggil poller).
 *  Hanya dari status awaiting_payment — order yang sudah dibayar tidak
 *  disentuh. Status akhir "failed" dengan alasan jelas supaya pembeli
 *  tahu harus buat order baru (bukan error supplier). */
export async function cancelOrderForExpiredPayment(
  id: string,
  reason: string,
): Promise<StoreOrder> {
  const order = await getStoreOrder(id);
  if (!order) throw new Error(`Order tidak ditemukan: ${id}`);
  if (order.status !== "awaiting_payment") return order; // sudah dibayar — jangan batalkan
  return updateOrder(id, { status: "failed", error: reason });
}

/** Dipanggil webhook Sondev saat order selesai (order.completed / DELIVERED).
 *  Idempotent: hanya mengubah order yang masih processing/paid. */
export async function markOrderDelivered(
  id: string,
  deliveredItems: string[],
  resultLink: string | null,
): Promise<StoreOrder> {
  const order = await getStoreOrder(id);
  if (!order) throw new Error(`Order tidak ditemukan: ${id}`);
  if (order.status === "delivered") return order;
  if (order.status !== "processing" && order.status !== "paid") return order;
  return updateOrder(id, {
    status: "delivered",
    deliveredItems,
    resultLink: resultLink ?? order.resultLink,
  });
}

/** Dipanggil webhook Sondev saat order gagal (order.failed / FAILED / REFUNDED).
 *  Idempotent: hanya mengubah order yang masih processing/paid. */
export async function markOrderFailed(id: string, error: string): Promise<StoreOrder> {
  const order = await getStoreOrder(id);
  if (!order) throw new Error(`Order tidak ditemukan: ${id}`);
  if (order.status === "failed" || order.status === "delivered") return order;
  if (order.status !== "processing" && order.status !== "paid") return order;
  return updateOrder(id, { status: "failed", error });
}

const sleep = (ms: number) => new Promise((r) => setTimeout(r, ms));

function isTerminal(status: string): boolean {
  return ["success", "failed", "cancelled"].includes(status);
}

/** Cek apakah saldo supplier cukup untuk meng-cover harga grosir produk x qty.
 *  Dipakai SEBELUM pembeli bayar (POST /api/orders) dan di awal fulfillOrder.
 *  FAIL OPEN: kalau API supplier tidak bisa dicek (mis. Cloudflare 403 dari
 *  VM ini) atau harga grosir tidak diketahui, order tetap jalan seperti dulu
 *  — jangan blokir pembeli karena cek yang gagal. Hanya tolak kalau SUDAH
 *  PASTI saldo tidak cukup (saldo + harga grosir sama-sama terbaca). */
export async function checkSupplierFunds(
  catalogProductId: string,
  quantity: number,
): Promise<{ ok: boolean; reason?: string }> {
  const product = getCatalogProduct(catalogProductId);
  const supplier = getSupplier(product.supplier);
  try {
    const [balance, products] = await Promise.all([
      supplier.getBalance(),
      supplier.listProducts(),
    ]);
    const wholesale = products.find(
      (p) => p.code === product.supplierProductCode,
    );
    if (!wholesale) return { ok: true }; // produk tidak ada di katalog live — jangan blokir
    const unitIdr =
      wholesale.wholesalePriceIdr ??
      (wholesale.wholesalePriceUsd != null
        ? wholesale.wholesalePriceUsd * IDR_PER_USD
        : null);
    if (unitIdr == null) return { ok: true }; // harga grosir tidak diketahui — jangan blokir
    const vnd =
      typeof (balance.raw as { vnd?: unknown } | null)?.vnd === "number"
        ? ((balance.raw as { vnd: number }).vnd * IDR_PER_USD) / VND_PER_USD
        : 0;
    const fundsIdr = balance.idr + balance.usd * IDR_PER_USD + vnd;
    const needIdr = unitIdr * quantity;
    if (fundsIdr < needIdr) {
      return {
        ok: false,
        reason:
          "Produk ini sedang tidak bisa dibeli — stok supplier belum tersedia. Coba lagi nanti.",
      };
    }
    return { ok: true };
  } catch {
    return { ok: true }; // cek gagal — fail open
  }
}

/** Tandai order gagal. Refund tergantung cara bayar:
 *  - Dompet (paid_via = "wallet") -> saldo dikembalikan ke dompet otomatis,
 *    status jadi "refunded".
 *  - QRIS (paid_via = "qris") -> TIDAK ADA refund otomatis (QRIS tidak
 *    punya API refund). Status tetap "failed" + catatan refund MANUAL +
 *    notifikasi ke admin supaya uangnya ditransfer balik manual.
 *  Tidak pernah throw — kegagalan refund/notifikasi tidak boleh menutupi
 *  status gagal ordernya. */
async function failOrder(
  orderId: string,
  error: string,
  deliveredItems: string[] = [],
  resultLink: string | null = null,
): Promise<StoreOrder> {
  const failed = await updateOrder(orderId, {
    status: "failed",
    deliveredItems,
    resultLink,
    error,
  });
  // Order dompet -> refund ke dompet (bukan QRIS).
  if (failed.paidVia === "wallet") {
    try {
      const { refundWalletForOrder, tgIdFromBuyerRef } = await import("./wallet");
      const tgId = tgIdFromBuyerRef(failed.buyerRef);
      if (tgId != null && failed.retailTotalIdr > 0) {
        await refundWalletForOrder({
          tgId,
          amountIdr: failed.retailTotalIdr,
          orderId: failed.id,
        });
        return updateOrder(failed.id, {
          status: "refunded",
          error: `${error} — saldo sudah dikembalikan ke dompet kamu.`,
        });
      }
    } catch (refundErr) {
      console.error(
        `[orders] refund dompet ${failed.id} gagal:`,
        refundErr instanceof Error ? refundErr.message : String(refundErr),
      );
    }
    return updateOrder(failed.id, {
      error: `${error} — refund ke dompet gagal, hubungi admin.`,
    });
  }
  // Order QRIS -> refund MANUAL oleh admin (transfer balik ke pembeli).
  try {
    const { notifyAdmin } = await import("./notify");
    const total = `Rp${failed.retailTotalIdr.toLocaleString("id-ID")}`;
    void notifyAdmin(
      `⚠️ <b>Refund manual dibutuhkan</b>\n\nOrder ${failed.id} (${failed.catalogProductId} × ${failed.quantity}, ${total}) gagal setelah dibayar via QRIS.\nPembeli: ${failed.buyerRef ?? "web"}\nAlasan: ${error}\n\nQRIS tidak punya refund otomatis — transfer balik ${total} ke pembeli secara manual, lalu kabari pembeli.`,
    );
  } catch (notifyErr) {
    console.error(
      `[orders] notifikasi refund manual ${failed.id} gagal:`,
      notifyErr instanceof Error ? notifyErr.message : String(notifyErr),
    );
  }
  return updateOrder(failed.id, {
    error: `${error} — refund manual: admin akan transfer balik ke kamu, hubungi support kalau belum masuk.`,
  });
}

/** Jalankan fulfillment ke supplier. Hanya untuk order yang sudah "paid".
 *  Polling bisa makan waktu sampai ~5 menit — di produksi jalankan dari
 *  worker/queue atau tanpa await dari webhook, JANGAN tahan request HTTP. */
export async function fulfillOrder(orderId: string): Promise<StoreOrder> {
  const order = await getStoreOrder(orderId);
  if (!order) throw new Error(`Order tidak ditemukan: ${orderId}`);
  if (order.status !== "paid") {
    throw new Error(
      `Order ${orderId} belum dibayar (status: ${order.status})`,
    );
  }
  const product = getCatalogProduct(order.catalogProductId);
  const supplier = getSupplier(product.supplier);
  await updateOrder(order.id, { status: "processing" });

  try {
    // 1) Cek saldo supplier sebelum belanja. Kalau SUDAH PASTI tidak cukup,
    //    lempar error -> order gagal -> refund otomatis (lihat failOrder).
    //    Kalau ceknya gagal (API tidak bisa dihubungi), fail open: lanjut.
    const funds = await checkSupplierFunds(product.id, order.quantity);
    if (!funds.ok) {
      throw new Error(funds.reason ?? "Saldo supplier tidak cukup");
    }

    // 2) Buat order ke supplier — idempotency key = ID order internal.
    const deadline = Date.now() + 5 * 60_000;
    let results: SupplierOrderResult[] = [];

    if (product.fulfillment === "lahastore-xiaomi3") {
      if (!(supplier instanceof LahaStoreAdapter)) {
        throw new Error("Produk Xiaomi harus lewat LahaStore");
      }
      if (!order.xiaomiAccounts?.length) {
        throw new Error(
          "Order Xiaomi butuh akun pembeli (email|password|2FA)",
        );
      }
      const batch = await supplier.createXiaomiOrder({
        clientOrderId: order.id,
        currency: "vnd",
        accounts: order.xiaomiAccounts,
      });
      results = batch.orders;
      await updateOrder(order.id, {
        supplierOrderId: results[0]?.supplierOrderId ?? null,
      });
      // Poll tiap order dalam batch (1 akun = 1 order supplier).
      while (
        results.some((r) => !isTerminal(r.status)) &&
        Date.now() < deadline
      ) {
        await sleep(8000);
        results = await Promise.all(
          results.map((r) =>
            supplier.getXiaomiOrder(Number(r.supplierOrderId)),
          ),
        );
      }
    } else if (product.fulfillment === "lahastore-sheerid") {
      if (!(supplier instanceof LahaStoreAdapter)) {
        throw new Error("Produk SheerID harus lewat LahaStore");
      }
      if (!order.sheeridUrl) {
        throw new Error("Order SheerID butuh URL verifikasi dari pembeli");
      }
      const job = await supplier.createSheeridVerification({
        sheeridUrl: order.sheeridUrl,
        clientOrderId: order.id,
      });
      await updateOrder(order.id, { supplierOrderId: job.jobId });
      let current = { status: job.status, resultLink: null as string | null };
      while (!isTerminal(current.status) && Date.now() < deadline) {
        await sleep(8000);
        current = await supplier.getSheeridJob(job.jobId);
      }
      results = [
        {
          supplierOrderId: job.jobId,
          status:
            current.status === "success"
              ? "success"
              : current.status === "failed"
                ? "failed"
                : "unknown",
          deliveredItems: [],
          resultLink: current.resultLink,
          errorCode: null,
          errorMessage: null,
          raw: current,
        },
      ];
    } else {
      const created = await supplier.createOrder({
        idempotencyKey: order.id,
        supplierProductCode: product.supplierProductCode,
        quantity: order.quantity,
        // Sondev: target WAJIB untuk produk on-demand (docs v1.2).
        target: order.target ?? undefined,
      });
      await updateOrder(order.id, {
        supplierOrderId: created.supplierOrderId,
      });
      let current = created;
      // Polling ladder Sondev (docs v1.2): 5s → 10s → 20s → 30s, berhenti 10 menit.
      const ladder = [5000, 10000, 20000, 30000];
      let step = 0;
      const pollDeadline = Date.now() + 10 * 60_000;
      while (!isTerminal(current.status) && Date.now() < pollDeadline) {
        await sleep(ladder[Math.min(step, ladder.length - 1)]);
        step += 1;
        current = await supplier.getOrder(current.supplierOrderId);
      }
      results = [current];
    }

    // 3) Simpan hasil & tentukan status akhir.
    const deliveredItems = results.flatMap((r) => r.deliveredItems);
    const resultLink =
      results.map((r) => r.resultLink).find(Boolean) ?? null;
    const failed = results.find(
      (r) => r.status === "failed" || r.status === "cancelled",
    );
    const allSuccess =
      results.length > 0 && results.every((r) => r.status === "success");
    if (allSuccess) {
      return updateOrder(order.id, {
        status: "delivered",
        deliveredItems,
        resultLink,
      });
    }
    if (failed) {
      return failOrder(
        order.id,
        failed.errorMessage ??
          failed.errorCode ??
          "Supplier gagal memproses order",
        deliveredItems,
        resultLink,
      );
    }
    return failOrder(
      order.id,
      "Timeout menunggu supplier — admin akan memproses refund manual, hubungi support kalau belum ada kabar.",
      deliveredItems,
      resultLink,
    );
  } catch (err) {
    return failOrder(
      order.id,
      err instanceof Error ? err.message : String(err),
    );
  }
}
