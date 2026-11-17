import { NextResponse } from "next/server";
import {
  checkSupplierFunds,
  createStoreOrder,
  fulfillOrder,
  markPaid,
  setOrderPaidVia,
} from "@/lib/orders";
import { createQrisPayment } from "@/lib/payments/qris";
import { renderQrisCardOrPlain } from "@/lib/payments/qris-card";
import { getGopayStaticQris } from "@/lib/payments/qris-settings";
import { validateInitData } from "@/lib/telegram-webapp";
import { telegramBotToken } from "@/lib/config";

/**
 * POST /api/orders — buat order + pembayaran QRIS dinamis.
 * Body JSON: { productId, quantity?, buyerRef?, xiaomiAccounts?, sheeridUrl? }
 * Balasan: { ok, order: { id, status, totalIdr }, payment: {
 *   qrPngBase64, qrString, uniqueAmountIdr, expiresAt, reference } | null }
 * Pembeli scan QR (kartu standee komposit) dengan nominal unik yang tertera.
 * Setelah lunas, poller feed GoBiz (/lib/payments/qris-poller.ts) akan
 * memanggil markPaid -> fulfillOrder otomatis. Klien (web / /tg) memantau
 * status order lewat GET /api/orders/[id] sampai delivered/failed.
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
    await setOrderPaidVia(order.id, "qris");
    const staticPayload = getGopayStaticQris();
    if (!staticPayload) {
      throw new Error("QRIS statis belum dipasang di panel admin (/admin).");
    }
    const { payment: qrisPayment, qrString } = createQrisPayment({
      reference: order.id,
      amountIdr: order.retailTotalIdr,
      staticPayload,
    });
    // Kartu standee komposit (fallback QR polos kalau template tidak ada).
    const { png } = await renderQrisCardOrPlain(qrString);
    return NextResponse.json(
      {
        ok: true,
        order: { id: order.id, status: order.status, totalIdr: order.retailTotalIdr },
        payment: {
          qrPngBase64: png.toString("base64"),
          qrString,
          uniqueAmountIdr: qrisPayment.unique_amount_idr,
          expiresAt: qrisPayment.expires_at,
          reference: qrisPayment.reference,
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
