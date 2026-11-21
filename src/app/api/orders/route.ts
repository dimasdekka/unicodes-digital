import { NextResponse } from "next/server";
import {
  checkSupplierFunds,
  createStoreOrder,
  fulfillOrder,
  markPaid,
  setOrderPaidVia,
} from "@/lib/orders";
import { getCatalogProduct } from "@/lib/catalog";
import {
  createMidtransPayment,
  createSnapTransaction,
} from "@/lib/payments/midtrans";
import { validateInitData } from "@/lib/telegram-webapp";
import { telegramBotToken } from "@/lib/config";

/**
 * POST /api/orders — buat order + pembayaran Midtrans Snap (QRIS).
 * Body JSON: { productId, quantity?, buyerRef?, xiaomiAccounts?, sheeridUrl? }
 * Balasan: { ok, order: { id, status, totalIdr }, payment: {
 *   redirectUrl, snapToken, expiresAt, reference } | null }
 * Pembeli membuka redirectUrl (halaman bayar Midtrans, QRIS tampil di sana).
 * Setelah lunas, poller status Midtrans (/lib/payments/midtrans-poller.ts)
 * atau webhook (/api/webhooks/midtrans) akan memanggil markPaid ->
 * fulfillOrder otomatis. Klien (web / /tg) memantau status order lewat
 * GET /api/orders/[id] sampai delivered/failed.
 * payment null = order gratis (total 0) — langsung diproses.
 *
 * Telegram Web App: kirim juga { initData } (dari Telegram.WebApp.initData).
 * Kalau initData ada, server memvalidasinya (HMAC bot token) dan buyerRef
 * jadi `tg:<telegram_id>` user itu — initData palsu ditolak 401. Tanpa
 * initData, alur web biasa tetap jalan seperti dulu.
 */
export async function POST(req: Request) {
  try {
    const body = (await req.json().catch(() => ({}))) as Record<string, unknown>;
    const productId = String(body.productId ?? "");
    const quantity =
      typeof body.quantity === "number" && body.quantity > 0
        ? Math.floor(body.quantity)
        : 1;
    // Jumlah custom tidak boleh lebih dari 30 (batas penebusan Muse per kode).
    if (quantity > 30) {
      return NextResponse.json(
        { ok: false, error: "Jumlah maksimal 30 per order." },
        { status: 400 },
      );
    }
    // Telegram Web App: validasi initData -> buyerRef dari user terverifikasi.
    let buyerRef =
      typeof body.buyerRef === "string" ? body.buyerRef : "web";
    if (typeof body.initData === "string" && body.initData) {
      let botToken: string;
      try {
        botToken = telegramBotToken();
      } catch {
        return NextResponse.json(
          { ok: false, error: "Telegram belum dikonfigurasi di server." },
          { status: 500 },
        );
      }
      const valid = validateInitData(body.initData, botToken);
      if (!valid.ok) {
        return NextResponse.json(
          { ok: false, error: `initData Telegram tidak valid (${valid.reason}).` },
          { status: 401 },
        );
      }
      buyerRef = `tg:${valid.user.id}`;
    }
    // Jangan biarkan pembeli bayar kalau SUDAH PASTI saldo supplier tidak
    // cukup. Kalau ceknya gagal (API supplier tidak bisa dihubungi),
    // checkSupplierFunds fail open — order tetap jalan seperti dulu.
    const funds = await checkSupplierFunds(productId, quantity);
    if (!funds.ok) {
      return NextResponse.json(
        { ok: false, error: funds.reason ?? "Produk ini sedang tidak bisa dibeli." },
        { status: 400 },
      );
    }
    const order = await createStoreOrder({
      catalogProductId: productId,
      quantity,
      buyerRef,
      xiaomiAccounts: Array.isArray(body.xiaomiAccounts)
        ? (body.xiaomiAccounts as unknown[]).map(String)
        : undefined,
      sheeridUrl:
        typeof body.sheeridUrl === "string" ? body.sheeridUrl : null,
      // Sondev: target WAJIB untuk produk on-demand (kode invite / email).
      target: typeof body.target === "string" ? body.target.trim() : null,
    });
    // Order gratis (promo menutup total) — langsung lunas + proses.
    if (order.retailTotalIdr <= 0) {
      await markPaid(order.id);
      void fulfillOrder(order.id).catch((err: unknown) => {
        console.error(`[api/orders] fulfill ${order.id} gagal:`, err);
      });
      return NextResponse.json(
        {
          ok: true,
          order: { id: order.id, status: "paid", totalIdr: 0 },
          payment: null,
        },
        { status: 201 },
      );
    }
    await setOrderPaidVia(order.id, "midtrans");
    const product = getCatalogProduct(order.catalogProductId);
    const snap = await createSnapTransaction({
      orderId: order.id,
      amountIdr: order.retailTotalIdr,
      itemName: product.name,
    });
    const payment = createMidtransPayment({
      orderId: order.id,
      amountIdr: order.retailTotalIdr,
      snapToken: snap.token,
      redirectUrl: snap.redirectUrl,
    });
    return NextResponse.json(
      {
        ok: true,
        order: { id: order.id, status: order.status, totalIdr: order.retailTotalIdr },
        payment: {
          redirectUrl: payment.redirect_url,
          snapToken: payment.snap_token,
          expiresAt: payment.expires_at,
          reference: payment.order_id,
        },
      },
      { status: 201 },
    );
  } catch (err) {
    return NextResponse.json(
      { ok: false, error: err instanceof Error ? err.message : String(err) },
      { status: 400 },
    );
  }
}
