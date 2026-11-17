// Interface yang WAJIB dipenuhi semua supplier.
// Mau tambah store ke-3, ke-4, dst: cukup bikin 1 file adapter baru yang
// implement interface ini + daftarkan di src/lib/suppliers/index.ts.
// Tidak ada kode lain (katalog, order, checkout) yang perlu diubah.

export type SupplierId = "lahastore" | "sondev";

export interface SupplierBalance {
  // Saldo yang bisa dipakai belanja, dalam IDR kalau bisa dihitung.
  // Kalau supplier pakai mata uang lain, konversi di adapter.
  idr: number;
  usd: number;
  credit: number;
  raw: unknown;
}

export interface SupplierProduct {
  code: string; // product_code (Sondev) / productId atau code (LahaStore)
  name: string;
  wholesalePriceIdr: number | null;
  wholesalePriceUsd: number | null;
  stock: number | null; // null = tidak diketahui / tidak terbatas
  /** Batas jumlah per order (dari /products atau /stock). */
  minQty?: number | null;
  maxQty?: number | null;
}

export type SupplierOrderStatus =
  | "pending"
  | "processing"
  | "success"
  | "failed"
  | "cancelled"
  | "unknown";

export interface SupplierOrderResult {
  supplierOrderId: string;
  status: SupplierOrderStatus;
  /** Barang hasil fulfillment: akun, kode, link, dll (format bebas per supplier). */
  deliveredItems: string[];
  resultLink: string | null;
  errorCode: string | null;
  errorMessage: string | null;
  raw: unknown;
}

export interface CreateOrderInput {
  /** Kunci idempotency — pakai ID order internal kita (unik per order pembeli).
   *  Kirim ulang dengan key yang sama = aman, supplier tidak men-charge 2x. */
  idempotencyKey: string;
  supplierProductCode: string;
  quantity: number;
  /** Opsional — target pesanan (mis. username/email) kalau produk membutuhkannya. */
  target?: string;
}

export interface SupplierAdapter {
  readonly id: SupplierId;
  /** Saldo dompet supplier saat ini. */
  getBalance(): Promise<SupplierBalance>;
  /** Katalog produk supplier beserta harga grosir. */
  listProducts(): Promise<SupplierProduct[]>;
  /** Stok real-time satu produk (null kalau tidak diketahui). */
  getStock(supplierProductCode: string): Promise<number | null>;
  /** Buat order ke supplier. */
  createOrder(input: CreateOrderInput): Promise<SupplierOrderResult>;
  /** Cek status order supplier. */
  getOrder(supplierOrderId: string): Promise<SupplierOrderResult>;
}
