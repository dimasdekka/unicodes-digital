// Adapter Sondev — buat Muse.
// Docs v1.2: ~/workspace/user/files/SONDEV_RESELLER_API_EN.md
// (juga template resmi sondev_reseller_bot.py, Aiogram 3, API v1.1).
// Auth: API key + HMAC-SHA256. Setiap request kirim:
//   X-API-KEY, X-TIMESTAMP (detik Unix, ±300 detik dari jam server),
//   X-SIGNATURE = hex HMAC-SHA256(secret, timestamp + "\n" + METHOD + "\n" + path + "\n" + body)
//   - path = path LENGKAP termasuk "/api/v1" (mis. "/api/v1/profile"),
//     TANPA query string (query tidak ikut signature!)
//   - body JSON harus compact tanpa spasi (JSON.stringify sudah compact)
//   - body kosong untuk GET/DELETE
// Secret didapat dari bot Telegram Sondev (/reseller -> "Reveal Secret"), cuma tampil SEKALI.
// Semua pemanggilan di sini HANYA dari server — jangan pernah dari kode client/browser.
//
// Bentuk respons (envelope): sukses { status: "success", request_id, data: ... },
// gagal { status: "error", error, message, request_id, retry_after? }.
// Harga grosir = price_idr / price_usd di /products. Saldo = balance_idr / balance_usdt
// (flat) di /profile. Stok = available_stock ("UNLIMITED" atau angka) — untuk produk
// muse_service JANGAN jadikan patokan jumlah; pakai sellable_quantity / available_now
// dari /stock (keduanya bisa null kalau pool tidak terbaca — null = tidak diketahui,
// BUKAN 0).
import { createHmac } from "node:crypto";
import type {
  CreateOrderInput,
  SupplierAdapter,
  SupplierBalance,
  SupplierId,
  SupplierOrderResult,
  SupplierOrderStatus,
  SupplierProduct,
} from "./types";

import { sondevBaseUrl, sondevConfig } from "../config";

const BASE_URL = sondevBaseUrl();
// Path dasar dari base URL (mis. "/api/v1") — server menandatangani path
// LENGKAP ini ("/api/v1/profile"), bukan path relatif ("/profile").
// Bukti: pesan error server + template resmi (endpoint "/api/v1/orders/create"
// yang ditandatangani utuh). Dulu adapter menandatangani path relatif saja
// -> semua endpoint signed ditolak INVALID_SIGNATURE (ditemukan 2026-11-21).
let BASE_PATH = "";
try {
  BASE_PATH = new URL(BASE_URL).pathname.replace(/\/$/, "");
} catch {
  BASE_PATH = "";
}

function credentials(): { key: string; secret: string } {
  const { apiKey, apiSecret } = sondevConfig();
  return { key: apiKey, secret: apiSecret };
}

function sign(secret: string, timestamp: string, method: string, path: string, body: string): string {
  return createHmac("sha256", secret)
    .update(`${timestamp}\n${method}\n${path}\n${body}`)
    .digest("hex"); // lowercase hex, sesuai docs
}

const sleep = (ms: number) => new Promise((r) => setTimeout(r, ms));

/** Backoff eksponensial 2/4/8/16 detik dengan jitter ±30%, maks 30 detik (docs). */
function backoff(attempt: number): number {
  const base = Math.min(2 ** attempt, 30) * 1000;
  return base * (0.7 + Math.random() * 0.6);
}

/** Error bisnis — JANGAN di-retry (4xx kecuali 429 RATE_LIMIT_MINUTE & 409 REPLAY_DETECTED). */
class NonRetryable extends Error {}

async function call<T>(pathWithQuery: string, init?: RequestInit): Promise<T> {
  const { key, secret } = credentials();
  const method = (init?.method ?? "GET").toUpperCase();
  const body = typeof init?.body === "string" ? init.body : "";
  // PENTING (docs + template resmi): yang ditandatangani adalah path
  // LENGKAP ("/api/v1/..."), TANPA query string (query tidak ikut signature).
  const [signPathRel] = pathWithQuery.split("?");
  const signPath = `${BASE_PATH}${signPathRel}`;
  const maxAttempts = 4;
  let lastError: Error | null = null;

  for (let attempt = 1; attempt <= maxAttempts; attempt++) {
    // Timestamp + signature BARU tiap percobaan — pakai ulang = 409 REPLAY_DETECTED.
    const timestamp = Math.floor(Date.now() / 1000).toString();
    try {
      const res = await fetch(`${BASE_URL}${pathWithQuery}`, {
        ...init,
        headers: {
          "X-API-KEY": key,
          "X-TIMESTAMP": timestamp,
          "X-SIGNATURE": sign(secret, timestamp, method, signPath, body),
          "Content-Type": "application/json",
          Accept: "application/json",
          // User-Agent wajib diisi — tanpanya bisa kena block Cloudflare 1010.
          "User-Agent": "unicodes-digital/0.1",
          ...(init?.headers ?? {}),
        },
        cache: "no-store",
        signal: AbortSignal.timeout(25000),
      });
      const data = (await res.json().catch(() => ({}))) as Record<string, unknown>;
      if (res.ok) {
        // Semua respons dibungkus envelope { data: ... }.
        return (data.data ?? data) as T;
      }
      const code =
        typeof data.error === "string"
          ? data.error
          : typeof data.code === "string"
            ? data.code
            : `HTTP_${res.status}`;
      const msg =
        typeof data.message === "string" ? data.message : "Sondev request gagal";
      const requestId =
        typeof data.request_id === "string"
          ? data.request_id
          : (res.headers.get("X-Request-Id") ?? "");
      // Simpan request_id di pesan error — admin butuh ini untuk tracing.
      const err = new Error(
        requestId ? `${code}: ${msg} (req ${requestId})` : `${code}: ${msg}`,
      );
      // Retryable: 429 RATE_LIMIT_MINUTE, 409 REPLAY_DETECTED (dengan signature baru),
      // dan semua 5xx. TIDAK retry: DAILY_LIMIT, 402, 400, 401, 403, 409 SOLD_OUT.
      const retryable =
        res.status >= 500 ||
        code === "REPLAY_DETECTED" ||
        (res.status === 429 && code !== "DAILY_LIMIT");
      if (!retryable || attempt === maxAttempts) {
        throw new NonRetryable(err.message);
      }
      // Hormati Retry-After (header dulu, lalu field retry_after di body).
      const retryAfterHeader = Number(res.headers.get("Retry-After") ?? 0);
      const retryAfterBody = Number(data.retry_after ?? 0);
      const waitSec = Math.max(retryAfterHeader, retryAfterBody);
      await sleep(
        waitSec > 0 ? Math.min(waitSec, 30) * 1000 : backoff(attempt),
      );
      lastError = err;
    } catch (err) {
      if (err instanceof NonRetryable) throw err;
      // Network error / timeout: aman diulang (belum tentu sampai ke server).
      if (attempt === maxAttempts) {
        throw lastError ?? (err instanceof Error ? err : new Error(String(err)));
      }
      await sleep(backoff(attempt));
      lastError = err instanceof Error ? err : new Error(String(err));
    }
  }
  throw lastError ?? new Error("Sondev request gagal");
}

/** GET tanpa auth (mis. /status/public) — tidak pakai signature. */
async function publicCall<T>(path: string): Promise<T> {
  const res = await fetch(`${BASE_URL}${path}`, {
    headers: { Accept: "application/json", "User-Agent": "unicodes-digital/0.1" },
    cache: "no-store",
    signal: AbortSignal.timeout(10000),
  });
  const data = (await res.json().catch(() => ({}))) as Record<string, unknown>;
  return (data.data ?? data) as T;
}

function mapStatus(status: unknown): SupplierOrderStatus {
  switch (String(status ?? "").toUpperCase()) {
    case "COMPLETED":
    case "DELIVERED":
    case "SUCCESS":
      return "success";
    case "FAILED":
    case "REFUNDED":
    case "PARTIAL_DELIVERED":
      // REFUNDED = gagal + saldo grosir dikembalikan; PARTIAL_DELIVERED =
      // gagal sebagian — keduanya diperlakukan sebagai failed di sisi pembeli.
      return "failed";
    case "CANCELLED":
      return "cancelled";
    case "PROCESSING":
    case "PENDING":
      return "processing";
    default:
      return "unknown";
  }
}

/**
 * Ambil barang hasil fulfillment dari respons order.
 * Format resmi (docs): delivered_data.items = array string mentah —
 *   "email|mailbox_link", URL aktivasi mentah, "email:password", atau teks bebas;
 *   token/kode ada di delivered_data.code (+ redeemed).
 * Bentuk lama (delivered_items / items) tetap didukung sebagai fallback.
 */
function extractDelivered(order: Record<string, unknown>): {
  items: string[];
  resultLink: string | null;
} {
  const items: string[] = [];
  const deliveredData = (order.delivered_data ?? {}) as Record<string, unknown>;
  const rawItems = (deliveredData.items ?? order.delivered_items ?? order.items) as unknown;
  if (Array.isArray(rawItems)) {
    for (const it of rawItems) {
      if (typeof it === "string" && it.trim()) items.push(it);
      else if (it && typeof it === "object") items.push(JSON.stringify(it));
    }
  }
  if (typeof deliveredData.code === "string" && deliveredData.code.trim()) {
    items.push(deliveredData.code);
  }
  let resultLink =
    typeof order.result_link === "string" ? order.result_link : null;
  if (!resultLink) {
    // Cari URL pertama di dalam items (mis. mailbox_link dari "email|link").
    for (const it of items) {
      const m = it.match(/https?:\/\/\S+/);
      if (m) {
        resultLink = m[0];
        break;
      }
    }
  }
  return { items, resultLink };
}

function toOrderResult(
  body: Record<string, unknown>,
  fallbackId: string,
): SupplierOrderResult {
  const order = (body.order ?? body) as Record<string, unknown>;
  // ALREADY_EXISTS = ref_id sudah dipakai — body adalah order aslinya,
  // jadi baca hasil dari delivery_status-nya, bukan dari status itu sendiri.
  const rawStatus = String(order.status ?? "").toUpperCase();
  const effectiveStatus =
    rawStatus === "ALREADY_EXISTS"
      ? (order.delivery_status ?? order.status)
      : (order.status ?? order.delivery_status);
  const { items, resultLink } = extractDelivered(order);
  const errorMessage =
    typeof order.refund_note === "string" && order.refund_note
      ? order.refund_note
      : typeof order.message === "string"
        ? order.message
        : null;
  return {
    supplierOrderId: String(order.ref_id ?? order.id ?? fallbackId),
    status: mapStatus(effectiveStatus),
    deliveredItems: items,
    resultLink,
    errorCode:
      typeof body.code === "string"
        ? body.code
        : typeof body.error === "string"
          ? body.error
          : null,
    errorMessage,
    raw: body,
  };
}

export class SondevAdapter implements SupplierAdapter {
  readonly id: SupplierId = "sondev";
  private productsCache: { at: number; items: SupplierProduct[] } | null = null;

  async getBalance(): Promise<SupplierBalance> {
    // Docs: /profile mengembalikan balance_idr / balance_usdt FLAT (bukan nested).
    const body = await call<{
      balance_idr?: number;
      balance_usdt?: number;
      is_reseller?: boolean;
      webhook_healthy?: boolean;
      orders_today?: number;
      daily_order_limit?: number;
      rate_limit_per_min?: number;
      allowed_products?: string[] | null;
    }>("/profile");
    return {
      idr: body.balance_idr ?? 0,
      usd: body.balance_usdt ?? 0, // USDT dipakai dulu baru IDR (docs) — simpan di kolom usd
      credit: 0,
      raw: body,
    };
  }

  /** Probe publik (tanpa auth): apakah Sondev sedang menerima order.
   *  Mengembalikan null kalau respons tidak bisa dibaca (mis. Cloudflare
   *  block) — pemanggil JANGAN menganggap toko tutup dari hasil null. */
  async getPublicStatus(): Promise<{ acceptingOrders: boolean | null; raw: unknown }> {
    const body = await publicCall<{
      accepting_orders?: boolean;
      maintenance?: { active?: boolean };
    }>("/status/public");
    const hasSignal =
      typeof body.accepting_orders === "boolean" ||
      typeof body.maintenance?.active === "boolean";
    return {
      acceptingOrders: hasSignal
        ? (body.accepting_orders ?? !(body.maintenance?.active ?? false))
        : null,
      raw: body,
    };
  }

  async listProducts(): Promise<SupplierProduct[]> {
    // Docs: cache katalog 60–300 detik di sisi kita.
    if (this.productsCache && Date.now() - this.productsCache.at < 120_000) {
      return this.productsCache.items;
    }
    const body = await call<
      | { products?: Record<string, unknown>[] }
      | Record<string, unknown>[]
    >("/products");
    const arr = Array.isArray(body) ? body : (body.products ?? []);
    const items = arr.map((p) => {
      // available_stock = "UNLIMITED" (on-demand, mis. Muse) atau angka.
      const stockRaw = p.available_stock;
      const stock =
        typeof stockRaw === "number"
          ? stockRaw
          : typeof p.stock === "number"
            ? p.stock
            : null;
      return {
        code: String(p.code ?? p.product_code ?? ""),
        name: String(p.name ?? p.code ?? p.product_code ?? ""),
        wholesalePriceIdr:
          typeof p.price_idr === "number" ? p.price_idr : null,
        wholesalePriceUsd:
          typeof p.price_usd === "number" ? p.price_usd : null,
        stock,
        minQty: typeof p.min_qty === "number" ? p.min_qty : null,
        maxQty: typeof p.max_qty === "number" ? p.max_qty : null,
      };
    });
    this.productsCache = { at: Date.now(), items };
    return items;
  }

  async getStock(supplierProductCode: string): Promise<number | null> {
    // Docs: /stock?code=... — param-nya "code" (1–50 karakter a-z0-9_).
    // Jangan jadikan patokan available_stock untuk muse_service (string);
    // pakai sellable_quantity (= min(available_now, max_qty)) atau available_now.
    // available_now BISA null kalau pool tidak terbaca — null = tidak diketahui, BUKAN 0.
    const code = supplierProductCode.replace(/[^a-z0-9_]/g, "").slice(0, 50);
    const body = await call<Record<string, unknown>[] | Record<string, unknown>>(
      `/stock?code=${encodeURIComponent(code)}`,
    );
    const arr = Array.isArray(body) ? body : (body.data as Record<string, unknown>[] | undefined) ?? [];
    const found =
      arr.find((p) => String(p.code ?? "") === code) ?? arr[0];
    if (!found) return null;
    const sellable = found.sellable_quantity;
    if (typeof sellable === "number") return sellable;
    const now = found.available_now;
    if (typeof now === "number") return now;
    // available_stock angka = stok akun; "UNLIMITED" = tak terbatas.
    if (typeof found.available_stock === "number") return found.available_stock;
    return null;
  }

  async createOrder(input: CreateOrderInput): Promise<SupplierOrderResult> {
    const payload: Record<string, unknown> = {
      ref_id: input.idempotencyKey, // 1–100 karakter, unik per reseller
      product_code: input.supplierProductCode,
      quantity: input.quantity,
    };
    // target WAJIB untuk produk on-demand (invite code / email target) —
    // tanpa ini order ditolak 400 INVALID_ORDER.
    if (input.target) payload.target = input.target;
    const body = await call<Record<string, unknown>>("/orders/create", {
      method: "POST",
      body: JSON.stringify(payload),
    });
    // Reuse ref_id -> supplier kembalikan order asli + ALREADY_EXISTS, tanpa charge ulang.
    return toOrderResult(body, input.idempotencyKey);
  }

  async getOrder(supplierOrderId: string): Promise<SupplierOrderResult> {
    const body = await call<Record<string, unknown>>(
      `/orders/${encodeURIComponent(supplierOrderId)}`,
    );
    return toOrderResult(body, supplierOrderId);
  }

  /** Daftar order API terbaru — untuk rekonsiliasi harian (docs). */
  async listOrders(limit = 20): Promise<Record<string, unknown>[]> {
    const body = await call<Record<string, unknown>[] | Record<string, unknown>>(
      `/orders?limit=${Math.min(Math.max(limit, 1), 100)}`,
    );
    return Array.isArray(body) ? body : [];
  }

  /** Log request + ringkasan error 24 jam — debugging tercepat (docs). */
  async getLogs(limit = 30): Promise<Record<string, unknown>> {
    return call<Record<string, unknown>>(`/logs?limit=${limit}`);
  }

  /** Ping webhook (sinkron) — panggil setelah mengubah webhook URL. */
  async testWebhook(url?: string): Promise<Record<string, unknown>> {
    return call<Record<string, unknown>>("/webhook/test", {
      method: "POST",
      body: JSON.stringify(url ? { webhook_url: url } : {}),
    });
  }

  /** Webhook yang gagal permanen (dead letter). */
  async getDeadLetters(): Promise<Record<string, unknown>> {
    return call<Record<string, unknown>>("/webhook/dead-letters");
  }

  /** Masukkan semua dead letter kembali ke antrean. */
  async replayWebhooks(): Promise<Record<string, unknown>> {
    return call<Record<string, unknown>>("/webhook/replay", {
      method: "POST",
      body: JSON.stringify({}),
    });
  }
}
