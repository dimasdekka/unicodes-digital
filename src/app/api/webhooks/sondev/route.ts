import { createHmac, timingSafeEqual } from "node:crypto";
import { NextResponse } from "next/server";
import { sondevConfig } from "@/lib/config";
import { recordWebhookDelivery } from "@/lib/db";
import {
  getStoreOrder,
  markOrderDelivered,
  markOrderFailed,
} from "@/lib/orders";

/**
 * POST /api/webhooks/sondev — terima update order dari Sondev.
 * Diselaraskan dengan template resmi Sondev (sondev_reseller_bot.py):
 * - Header: X-Sondev-Timestamp + X-Sondev-Signature (BUKAN X-Timestamp).
 * - Signature: HMAC-SHA256(secret, timestamp + "\n" + raw body),
 *   toleransi ±300 detik; signature boleh berawalan "sha256=".
 * - Payload: { event, order_no, ref_id, delivery_status, delivered_data, refund_note }
 *   event: "order.completed" / "order.failed";
 *   delivery_status: DELIVERED / FAILED / REFUNDED / PARTIAL_DELIVERED.
 * - delivered_data.items: array string ("email|mailbox_link", URL, "email:password");
 *   token/kode ada di delivered_data.code.
 * - Jawab 200 CEPAT (Sondev memberi ~12 detik sebelum dead-letter) —
 *   handler ini hanya update DB, tidak memanggil API supplier.
 * LahaStore tidak punya inbound webhook — order LahaStore dipantau via polling
 * di fulfillOrder (src/lib/orders.ts).
 */
function verifySignature(
  rawBody: string,
  timestamp: string,
  signature: string,
  secret: string,
): boolean {
  if (!timestamp || !signature) return false;
  const ts = Number(timestamp);
  if (!Number.isFinite(ts) || Math.abs(Date.now() / 1000 - ts) > 300) {
    return false;
  }
  let provided = signature.trim();
  if (provided.startsWith("sha256=")) provided = provided.slice(7);
  const expected = createHmac("sha256", secret)
    .update(`${timestamp}\n${rawBody}`)
    .digest("hex");
  const a = Buffer.from(provided, "utf8");
  const b = Buffer.from(expected, "utf8");
  return a.length === b.length && timingSafeEqual(a, b);
}

export async function POST(req: Request) {
  let secret: string;
  try {
    secret = sondevConfig().apiSecret;
  } catch {
    return NextResponse.json({ ok: false, error: "SONDEV_API_SECRET belum diisi" }, { status: 500 });
  }
  const rawBody = await req.text();
  const timestamp = req.headers.get("x-sondev-timestamp") ?? "";
  const signature = req.headers.get("x-sondev-signature") ?? "";
  if (!verifySignature(rawBody, timestamp, signature, secret)) {
    return NextResponse.json({ ok: false, error: "Invalid signature" }, { status: 401 });
  }

  let payload: Record<string, unknown>;
  try {
    payload = JSON.parse(rawBody) as Record<string, unknown>;
  } catch {
    return NextResponse.json({ ok: false, error: "Invalid JSON" }, { status: 400 });
  }

  const refId = typeof payload.ref_id === "string" ? payload.ref_id : null;
  const event = String(payload.event ?? "");
  const deliveryStatus = String(payload.delivery_status ?? "").toUpperCase();

  // webhook.test bukan event order — jawab 200 saja (docs v1.2).
  if (event === "webhook.test") {
    return NextResponse.json({ ok: true });
  }

  // Dedupe: X-Sondev-Delivery unik per grup percobaan — retry Sondev tidak
  // boleh memproses order yang sama dua kali (docs v1.2).
  const deliveryId = req.headers.get("x-sondev-delivery") ?? "";
  if (!recordWebhookDelivery("sondev", deliveryId)) {
    return NextResponse.json({ ok: true, duplicate: true });
  }

  if (refId) {
    const order = await getStoreOrder(refId);
    if (order && (order.status === "processing" || order.status === "paid")) {
      if (event === "order.completed" || deliveryStatus === "DELIVERED") {
        const dd = (payload.delivered_data ?? {}) as Record<string, unknown>;
        const items = Array.isArray(dd.items)
          ? (dd.items as unknown[]).map(String)
          : order.deliveredItems;
        if (typeof dd.code === "string" && dd.code.trim()) items.push(dd.code);
        let resultLink: string | null = null;
        for (const it of items) {
          const m = it.match(/https?:\/\/\S+/);
          if (m) {
            resultLink = m[0];
            break;
          }
        }
        await markOrderDelivered(refId, items, resultLink);
      } else if (
        event === "order.failed" ||
        ["FAILED", "REFUNDED", "PARTIAL_DELIVERED"].includes(deliveryStatus)
      ) {
        const note =
          typeof payload.refund_note === "string" && payload.refund_note
            ? payload.refund_note
            : "Pesanan gagal diproses supplier.";
        await markOrderFailed(refId, note);
      }
      // Event non-terminal (mis. PROCESSING) diabaikan — polling yang urus.
    }
  }
  return NextResponse.json({ ok: true });
}
