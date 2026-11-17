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
    createdAt: row.created_at,
    updatedAt: row.updated_at,
  };
}

function newId(): string {
  return `UD-${Date.now().toString(36)}-${Math.random()
    .toString(36)
    .slice(2, 8)}`.toUpperCase();
}

export async function createStoreOrder(input: {
  catalogProductId: string;
  quantity?: number;
  buyerRef?: string | null;
  xiaomiAccounts?: string[];
  sheeridUrl?: string | null;
  target?: string | null;
}): Promise<StoreOrder> {
  const product = getCatalogProduct(input.catalogProductId);
  const quantity = input.quantity ?? 1;
  const now = new Date().toISOString();
  const order: StoreOrder = {
    id: newId(),
    catalogProductId: product.id,
    quantity,
    retailTotalIdr: product.retailPriceIdr * quantity,
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
    createdAt: now,
    updatedAt: now,
  };
  getDb().prepare(
    `INSERT INTO store_orders
      (id, catalog_product_id, quantity, retail_total_idr, status, supplier,
       supplier_order_id, delivered_items, result_link, error, buyer_ref,
       xiaomi_accounts, sheerid_url, target, created_at, updated_at)
     VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)`,
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

/** Dipanggil webhook Midtrans setelah bayar lunas (settlement/capture). */
export async function markPaid(id: string): Promise<StoreOrder> {
  const order = await getStoreOrder(id);
  if (!order) throw new Error(`Order tidak ditemukan: ${id}`);
  if (order.status !== "awaiting_payment") return order; // idempotent
  return updateOrder(id, { status: "paid" });
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
    // 1) Cek saldo supplier sebelum belanja (gagal di sini = jangan lanjut).
    // TODO: bandingkan dengan harga grosir produk x qty via supplier.listProducts().
    await supplier.getBalance();

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
      return updateOrder(order.id, {
        status: "failed",
        deliveredItems,
        resultLink,
        error:
          failed.errorMessage ??
          failed.errorCode ??
          "Supplier gagal memproses order",
      });
    }
    return updateOrder(order.id, {
      status: "failed",
      deliveredItems,
      resultLink,
      error:
        "Timeout menunggu supplier — cek manual di admin. Jangan buat order baru (pakai idempotency key yang sama).",
    });
  } catch (err) {
    return updateOrder(order.id, {
      status: "failed",
      error: err instanceof Error ? err.message : String(err),
    });
  }
}
