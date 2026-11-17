// Adapter LahaStore — buat AI lain (Gemini dkk).
// Docs: https://lahastore.up.railway.app/api/v1/docs
// Auth: header X-API-Key di semua request. Key harus diaktifkan admin dulu.
// Semua pemanggilan di sini HANYA dari server (API route / server lib) —
// jangan pernah panggil dari kode client/browser.
import type {
  CreateOrderInput,
  SupplierAdapter,
  SupplierBalance,
  SupplierId,
  SupplierOrderResult,
  SupplierOrderStatus,
  SupplierProduct,
} from "./types";

const BASE_URL =
  process.env.LAHASTORE_BASE_URL ?? "https://lahastore.up.railway.app/api/v1";

function apiKey(): string {
  const key = process.env.LAHASTORE_API_KEY;
  if (!key) throw new Error("LAHASTORE_API_KEY belum diisi di .env.local");
  return key;
}

async function call<T>(path: string, init?: RequestInit): Promise<T> {
  const res = await fetch(`${BASE_URL}${path}`, {
    ...init,
    headers: {
      "X-API-Key": apiKey(),
      "Content-Type": "application/json",
      ...(init?.headers ?? {}),
    },
    cache: "no-store",
  });
  const body = (await res.json().catch(() => ({}))) as Record<string, unknown>;
  if (!res.ok || body.ok === false) {
    const code = typeof body.code === "string" ? body.code : `HTTP_${res.status}`;
    const msg =
      typeof body.error === "string" ? body.error : "LahaStore request gagal";
    throw new Error(`${code}: ${msg}`);
  }
  return body as T;
}

function mapStatus(status: unknown): SupplierOrderStatus {
  switch (status) {
    case "completed":
    case "success":
      return "success";
    case "failed":
      return "failed";
    case "cancelled":
      return "cancelled";
    case "pending":
    case "processing":
    case "running":
    case "submitting":
      return "processing";
    default:
      return "unknown";
  }
}

function mapOrder(order: Record<string, unknown>): SupplierOrderResult {
  const accounts = Array.isArray(order.accounts)
    ? (order.accounts as unknown[]).map(String)
    : [];
  return {
    supplierOrderId: String(order.orderCode ?? order.id ?? ""),
    status: mapStatus(order.status),
    deliveredItems: accounts,
    resultLink: null,
    errorCode: null,
    errorMessage: null,
    raw: order,
  };
}

export class LahaStoreAdapter implements SupplierAdapter {
  readonly id: SupplierId = "lahastore";

  async getBalance(): Promise<SupplierBalance> {
    const body = await call<{
      wallet?: { vnd?: number; usd?: number; credit?: number };
    }>("/balance");
    const w = body.wallet ?? {};
    return {
      idr: 0, // LahaStore tidak memberi saldo IDR langsung; VND dikonversi di pricing.ts
      usd: w.usd ?? 0,
      credit: w.credit ?? 0,
      raw: { vnd: w.vnd ?? 0, ...body },
    };
  }

  async listProducts(): Promise<SupplierProduct[]> {
    const body = await call<{ products?: Record<string, unknown>[] }>(
      "/products?page=1&limit=50",
    );
    return (body.products ?? []).map((p) => ({
      code: String(p.id ?? p.code ?? ""),
      name: String(p.name ?? p.code ?? ""),
      wholesalePriceIdr: null, // harga di LahaStore umumnya VND — konversi di pricing.ts
      wholesalePriceUsd:
        typeof p.priceUsd === "number" ? p.priceUsd : null,
      stock: typeof p.stock === "number" ? p.stock : null,
    }));
  }

  async getStock(supplierProductCode: string): Promise<number | null> {
    const products = await this.listProducts();
    return products.find((p) => p.code === supplierProductCode)?.stock ?? null;
  }

  /** Order produk shop biasa (bukan Gemini Xiaomi / SheerID — lihat di bawah). */
  async createOrder(input: CreateOrderInput): Promise<SupplierOrderResult> {
    const body = await call<{ order?: Record<string, unknown> }>("/orders", {
      method: "POST",
      body: JSON.stringify({
        productId: input.supplierProductCode,
        qty: input.quantity,
        currency: "vnd",
        shopOrderId: input.idempotencyKey,
      }),
    });
    if (!body.order) throw new Error("LahaStore: respons /orders tanpa order");
    return mapOrder(body.order);
  }

  async getOrder(supplierOrderId: string): Promise<SupplierOrderResult> {
    const body = await call<{ order?: Record<string, unknown> }>(
      `/orders/${encodeURIComponent(supplierOrderId)}`,
    );
    if (!body.order) throw new Error("LahaStore: order tidak ditemukan");
    return mapOrder(body.order);
  }

  // ------------------------------------------------------------------
  // Produk Gemini — jalurnya BEDA dari order shop biasa (docs LahaStore).
  // Dipakai order service kalau catalog.product.fulfillment = "xiaomi3"
  // atau "sheerid". Harga cek dulu via /xiaomi3/services & /sheerid/service_info.
  // ------------------------------------------------------------------

  /** Gemini Pro Xiaomi 3 bulan. `accounts`: 1–200 baris "email|password|2FA-Base32-32-karakter
   *  atau kode cadangan 8 angka". Satu akun = satu order supplier. */
  async createXiaomiOrder(input: {
    clientOrderId: string;
    currency: "usd" | "vnd";
    accounts: string[];
  }): Promise<{ orders: SupplierOrderResult[]; duplicated: boolean }> {
    const body = await call<{
      duplicated?: boolean;
      orders?: Record<string, unknown>[];
    }>("/xiaomi3/orders", {
      method: "POST",
      body: JSON.stringify({
        taskType: "default",
        currency: input.currency,
        clientOrderId: input.clientOrderId,
        accounts: input.accounts,
      }),
    });
    return {
      duplicated: body.duplicated === true,
      orders: (body.orders ?? []).map((o) => ({
        supplierOrderId: String(o.id ?? ""),
        status: mapStatus(o.status),
        deliveredItems: [],
        resultLink:
          typeof o.resultLink === "string" ? o.resultLink : null,
        errorCode:
          typeof o.errorCode === "string" ? o.errorCode : null,
        errorMessage:
          typeof o.errorEn === "string" ? o.errorEn : null,
        raw: o,
      })),
    };
  }

  async getXiaomiOrder(id: number): Promise<SupplierOrderResult> {
    const body = await call<{ order?: Record<string, unknown> }>(
      `/xiaomi3/orders/${id}`,
    );
    if (!body.order) throw new Error("LahaStore: order Xiaomi tidak ditemukan");
    const o = body.order;
    return {
      supplierOrderId: String(o.id ?? ""),
      status: mapStatus(o.status),
      deliveredItems: [],
      resultLink: typeof o.resultLink === "string" ? o.resultLink : null,
      errorCode: typeof o.errorCode === "string" ? o.errorCode : null,
      errorMessage: typeof o.errorEn === "string" ? o.errorEn : null,
      raw: o,
    };
  }

  /** Cek progress + hasil job SheerID. Dipakai polling di fulfillOrder. */
  async getSheeridJob(
    jobId: string,
  ): Promise<{ status: string; resultLink: string | null; raw: unknown }> {
    const body = await call<{ job?: Record<string, unknown> }>(
      `/sheerid/jobs/${encodeURIComponent(jobId)}`,
    );
    if (!body.job) throw new Error("LahaStore: job SheerID tidak ditemukan");
    return {
      status: String(body.job.status ?? "pending"),
      resultLink:
        typeof body.job.result_link === "string" ? body.job.result_link : null,
      raw: body.job,
    };
  }

  /** Gemini Student Pro — buat job verifikasi SheerID. */
  async createSheeridVerification(input: {
    sheeridUrl: string;
    clientOrderId: string;
    discountCode?: string;
  }): Promise<{ jobId: string; status: string; raw: unknown }> {
    const body = await call<{ job?: Record<string, unknown> }>(
      "/sheerid/verify",
      {
        method: "POST",
        body: JSON.stringify({
          sheerid_url: input.sheeridUrl,
          client_order_id: input.clientOrderId,
          ...(input.discountCode
            ? { discount_code: input.discountCode }
            : {}),
        }),
      },
    );
    if (!body.job) throw new Error("LahaStore: respons /sheerid/verify tanpa job");
    return {
      jobId: String(body.job.job_id ?? ""),
      status: String(body.job.status ?? "pending"),
      raw: body.job,
    };
  }
}
