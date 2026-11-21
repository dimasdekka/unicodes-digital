//! Route JSON `/api/*` + webhook — port `src/app/api/**` (versi TS).
//!
//! - `POST /api/orders` (+ `GET /api/orders/:id`, `POST .../fulfill`)
//! - `GET /api/products`
//! - `/api/admin/*`: login/logout, tiket, harga produk, QRIS, GoPay
//! - `POST /api/webhooks/midtrans`, `POST /api/webhooks/sondev`
//!
//! Semua route `/api/admin/*` menjawab 401 tanpa cookie admin yang valid.
//! Panel GoPay (OTP GoBiz) di-port apa adanya — jalur DORMANT, OTP
//! diblokir server Gojek (HTTP 400 GoPay-1000); jangan "perbaiki" OTP-nya.

use std::collections::HashMap;

use axum::body::Bytes;
use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Redirect, Response};
use axum::Form;
use axum::Json;
use hmac::{Hmac, Mac};
use serde::Deserialize;
use serde_json::{json, Value};
use sha2::Sha256;

use crate::catalog::{get_catalog_product, list_active_products, CATALOG};
use crate::db::record_webhook_delivery;
use crate::lib::notify::NotifyCtx;
use crate::lib::orders::{
    check_supplier_funds, create_store_order, fulfill_order, get_store_order, mark_order_delivered,
    mark_order_failed, mark_paid, set_order_paid_via, CreateStoreOrderInput, StoreOrder,
    StoreOrderStatus,
};
use crate::lib::payments::gopay::{GopayClient, GopayError, GopayErrorCode};
use crate::lib::payments::midtrans::{
    create_midtrans_payment, create_snap_transaction_with, midtrans_configured,
    verify_notification_signature, MidtransClient,
};
use crate::lib::payments::midtrans_poller::process_midtrans_result;
use crate::lib::payments::qris::{create_qris_payment, QrisPaymentRow};
use crate::lib::payments::qris_card::render_qris_card_or_plain;
use crate::lib::payments::qris_poller::{tick_qris_payments, QrisPollCode};
use crate::lib::payments::qris_settings::{
    extract_merchant_info, get_gopay_static_qris, list_recent_qris_payments,
    set_gopay_static_qris, validate_static_qris,
};
use crate::lib::product_prices::set_price_override;
use crate::lib::suppliers::{get_supplier, SupplierId};
use crate::lib::support_tickets::{
    close_ticket, count_open_tickets, get_ticket, get_ticket_messages, list_tickets,
    reply_to_ticket, reopen_ticket, SupportMessage, SupportTicket,
};
use crate::lib::telegram_webapp::{validate_init_data, InitDataValidation};
use crate::web::auth::{
    admin_password_hash, hash_password, is_admin_request, login_set_cookie_header,
    logout_set_cookie_header,
};
use crate::web::util::{is_form_post, parse_body, url_encode};
use crate::web::AppState;

type HmacSha256 = Hmac<Sha256>;

pub type ApiResponse = (StatusCode, Json<Value>);

fn ok_json(v: Value) -> ApiResponse {
    (StatusCode::OK, Json(v))
}

fn err_json(status: StatusCode, msg: &str) -> ApiResponse {
    (status, Json(json!({ "ok": false, "error": msg })))
}

fn unauthorized() -> ApiResponse {
    err_json(StatusCode::UNAUTHORIZED, "Tidak ada akses admin")
}

fn ct_eq(a: &str, b: &str) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for (x, y) in a.as_bytes().iter().zip(b.as_bytes().iter()) {
        diff |= x ^ y;
    }
    diff == 0
}

/// Satu order dalam bentuk JSON API (kunci camelCase, seperti TS).
fn order_json(o: &StoreOrder) -> Value {
    json!({
        "id": o.id,
        "product": o.catalog_product_id,
        "quantity": o.quantity,
        "totalIdr": o.retail_total_idr,
        "status": o.status.as_str(),
        "supplier": o.supplier,
        "deliveredItems": o.delivered_items,
        "resultLink": o.result_link,
        "error": o.error,
        "createdAt": o.created_at,
        "updatedAt": o.updated_at,
    })
}

fn ticket_json(t: &SupportTicket) -> Value {
    json!({
        "id": t.id,
        "tgId": t.tg_id,
        "username": t.username,
        "firstName": t.first_name,
        "status": t.status.as_str(),
        "subject": t.subject,
        "orderId": t.order_id,
        "createdAt": t.created_at,
        "updatedAt": t.updated_at,
        "lastMessageAt": t.last_message_at,
    })
}

fn message_json(m: &SupportMessage) -> Value {
    json!({
        "id": m.id,
        "ticketId": m.ticket_id,
        "sender": m.sender,
        "text": m.text,
        "createdAt": m.created_at,
    })
}

fn qris_row_json(r: &QrisPaymentRow) -> Value {
    json!({
        "reference": r.reference,
        "amount": r.amount_idr,
        "unique_amount": r.unique_amount_idr,
        "status": r.status.as_str(),
        "created_at": r.created_at,
    })
}

// ---------------------------------------------------------------------------
// Katalog + order (publik)
// ---------------------------------------------------------------------------

/// GET /api/products — katalog gabungan (semua supplier) untuk storefront.
pub async fn list_products(State(state): State<AppState>) -> ApiResponse {
    let products: Vec<Value> = list_active_products(&state.db)
        .iter()
        .map(crate::web::views::product_json)
        .collect();
    ok_json(json!({ "ok": true, "products": products }))
}

/// POST /api/orders — buat order + pembayaran Midtrans Snap (QRIS).
/// Body JSON: { productId, quantity?, buyerRef?, xiaomiAccounts?,
/// sheeridUrl?, target?, initData? }.
/// Jumlah custom tidak boleh lebih dari 30 (batas penebusan Muse per kode).
/// Web MASIH urutan lama: qty -> target -> bayar (bot: kode dulu baru qty)
/// — dicatat sebagai gap, jangan disamakan diam-diam (lihat notes/E-web.md).
pub async fn create_order(State(state): State<AppState>, body: Bytes) -> ApiResponse {
    let v: Value = serde_json::from_slice(&body).unwrap_or_default();
    let product_id = v
        .get("productId")
        .and_then(|x| x.as_str())
        .unwrap_or("")
        .to_string();
    let quantity = match v.get("quantity").and_then(|x| x.as_f64()) {
        Some(n) if n > 0.0 => n.floor() as i64,
        _ => 1,
    };
    if quantity > 30 {
        return err_json(
            StatusCode::BAD_REQUEST,
            "Jumlah maksimal 30 per order.",
        );
    }
    // Telegram Web App: validasi initData -> buyerRef dari user terverifikasi.
    let mut buyer_ref = v
        .get("buyerRef")
        .and_then(|x| x.as_str())
        .unwrap_or("web")
        .to_string();
    if let Some(init_data) = v
        .get("initData")
        .and_then(|x| x.as_str())
        .filter(|s| !s.is_empty())
    {
        if state.cfg.telegram_bot_token.is_empty() {
            return err_json(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Telegram belum dikonfigurasi di server.",
            );
        }
        match validate_init_data(init_data, &state.cfg.telegram_bot_token) {
            InitDataValidation::Ok { user, .. } => {
                buyer_ref = format!("tg:{}", user.id);
            }
            other => {
                return err_json(
                    StatusCode::UNAUTHORIZED,
                    &format!(
                        "initData Telegram tidak valid ({}).",
                        other.reason().unwrap_or("bad_init_data")
                    ),
                );
            }
        }
    }
    // Jangan biarkan pembeli bayar kalau SUDAH PASTI saldo supplier tidak
    // cukup (fail open kalau ceknya gagal — lihat check_supplier_funds).
    let product = match get_catalog_product(&state.db, &product_id) {
        Ok(p) => p,
        Err(e) => return err_json(StatusCode::BAD_REQUEST, &e.to_string()),
    };
    let supplier_id = match SupplierId::from_str(product.supplier) {
        Some(s) => s,
        None => {
            return err_json(
                StatusCode::BAD_REQUEST,
                &format!("Supplier belum terdaftar: {}", product.supplier),
            )
        }
    };
    let supplier = match get_supplier(supplier_id, &state.cfg) {
        Ok(s) => s,
        Err(e) => return err_json(StatusCode::BAD_REQUEST, &e.to_string()),
    };
    let (funds_ok, reason) =
        check_supplier_funds(&state.db, &supplier, &product_id, quantity).await;
    if !funds_ok {
        return err_json(
            StatusCode::BAD_REQUEST,
            reason
                .as_deref()
                .unwrap_or("Produk ini sedang tidak bisa dibeli."),
        );
    }
    let xiaomi_accounts = v.get("xiaomiAccounts").and_then(|x| x.as_array()).map(|a| {
        a.iter()
            .filter_map(|x| x.as_str().map(|s| s.to_string()))
            .collect::<Vec<_>>()
    });
    let sheerid_url = v
        .get("sheeridUrl")
        .and_then(|x| x.as_str())
        .map(|s| s.to_string());
    let target = v
        .get("target")
        .and_then(|x| x.as_str())
        .map(|s| s.trim().to_string());
    let order = match create_store_order(
        &state.db,
        CreateStoreOrderInput {
            id: None,
            catalog_product_id: product_id,
            quantity: Some(quantity),
            buyer_ref: Some(buyer_ref),
            xiaomi_accounts,
            sheerid_url,
            target,
            discount_idr: None,
            promo_code: None,
        },
    ) {
        Ok(o) => o,
        Err(e) => return err_json(StatusCode::BAD_REQUEST, &e.to_string()),
    };
    // Order gratis (promo menutup total) — langsung lunas + proses.
    if order.retail_total_idr <= 0 {
        if let Err(e) = mark_paid(&state.db, &order.id) {
            return err_json(StatusCode::BAD_REQUEST, &e.to_string());
        }
        // Fulfillment jalan di background (sama seperti TS: void fulfillOrder).
        let db = state.db.clone();
        let cfg = state.cfg.clone();
        let order_id = order.id.clone();
        tokio::spawn(async move {
            let Some(ord) = get_store_order(&db, &order_id) else {
                return;
            };
            let Some(sid) = SupplierId::from_str(&ord.supplier) else {
                return;
            };
            let Ok(sup) = get_supplier(sid, &cfg) else {
                return;
            };
            if let Err(e) = fulfill_order(&db, None, &sup, &order_id).await {
                tracing::error!("[api/orders] fulfill {order_id} gagal: {e}");
            }
        });
        return (
            StatusCode::CREATED,
            Json(json!({
                "ok": true,
                "order": { "id": order.id, "status": "paid", "totalIdr": 0 },
                "payment": null,
            })),
        );
    }
    if let Err(e) = set_order_paid_via(&state.db, &order.id, "midtrans") {
        return err_json(StatusCode::BAD_REQUEST, &e.to_string());
    }
    // Seam Midtrans (Gap 2): client dibangun dari cfg — default URL
    // sandbox/produksi seperti dulu; tes mengarahkan via env override.
    let snap = match create_snap_transaction_with(
        &MidtransClient::from_client(&state.http, &state.cfg),
        &state.cfg,
        &order.id,
        order.retail_total_idr,
        product.name,
    )
    .await
    {
        Ok(s) => s,
        Err(e) => return err_json(StatusCode::BAD_REQUEST, &e.to_string()),
    };
    let payment = match create_midtrans_payment(
        &state.db,
        &order.id,
        order.retail_total_idr,
        &snap.token,
        &snap.redirect_url,
        None,
    ) {
        Ok(p) => p,
        Err(e) => return err_json(StatusCode::BAD_REQUEST, &e.to_string()),
    };
    (
        StatusCode::CREATED,
        Json(json!({
            "ok": true,
            "order": { "id": order.id, "status": order.status.as_str(), "totalIdr": order.retail_total_idr },
            "payment": {
                "redirectUrl": payment.redirect_url,
                "snapToken": payment.snap_token,
                "expiresAt": payment.expires_at,
                "reference": payment.order_id,
            },
        })),
    )
}

/// GET /api/orders/:id — lacak status order.
pub async fn get_order(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> ApiResponse {
    match get_store_order(&state.db, &id) {
        Some(o) => ok_json(json!({ "ok": true, "order": order_json(&o) })),
        None => err_json(StatusCode::NOT_FOUND, "Order tidak ditemukan"),
    }
}

/// POST /api/orders/:id/fulfill — JALANKAN FULFILLMENT (testing/admin saja).
/// TODO PRODUKSI (sama seperti TS): lindungi route ini (admin auth) dan
/// panggil HANYA setelah pembayaran terkonfirmasi. Di sini markPaid
/// dipanggil otomatis supaya bisa dites end-to-end tanpa payment gateway.
pub async fn fulfill_order_route(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> ApiResponse {
    let Some(order) = get_store_order(&state.db, &id) else {
        return err_json(StatusCode::NOT_FOUND, "Order tidak ditemukan");
    };
    if let Err(e) = mark_paid(&state.db, &id) {
        return err_json(StatusCode::BAD_REQUEST, &e.to_string());
    }
    let supplier = match SupplierId::from_str(&order.supplier)
        .ok_or_else(|| anyhow::anyhow!("Supplier belum terdaftar: {}", order.supplier))
        .and_then(|sid| get_supplier(sid, &state.cfg))
    {
        Ok(s) => s,
        Err(e) => return err_json(StatusCode::BAD_REQUEST, &e.to_string()),
    };
    let notify = NotifyCtx::from_config(&state.cfg);
    match fulfill_order(&state.db, Some(&notify), &supplier, &id).await {
        Ok(done) => ok_json(json!({ "ok": true, "order": order_json(&done) })),
        Err(e) => err_json(StatusCode::BAD_REQUEST, &e.to_string()),
    }
}

// ---------------------------------------------------------------------------
// Webhook
// ---------------------------------------------------------------------------

/// POST /api/webhooks/midtrans — notifikasi pembayaran Midtrans.
/// Verifikasi signature SHA512; lalu proses via SATU fungsi transisi
/// `process_midtrans_result` (jalur yang sama dengan poller — idempotent).
pub async fn midtrans_webhook(State(state): State<AppState>, body: Bytes) -> ApiResponse {
    if !midtrans_configured(&state.cfg) {
        return err_json(
            StatusCode::INTERNAL_SERVER_ERROR,
            "Midtrans belum dikonfigurasi.",
        );
    }
    let v: Value = serde_json::from_slice(&body).unwrap_or_default();
    let get_s = |k: &str| {
        v.get(k)
            .and_then(|x| x.as_str())
            .unwrap_or("")
            .to_string()
    };
    let order_id = get_s("order_id");
    let status_code = get_s("status_code");
    let gross_amount = match v.get("gross_amount") {
        Some(Value::String(s)) => s.clone(),
        Some(other) => other.to_string(),
        None => String::new(),
    };
    let signature_key = get_s("signature_key");
    let tx_status = get_s("transaction_status");
    if order_id.is_empty()
        || status_code.is_empty()
        || gross_amount.is_empty()
        || signature_key.is_empty()
        || tx_status.is_empty()
    {
        return err_json(StatusCode::BAD_REQUEST, "Body notifikasi tidak lengkap.");
    }
    let valid = verify_notification_signature(
        &state.cfg.midtrans_server_key,
        &order_id,
        &status_code,
        &gross_amount,
        &signature_key,
    );
    if !valid {
        return err_json(StatusCode::UNAUTHORIZED, "Signature tidak valid.");
    }
    let outcome =
        process_midtrans_result(&state.db, state.settlement.as_ref(), &order_id, &tx_status)
            .await;
    tracing::info!("[webhook/midtrans] {order_id} {tx_status} -> {outcome:?}");
    ok_json(json!({ "ok": true }))
}

/// Verifikasi signature webhook Sondev (port dari route TS):
/// HMAC-SHA256(secret, timestamp + "\n" + raw body), toleransi ±300 detik,
/// signature boleh berawalan "sha256=".
fn verify_sondev_signature(
    raw_body: &[u8],
    timestamp: &str,
    signature: &str,
    secret: &str,
) -> bool {
    if timestamp.is_empty() || signature.is_empty() {
        return false;
    }
    let ts: i64 = match timestamp.trim().parse() {
        Ok(n) => n,
        Err(_) => return false,
    };
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    if (now - ts).abs() > 300 {
        return false;
    }
    let provided = signature
        .trim()
        .strip_prefix("sha256=")
        .unwrap_or(signature.trim());
    let mut mac = match HmacSha256::new_from_slice(secret.as_bytes()) {
        Ok(m) => m,
        Err(_) => return false,
    };
    mac.update(timestamp.as_bytes());
    mac.update(b"\n");
    mac.update(raw_body);
    let expected = hex::encode(mac.finalize().into_bytes());
    ct_eq(&expected, provided)
}

fn first_url(text: &str) -> Option<String> {
    for token in text.split_whitespace() {
        if token.starts_with("http://") || token.starts_with("https://") {
            return Some(token.trim_end_matches([',', '.', ')']).to_string());
        }
    }
    None
}

/// POST /api/webhooks/sondev — terima update order dari Sondev.
/// Header X-Sondev-Timestamp + X-Sondev-Signature; dedupe via
/// X-Sondev-Delivery. Jawab 200 CEPAT — handler hanya update DB.
pub async fn sondev_webhook(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Bytes,
) -> ApiResponse {
    let secret = state.cfg.sondev_api_secret.clone();
    if secret.is_empty() {
        return err_json(
            StatusCode::INTERNAL_SERVER_ERROR,
            "SONDEV_API_SECRET belum diisi",
        );
    }
    let timestamp = headers
        .get("x-sondev-timestamp")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    let signature = headers
        .get("x-sondev-signature")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    if !verify_sondev_signature(&body, timestamp, signature, &secret) {
        return err_json(StatusCode::UNAUTHORIZED, "Invalid signature");
    }
    let payload: Value = match serde_json::from_slice(&body) {
        Ok(v) => v,
        Err(_) => return err_json(StatusCode::BAD_REQUEST, "Invalid JSON"),
    };
    let event = payload
        .get("event")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    // webhook.test bukan event order — jawab 200 saja (docs v1.2).
    if event == "webhook.test" {
        return ok_json(json!({ "ok": true }));
    }
    // Dedupe: X-Sondev-Delivery unik per grup percobaan.
    let delivery_id = headers
        .get("x-sondev-delivery")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    if !record_webhook_delivery(&state.db, "sondev", delivery_id) {
        return ok_json(json!({ "ok": true, "duplicate": true }));
    }
    let delivery_status = payload
        .get("delivery_status")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_uppercase();
    if let Some(ref_id) = payload.get("ref_id").and_then(|v| v.as_str()) {
        if let Some(order) = get_store_order(&state.db, ref_id) {
            if order.status == StoreOrderStatus::Processing
                || order.status == StoreOrderStatus::Paid
            {
                if event == "order.completed" || delivery_status == "DELIVERED" {
                    let dd = payload
                        .get("delivered_data")
                        .cloned()
                        .unwrap_or(json!({}));
                    let mut items: Vec<String> = dd
                        .get("items")
                        .and_then(|v| v.as_array())
                        .map(|a| {
                            a.iter()
                                .filter_map(|x| x.as_str().map(|s| s.to_string()))
                                .collect()
                        })
                        .unwrap_or_else(|| order.delivered_items.clone());
                    if let Some(code) = dd
                        .get("code")
                        .and_then(|v| v.as_str())
                        .filter(|s| !s.trim().is_empty())
                    {
                        items.push(code.to_string());
                    }
                    let mut result_link = None;
                    for it in &items {
                        if let Some(url) = first_url(it) {
                            result_link = Some(url);
                            break;
                        }
                    }
                    let _ = mark_order_delivered(&state.db, ref_id, items, result_link);
                } else if event == "order.failed"
                    || ["FAILED", "REFUNDED", "PARTIAL_DELIVERED"]
                        .contains(&delivery_status.as_str())
                {
                    let note = payload
                        .get("refund_note")
                        .and_then(|v| v.as_str())
                        .filter(|s| !s.is_empty())
                        .unwrap_or("Pesanan gagal diproses supplier.");
                    let _ = mark_order_failed(&state.db, ref_id, note);
                }
                // Event non-terminal (mis. PROCESSING) diabaikan — polling yang urus.
            }
        }
    }
    ok_json(json!({ "ok": true }))
}

// ---------------------------------------------------------------------------
// Admin: login/logout
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
pub struct LoginForm {
    #[serde(default)]
    pub password: String,
}

/// POST /api/admin/login — form {password} -> cookie httpOnly -> redirect /admin.
pub async fn admin_login(
    State(state): State<AppState>,
    Form(form): Form<LoginForm>,
) -> Response {
    let expected = match admin_password_hash(&state.cfg) {
        Some(h) => h,
        None => {
            return err_json(
                StatusCode::INTERNAL_SERVER_ERROR,
                "ADMIN_PASSWORD belum diisi di env",
            )
            .into_response()
        }
    };
    if hash_password(&form.password) != expected {
        return Redirect::to("/admin?error=1").into_response();
    }
    let mut headers = HeaderMap::new();
    if let Ok(v) = HeaderValue::from_str(&login_set_cookie_header(&expected)) {
        headers.insert(axum::http::header::SET_COOKIE, v);
    }
    (headers, Redirect::to("/admin")).into_response()
}

/// GET /api/admin/logout — hapus cookie -> redirect /admin.
pub async fn admin_logout() -> (HeaderMap, Redirect) {
    let mut headers = HeaderMap::new();
    if let Ok(v) = HeaderValue::from_str(&logout_set_cookie_header()) {
        headers.insert(axum::http::header::SET_COOKIE, v);
    }
    (headers, Redirect::to("/admin"))
}

// ---------------------------------------------------------------------------
// Admin: tiket support
// ---------------------------------------------------------------------------

/// GET /api/admin/tickets?limit= — daftar tiket (JSON).
pub async fn admin_list_tickets(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(q): Query<HashMap<String, String>>,
) -> ApiResponse {
    if !is_admin_request(&state.cfg, &headers) {
        return unauthorized();
    }
    let limit: i64 = q
        .get("limit")
        .and_then(|s| s.parse().ok())
        .unwrap_or(50)
        .clamp(1, 200);
    let tickets = list_tickets(&state.db, limit);
    let open = count_open_tickets(&state.db);
    ok_json(json!({
        "ok": true,
        "openCount": open,
        "tickets": tickets.iter().map(ticket_json).collect::<Vec<_>>(),
    }))
}

/// GET /api/admin/tickets/:id — detail satu tiket + thread pesannya.
pub async fn admin_get_ticket(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> ApiResponse {
    if !is_admin_request(&state.cfg, &headers) {
        return unauthorized();
    }
    let Some(ticket) = get_ticket(&state.db, &id) else {
        return err_json(StatusCode::NOT_FOUND, "Tiket tidak ditemukan");
    };
    let messages = get_ticket_messages(&state.db, &id);
    ok_json(json!({
        "ok": true,
        "ticket": ticket_json(&ticket),
        "messages": messages.iter().map(message_json).collect::<Vec<_>>(),
    }))
}

/// POST /api/admin/tickets/:id/reply — balas tiket (JSON {text} atau form).
/// Balasan dikirim ke user via Bot API Telegram + simpan ke thread.
pub async fn admin_reply_ticket(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    body: Bytes,
) -> Response {
    if !is_admin_request(&state.cfg, &headers) {
        return unauthorized().into_response();
    }
    if get_ticket(&state.db, &id).is_none() {
        return err_json(StatusCode::NOT_FOUND, "Tiket tidak ditemukan").into_response();
    }
    let parsed = parse_body(&headers, &body);
    let text = parsed.get("text");
    let Some(text) = text.filter(|s| !s.trim().is_empty()) else {
        return err_json(StatusCode::BAD_REQUEST, "Balasan kosong").into_response();
    };
    let result = reply_to_ticket(&state.db, &state.cfg.telegram_bot_token, &id, &text).await;
    if parsed.is_form() {
        let mut url = format!("/admin/tickets/{id}");
        if let Err(e) = &result {
            url = format!("{url}?error={}", url_encode(e));
        }
        return Redirect::to(&url).into_response();
    }
    match result {
        // Balasan tetap tersimpan di thread — yang gagal hanya kirim ke
        // Telegram. Beri tahu admin supaya bisa kirim ulang.
        Ok(()) => ok_json(json!({ "ok": true, "ticketId": id })).into_response(),
        Err(e) => err_json(StatusCode::BAD_GATEWAY, &e).into_response(),
    }
}

/// POST /api/admin/tickets/:id/close — tutup tiket + kabari user di Telegram.
pub async fn admin_close_ticket(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Response {
    if !is_admin_request(&state.cfg, &headers) {
        return unauthorized().into_response();
    }
    if get_ticket(&state.db, &id).is_none() {
        return err_json(StatusCode::NOT_FOUND, "Tiket tidak ditemukan").into_response();
    }
    let result = close_ticket(&state.db, &state.cfg.telegram_bot_token, &id).await;
    if is_form_post(&headers) {
        return Redirect::to(&format!("/admin/tickets/{id}")).into_response();
    }
    ok_json(json!({ "ok": true, "ticketId": id, "telegramSent": result.is_ok() })).into_response()
}

/// POST /api/admin/tickets/:id/reopen — buka lagi tiket yang sudah ditutup.
pub async fn admin_reopen_ticket(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Response {
    if !is_admin_request(&state.cfg, &headers) {
        return unauthorized().into_response();
    }
    if get_ticket(&state.db, &id).is_none() {
        return err_json(StatusCode::NOT_FOUND, "Tiket tidak ditemukan").into_response();
    }
    let _ = reopen_ticket(&state.db, &id);
    if is_form_post(&headers) {
        return Redirect::to(&format!("/admin/tickets/{id}")).into_response();
    }
    ok_json(json!({ "ok": true, "ticketId": id })).into_response()
}

// ---------------------------------------------------------------------------
// Admin: harga produk
// ---------------------------------------------------------------------------

/// POST /api/admin/products/:id/price — ubah harga retail (JSON {priceIdr}
/// atau form {price}). Override disimpan di DB, berlaku langsung.
pub async fn admin_set_price(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    body: Bytes,
) -> Response {
    if !is_admin_request(&state.cfg, &headers) {
        return unauthorized().into_response();
    }
    if !CATALOG.iter().any(|p| p.id == id) {
        return err_json(StatusCode::NOT_FOUND, "Produk tidak ditemukan").into_response();
    }
    let parsed = parse_body(&headers, &body);
    let price_raw = parsed.get("priceIdr").or_else(|| parsed.get("price"));
    let price = price_raw.and_then(|s| s.parse::<f64>().ok());
    let Some(price) = price.filter(|n| n.is_finite()) else {
        return err_json(StatusCode::BAD_REQUEST, "Harga tidak valid").into_response();
    };
    let rounded = price.floor() as i64;
    if !(500..=10_000_000).contains(&rounded) {
        return err_json(
            StatusCode::BAD_REQUEST,
            "Harga harus Rp500 – Rp10.000.000",
        )
        .into_response();
    }
    if let Err(e) = set_price_override(&state.db, &id, rounded) {
        return err_json(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()).into_response();
    }
    if parsed.is_form() {
        return Redirect::to("/admin").into_response();
    }
    ok_json(json!({ "ok": true, "productId": id, "priceIdr": rounded })).into_response()
}

// ---------------------------------------------------------------------------
// Admin: QRIS GoPay
// ---------------------------------------------------------------------------

/// GET /api/admin/payments/qris — status QRIS (tanpa string lengkap).
pub async fn qris_status(State(state): State<AppState>, headers: HeaderMap) -> ApiResponse {
    if !is_admin_request(&state.cfg, &headers) {
        return unauthorized();
    }
    let stored = match get_gopay_static_qris(&state.db) {
        Ok(v) => v,
        Err(e) => return err_json(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()),
    };
    let Some(stored) = stored else {
        return ok_json(json!({
            "configured": false,
            "merchant_name": null,
            "merchant_city": null,
            "merchant_id": null,
        }));
    };
    match extract_merchant_info(&stored) {
        Ok(info) => ok_json(json!({
            "configured": true,
            "merchant_name": info.merchant_name,
            "merchant_city": info.merchant_city,
            "merchant_id": info.merchant_id,
        })),
        Err(e) => ok_json(json!({
            "configured": true,
            "merchant_name": null,
            "merchant_city": null,
            "merchant_id": null,
            "error": e.to_string(),
        })),
    }
}

/// POST /api/admin/payments/qris — simpan string QRIS statis (validasi
/// parse + CRC dulu). String lengkap TIDAK pernah dikembalikan ke client.
pub async fn qris_save(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Bytes,
) -> ApiResponse {
    if !is_admin_request(&state.cfg, &headers) {
        return unauthorized();
    }
    let v: Value = match serde_json::from_slice(&body) {
        Ok(v) => v,
        Err(_) => {
            return err_json(
                StatusCode::BAD_REQUEST,
                "Body harus JSON { static_qris }",
            )
        }
    };
    let static_qris = v
        .get("static_qris")
        .and_then(|x| x.as_str())
        .unwrap_or("")
        .trim()
        .to_string();
    match validate_static_qris(&static_qris) {
        Ok(info) => {
            if let Err(e) = set_gopay_static_qris(&state.db, &static_qris) {
                return err_json(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string());
            }
            ok_json(json!({
                "ok": true,
                "configured": true,
                "merchant_name": info.merchant_name,
                "merchant_city": info.merchant_city,
                "merchant_id": info.merchant_id,
            }))
        }
        Err(e) => err_json(StatusCode::BAD_REQUEST, &e.to_string()),
    }
}

/// GET /api/admin/payments/qris/payments — daftar pembayaran QRIS terbaru.
pub async fn qris_payments(State(state): State<AppState>, headers: HeaderMap) -> ApiResponse {
    if !is_admin_request(&state.cfg, &headers) {
        return unauthorized();
    }
    match list_recent_qris_payments(&state.db, 20) {
        Ok(rows) => ok_json(json!({
            "payments": rows.iter().map(qris_row_json).collect::<Vec<_>>(),
        })),
        Err(e) => err_json(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()),
    }
}

const QRIS_TEST_MIN: i64 = 100;
const QRIS_TEST_MAX: i64 = 100_000_000;

/// GET /api/admin/payments/qris/test?amount= — buat QR tes sungguhan.
/// QR bisa di-scan dari e-wallet untuk melihat nama merchant + nominal
/// sebelum bayar (kedaluwarsa 5 menit).
pub async fn qris_test(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(q): Query<HashMap<String, String>>,
) -> ApiResponse {
    if !is_admin_request(&state.cfg, &headers) {
        return unauthorized();
    }
    let static_qris = match get_gopay_static_qris(&state.db) {
        Ok(Some(s)) => s,
        Ok(None) => {
            return err_json(
                StatusCode::BAD_REQUEST,
                "QRIS GoPay belum dipasang — tempel dulu string QRIS statis dari GoBiz.",
            )
        }
        Err(e) => return err_json(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()),
    };
    let amount: i64 = q
        .get("amount")
        .and_then(|s| s.parse().ok())
        .unwrap_or(1500);
    if !(QRIS_TEST_MIN..=QRIS_TEST_MAX).contains(&amount) {
        return err_json(
            StatusCode::BAD_REQUEST,
            &format!(
                "Nominal harus bilangan bulat {QRIS_TEST_MIN}..{QRIS_TEST_MAX} rupiah"
            ),
        );
    }
    let reference = format!(
        "ADMIN-TEST-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis())
            .unwrap_or(0)
    );
    let created = match create_qris_payment(&state.db, &reference, amount, &static_qris, None) {
        Ok(c) => c,
        Err(e) => return err_json(StatusCode::BAD_REQUEST, &e.to_string()),
    };
    // Kartu standee komposit (fallback QR polos kalau template tidak ada).
    match render_qris_card_or_plain(&created.qr_string) {
        Ok(img) => ok_json(json!({
            "qr_png_base64": base64::Engine::encode(&base64::engine::general_purpose::STANDARD, &img.png),
            "qr_string": created.qr_string,
            "unique_amount": created.payment.unique_amount_idr,
            "expires_at": created.payment.expires_at,
            "reference": created.payment.reference,
            "amount_idr": created.payment.amount_idr,
        })),
        Err(e) => err_json(StatusCode::BAD_REQUEST, &e.to_string()),
    }
}

// ---------------------------------------------------------------------------
// Admin: GoBiz / GoPay (jalur DORMANT — OTP diblokir server Gojek)
// ---------------------------------------------------------------------------

fn gopay_error_response(err: &GopayError) -> ApiResponse {
    let status = if err.code == GopayErrorCode::AuthRequired {
        StatusCode::UNAUTHORIZED
    } else {
        StatusCode::BAD_REQUEST
    };
    (
        status,
        Json(json!({ "ok": false, "error": err.message, "code": err.code.as_str() })),
    )
}

fn gopay_client(state: &AppState) -> &GopayClient {
    &state.gopay
}

/// GET /api/admin/payments/gopay/session — status sesi (TANPA token).
pub async fn gopay_session(State(state): State<AppState>, headers: HeaderMap) -> ApiResponse {
    if !is_admin_request(&state.cfg, &headers) {
        return unauthorized();
    }
    match gopay_client(&state).session_status() {
        Ok(s) => ok_json(json!({
            "ok": true,
            "authenticated": s.authenticated,
            "phone": s.phone,
            "expiresAt": s.expires_at,
            "merchantId": s.merchant_id,
            "defaultPhone": if state.cfg.gobiz_phone.is_empty() { Value::Null } else { Value::String(state.cfg.gobiz_phone.clone()) },
        })),
        Err(e) => gopay_error_response(&e),
    }
}

/// POST /api/admin/payments/gopay/otp — minta OTP ke nomor HP merchant.
/// otp_token disimpan server-side (settings) — TIDAK dikembalikan ke client.
pub async fn gopay_otp(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Bytes,
) -> ApiResponse {
    if !is_admin_request(&state.cfg, &headers) {
        return unauthorized();
    }
    let v: Value = match serde_json::from_slice(&body) {
        Ok(v) => v,
        Err(_) => return err_json(StatusCode::BAD_REQUEST, "Body harus JSON { phone }"),
    };
    let phone = v
        .get("phone")
        .and_then(|x| x.as_str())
        .unwrap_or("")
        .trim()
        .to_string();
    match gopay_client(&state)
        .request_otp(if phone.is_empty() { None } else { Some(phone.as_str()) })
        .await
    {
        Ok(r) => ok_json(json!({ "ok": true, "phone": r.phone, "sentAt": r.sent_at })),
        Err(e) => gopay_error_response(&e),
    }
}

/// POST /api/admin/payments/gopay/verify — verifikasi OTP -> simpan sesi.
pub async fn gopay_verify(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Bytes,
) -> ApiResponse {
    if !is_admin_request(&state.cfg, &headers) {
        return unauthorized();
    }
    let v: Value = match serde_json::from_slice(&body) {
        Ok(v) => v,
        Err(_) => return err_json(StatusCode::BAD_REQUEST, "Body harus JSON { otp }"),
    };
    let otp = v
        .get("otp")
        .and_then(|x| x.as_str())
        .unwrap_or("")
        .to_string();
    let client = gopay_client(&state);
    match client.verify_otp(&otp).await {
        Ok(_) => match client.session_status() {
            Ok(s) => ok_json(json!({
                "ok": true,
                "session": {
                    "authenticated": s.authenticated,
                    "phone": s.phone,
                    "expiresAt": s.expires_at,
                    "merchantId": s.merchant_id,
                },
            })),
            Err(e) => gopay_error_response(&e),
        },
        Err(e) => gopay_error_response(&e),
    }
}

/// POST /api/admin/payments/gopay/poll — jalankan satu tick polling feed
/// secara manual. Dipakai tombol "Cek pembayaran masuk" + debugging.
pub async fn gopay_poll(State(state): State<AppState>, headers: HeaderMap) -> ApiResponse {
    if !is_admin_request(&state.cfg, &headers) {
        return unauthorized();
    }
    let result =
        tick_qris_payments(&state.db, &state.gopay, state.settlement.as_ref()).await;
    let code = result.code.as_ref().map(|c| match c {
        QrisPollCode::AuthRequired => "auth_required",
        QrisPollCode::FeedError => "feed_error",
    });
    let body = json!({
        "ok": result.ok,
        "checked": result.checked,
        "matched": result.matched,
        "expired": result.expired,
        "error": result.error,
        "code": code,
    });
    if result.ok {
        ok_json(body)
    } else {
        (StatusCode::UNAUTHORIZED, Json(body))
    }
}

/// GET /api/admin/payments/gopay/merchant — profil merchant + cocokkan
/// QRIS statis outlet.
pub async fn gopay_merchant(State(state): State<AppState>, headers: HeaderMap) -> ApiResponse {
    if !is_admin_request(&state.cfg, &headers) {
        return unauthorized();
    }
    match gopay_client(&state).get_merchant_profile().await {
        Ok(r) => ok_json(json!({
            "ok": true,
            "profile": {
                "id": r.profile.id,
                "merchantName": r.profile.merchant_name,
                "outletName": r.profile.outlet_name,
                "phone": r.profile.phone,
                "email": r.profile.email,
                "outlets": r.profile.outlets.iter().map(|o| json!({
                    "popId": o.pop_id,
                    "name": o.name,
                    "status": o.status,
                    "receiverId": o.receiver_id,
                    "qrString": o.qr_string,
                })).collect::<Vec<_>>(),
            },
            "staticQrisMatches": r.static_qris_matches,
            "warnings": r.warnings,
        })),
        Err(e) => gopay_error_response(&e),
    }
}

/// POST /api/admin/payments/gopay/logout — keluar (hapus sesi tersimpan).
/// Poller berhenti melunasi sampai login OTP lagi.
pub async fn gopay_logout(State(state): State<AppState>, headers: HeaderMap) -> ApiResponse {
    if !is_admin_request(&state.cfg, &headers) {
        return unauthorized();
    }
    let _ = gopay_client(&state).clear_session();
    ok_json(json!({ "ok": true }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use crate::db::{self, Db};
    use crate::lib::orders::{create_store_order, get_store_order, CreateStoreOrderInput};
    use crate::lib::payments::gopay::{
        GopayClient, GopayClientOptions, GopayError, GopayErrorCode, GopayHttp,
        GopaySession, HttpResponse,
    };
    use crate::lib::payments::midtrans::create_midtrans_payment;
    use crate::lib::payments::qris::{create_qris_payment, serialize_qris_payload, QrisTlv};
    use crate::lib::payments::qris_settings::set_gopay_static_qris;
    use crate::lib::wallet::{create_topup, ensure_user, get_wallet, EnsureUserInput};
    use crate::web::auth::{hash_password, ADMIN_COOKIE};
    use crate::web::settlement::WebSettlement;
    use sha2::{Digest, Sha512};
    use std::future::Future;
    use std::pin::Pin;
    use std::sync::{Arc, Mutex};

    fn test_db() -> Db {
        db::open(":memory:").expect("db tes")
    }

    fn test_cfg() -> Config {
        let mut cfg = Config::from_env().unwrap();
        cfg.admin_password = "test-admin-pw".to_string();
        cfg.midtrans_server_key = "TEST-SERVER-KEY".to_string();
        cfg.telegram_bot_token = String::new();
        cfg.telegram_news_bot_token = String::new();
        cfg.telegram_channel_id = String::new();
        // Fulfill yang di-spawn webhook TIDAK BOLEH menyentuh Sondev asli:
        // arahkan ke port lokal yang langsung refused.
        cfg.sondev_base_url = "http://127.0.0.1:9".to_string();
        cfg.sondev_api_key = String::new();
        cfg.sondev_api_secret = String::new();
        cfg.gobiz_phone = String::new();
        cfg
    }

    fn admin_headers() -> HeaderMap {
        let mut h = HeaderMap::new();
        h.insert(
            axum::http::header::COOKIE,
            format!("{ADMIN_COOKIE}={}", hash_password("test-admin-pw"))
                .parse()
                .unwrap(),
        );
        h
    }

    fn static_payload() -> String {
        serialize_qris_payload(&[
            QrisTlv { tag: "00".into(), value: "01".into() },
            QrisTlv { tag: "01".into(), value: "11".into() },
            QrisTlv {
                tag: "26".into(),
                value: "0014ID.CO.QRIS.WWW011693600123456789010215ID1234567890123".into(),
            },
            QrisTlv { tag: "52".into(), value: "4814".into() },
            QrisTlv { tag: "53".into(), value: "360".into() },
            QrisTlv { tag: "58".into(), value: "ID".into() },
            QrisTlv { tag: "59".into(), value: "TOKO TEST QRIS".into() },
            QrisTlv { tag: "60".into(), value: "JAKARTA".into() },
        ])
        .unwrap()
    }

    /// Transport HTTP palsu ala harness TS: GoID + GoBiz + feed.
    struct FakeGopayHttp {
        feed: Arc<Mutex<Vec<serde_json::Value>>>,
        static_qris: String,
    }

    impl GopayHttp for FakeGopayHttp {
        fn request(
            &self,
            _method: &str,
            url: &str,
            _headers: Vec<(String, String)>,
            _body: Option<String>,
        ) -> Pin<Box<dyn Future<Output = Result<HttpResponse, GopayError>> + Send + '_>> {
            let url_owned = url.to_string();
            let feed = self.feed.lock().unwrap().clone();
            let static_qris = self.static_qris.clone();
            Box::pin(async move {
                let body = if url_owned.contains("goid/login/request") {
                    serde_json::json!({"success": true, "data": {"otp_token": "OTP-HARNESS"}})
                } else if url_owned.contains("goid/token") {
                    serde_json::json!({
                        "access_token": "AT-HARNESS",
                        "refresh_token": "RT-HARNESS",
                        "token_type": "Bearer",
                        "expires_in": 3600,
                    })
                } else if url_owned.contains("users/me") {
                    serde_json::json!({"user": {"id": "U-HARNESS", "merchant_id": "M-HARNESS"}})
                } else if url_owned.contains("/v1/merchants/") {
                    serde_json::json!({
                        "id": "M-HARNESS",
                        "merchant_name": "Unicodes Digital",
                        "outlet_name": "Outlet Serang",
                        "phone": "081234567890",
                        "email": "t@t.t",
                        "pops": [{
                            "pop_id": "P-1",
                            "name": "Outlet Serang",
                            "status": "active",
                            "gopay": {
                                "gopay_receiver_id": "G535109406",
                                "gopay_qr_string": "GOPAY-QR",
                                "aspi_qr_string": static_qris,
                            },
                        }],
                    })
                } else if url_owned.contains("transactions") {
                    serde_json::json!({
                        "from": 0, "size": 100, "total": feed.len(), "transactions": feed,
                    })
                } else {
                    return Err(GopayError::new(
                        GopayErrorCode::HttpError,
                        format!("FakeGopayHttp: URL tak terduga {url_owned}"),
                    ));
                };
                Ok(HttpResponse { status: 200, body })
            })
        }
    }

    fn feed_tx(id: &str, amount_idr: i64) -> serde_json::Value {
        serde_json::json!({
            "id": id,
            "order_id": null,
            "merchant_id": "M-HARNESS",
            "transaction_status": "settlement",
            "payment_type": "qris",
            "gross_amount": amount_idr * 100,
            "real_gross_amount": amount_idr * 100,
            "currency": "IDR",
            "transaction_time": chrono::Utc::now().to_rfc3339(),
            "settlement_time": null,
            "transaction_source": "qris",
        })
    }

    fn make_state(
        db: &Db,
        cfg: Config,
        feed: Vec<serde_json::Value>,
    ) -> (AppState, Arc<Mutex<Vec<serde_json::Value>>>) {
        let feed_shared = Arc::new(Mutex::new(feed));
        let fake = FakeGopayHttp {
            feed: feed_shared.clone(),
            static_qris: static_payload(),
        };
        let gopay = GopayClient::with_options(
            db.clone(),
            GopayClientOptions {
                http: Some(Arc::new(fake)),
                ..Default::default()
            },
        );
        let state = AppState {
            cfg: cfg.clone(),
            db: db.clone(),
            http: reqwest::Client::new(),
            settlement: WebSettlement::new(db.clone(), cfg).as_arc(),
            gopay: Arc::new(gopay),
        };
        (state, feed_shared)
    }

    fn save_session(state: &AppState) {
        let now_ms = chrono::Utc::now().timestamp_millis();
        state
            .gopay
            .save_session(&GopaySession {
                access_token: "AT-HARNESS".to_string(),
                refresh_token: "RT-HARNESS".to_string(),
                token_type: "Bearer".to_string(),
                expires_at: now_ms + 3_600_000,
                phone: "81234567890".to_string(),
                merchant_id: Some("M-HARNESS".to_string()),
            })
            .unwrap();
    }

    fn midtrans_sig(order_id: &str, status_code: &str, gross_amount: &str) -> String {
        hex::encode(Sha512::digest(
            format!("{order_id}{status_code}{gross_amount}TEST-SERVER-KEY").as_bytes(),
        ))
    }

    // -------------------------------------------------- webhook Midtrans ---

    #[tokio::test]
    async fn webhook_midtrans_without_config_is_500() {
        let db = test_db();
        let mut cfg = test_cfg();
        cfg.midtrans_server_key = String::new();
        let (state, _) = make_state(&db, cfg, vec![]);
        let body = Bytes::from(r#"{"order_id":"UD-1","status_code":"200","gross_amount":"2000","signature_key":"x","transaction_status":"settlement"}"#);
        let (status, _) = midtrans_webhook(State(state), body).await;
        assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
    }

    #[tokio::test]
    async fn webhook_midtrans_incomplete_body_is_400() {
        let db = test_db();
        let (state, _) = make_state(&db, test_cfg(), vec![]);
        let body = Bytes::from(r#"{"order_id":"UD-1"}"#);
        let (status, _) = midtrans_webhook(State(state), body).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn webhook_midtrans_bad_signature_is_401() {
        // S34d: signature salah -> 401.
        let db = test_db();
        let (state, _) = make_state(&db, test_cfg(), vec![]);
        let body = Bytes::from(
            serde_json::json!({
                "order_id": "UD-1",
                "status_code": "200",
                "gross_amount": "2000",
                "signature_key": "0".repeat(128),
                "transaction_status": "settlement",
            })
            .to_string(),
        );
        let (status, _) = midtrans_webhook(State(state), body).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn webhook_midtrans_good_signature_settles_topup() {
        // S34d: signature benar -> 200 + top-up lunas (tanpa spawn fulfill).
        let db = test_db();
        ensure_user(
            &db,
            EnsureUserInput {
                tg_id: 111,
                first_name: Some("Budi".to_string()),
                username: Some("budi".to_string()),
                referrer_tg_id: None,
            },
        );
        let topup = create_topup(&db, 111, 25000).unwrap();
        create_midtrans_payment(
            &db,
            &topup.id,
            25000,
            "SNAP-1",
            "https://app.sandbox.midtrans.com/snap/v1/redirection/1",
            None,
        )
        .unwrap();
        let (state, _) = make_state(&db, test_cfg(), vec![]);
        let sig = midtrans_sig(&topup.id, "200", "25000");
        let body = Bytes::from(
            serde_json::json!({
                "order_id": topup.id,
                "status_code": "200",
                "gross_amount": "25000",
                "signature_key": sig,
                "transaction_status": "settlement",
            })
            .to_string(),
        );
        let (status, json) = midtrans_webhook(State(state), body).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(json.0["ok"], true);
        assert_eq!(get_wallet(&db, 111).balance_idr, 25000);
    }

    #[tokio::test]
    async fn webhook_midtrans_good_signature_settles_order() {
        // S34d: signature benar -> 200 + order lunas. Fulfill di-spawn ke
        // task background (current-thread runtime: tidak jalan sebelum
        // runtime drop; sondev_base_url juga diarahkan ke 127.0.0.1:9).
        let db = test_db();
        let order = create_store_order(
            &db,
            CreateStoreOrderInput {
                id: None,
                catalog_product_id: "muse-token-1b".to_string(),
                quantity: Some(1),
                buyer_ref: Some("tg:111".to_string()),
                xiaomi_accounts: None,
                sheerid_url: None,
                target: Some("UVI0QD".to_string()),
                discount_idr: None,
                promo_code: None,
            },
        )
        .unwrap();
        create_midtrans_payment(
            &db,
            &order.id,
            2000,
            "SNAP-1",
            "https://app.sandbox.midtrans.com/snap/v1/redirection/1",
            None,
        )
        .unwrap();
        let (state, _) = make_state(&db, test_cfg(), vec![]);
        let sig = midtrans_sig(&order.id, "200", "2000");
        let body = Bytes::from(
            serde_json::json!({
                "order_id": order.id,
                "status_code": "200",
                "gross_amount": "2000",
                "signature_key": sig,
                "transaction_status": "settlement",
            })
            .to_string(),
        );
        let (status, json) = midtrans_webhook(State(state), body).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(json.0["ok"], true);
        assert_eq!(
            get_store_order(&db, &order.id).unwrap().status.as_str(),
            "paid"
        );
    }

    // ------------------------------------------------------- GoPay admin ---

    #[tokio::test]
    async fn gopay_session_requires_admin_cookie() {
        // S25: tanpa cookie -> 401.
        let db = test_db();
        let (state, _) = make_state(&db, test_cfg(), vec![]);
        let (status, _) = gopay_session(State(state), HeaderMap::new()).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn gopay_session_reports_auth_state() {
        // S25: tanpa sesi -> authenticated false; dengan sesi -> true + HP.
        let db = test_db();
        let (state, _) = make_state(&db, test_cfg(), vec![]);
        let (status, json) = gopay_session(State(state.clone()), admin_headers()).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(json.0["authenticated"], false);
        assert_eq!(json.0["phone"], serde_json::Value::Null);
        save_session(&state);
        let (status2, json2) = gopay_session(State(state), admin_headers()).await;
        assert_eq!(status2, StatusCode::OK);
        assert_eq!(json2.0["authenticated"], true);
        assert_eq!(json2.0["phone"], "81234567890");
    }

    #[tokio::test]
    async fn gopay_merchant_matches_static_qris() {
        // S25: profil merchant + QRIS statis cocok.
        let db = test_db();
        set_gopay_static_qris(&db, &static_payload()).unwrap();
        let (state, _) = make_state(&db, test_cfg(), vec![]);
        save_session(&state);
        let (status, json) = gopay_merchant(State(state), admin_headers()).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(json.0["ok"], true);
        assert_eq!(json.0["profile"]["merchantName"], "Unicodes Digital");
        assert_eq!(json.0["staticQrisMatches"], true);
    }

    #[tokio::test]
    async fn gopay_poll_without_session_is_auth_required() {
        // S25/S30: poll manual tanpa sesi -> 401 + code auth_required.
        let db = test_db();
        let order = create_store_order(
            &db,
            CreateStoreOrderInput {
                id: None,
                catalog_product_id: "muse-token-1b".to_string(),
                quantity: Some(1),
                buyer_ref: Some("tg:111".to_string()),
                xiaomi_accounts: None,
                sheerid_url: None,
                target: Some("UVI0QD".to_string()),
                discount_idr: None,
                promo_code: None,
            },
        )
        .unwrap();
        create_qris_payment(&db, &order.id, 2000, &static_payload(), None).unwrap();
        let (state, _) = make_state(&db, test_cfg(), vec![]);
        let (status, json) = gopay_poll(State(state), admin_headers()).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
        assert_eq!(json.0["ok"], false);
        assert_eq!(json.0["code"], "auth_required");
    }

    #[tokio::test]
    async fn gopay_poll_matches_feed_via_handler() {
        // S25: poll manual -> ok, match 1 (order lunas).
        let db = test_db();
        let order = create_store_order(
            &db,
            CreateStoreOrderInput {
                id: None,
                catalog_product_id: "muse-token-1b".to_string(),
                quantity: Some(1),
                buyer_ref: Some("tg:111".to_string()),
                xiaomi_accounts: None,
                sheerid_url: None,
                target: Some("UVI0QD".to_string()),
                discount_idr: None,
                promo_code: None,
            },
        )
        .unwrap();
        let created =
            create_qris_payment(&db, &order.id, 2000, &static_payload(), None).unwrap();
        let unique = created.payment.unique_amount_idr;
        let (state, _) = make_state(&db, test_cfg(), vec![feed_tx("TX-POLL-1", unique)]);
        save_session(&state);
        let (status, json) = gopay_poll(State(state), admin_headers()).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(json.0["ok"], true);
        assert_eq!(json.0["matched"], 1);
        assert_eq!(
            get_store_order(&db, &order.id).unwrap().status.as_str(),
            "paid"
        );
    }

    #[tokio::test]
    async fn gopay_otp_flow_and_token_not_leaked() {
        // S31: minta OTP berhasil, otp_token TIDAK bocor ke client.
        let db = test_db();
        let (state, _) = make_state(&db, test_cfg(), vec![]);
        let body = Bytes::from(r#"{"phone":"081234567890"}"#);
        let (status, json) = gopay_otp(State(state.clone()), admin_headers(), body).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(json.0["ok"], true);
        assert_eq!(json.0["phone"], "81234567890");
        assert!(!json.0.to_string().contains("OTP-HARNESS"));
        // Verifikasi OTP berhasil -> sesi login.
        let body2 = Bytes::from(r#"{"otp":"123456"}"#);
        let (status2, json2) = gopay_verify(State(state.clone()), admin_headers(), body2).await;
        assert_eq!(status2, StatusCode::OK);
        assert_eq!(json2.0["ok"], true);
        assert_eq!(json2.0["session"]["authenticated"], true);
        // Logout: sesi terhapus.
        let (status3, json3) = gopay_logout(State(state.clone()), admin_headers()).await;
        assert_eq!(status3, StatusCode::OK);
        assert_eq!(json3.0["ok"], true);
        let (_, json4) = gopay_session(State(state), admin_headers()).await;
        assert_eq!(json4.0["authenticated"], false);
    }

    #[tokio::test]
    async fn gopay_verify_bad_format_is_400() {
        // S31: format OTP salah ditolak 400.
        let db = test_db();
        let (state, _) = make_state(&db, test_cfg(), vec![]);
        let body = Bytes::from(r#"{"otp":"ab"}"#);
        let (status, _) = gopay_verify(State(state), admin_headers(), body).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn gopay_otp_requires_admin_cookie() {
        let db = test_db();
        let (state, _) = make_state(&db, test_cfg(), vec![]);
        let body = Bytes::from(r#"{"phone":"081234567890"}"#);
        let (status, _) = gopay_otp(State(state), HeaderMap::new(), body).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
    }

    // ------------------------------------------------- Gap 3: POST /api/orders

    /// Jalankan mock server lokal (127.0.0.1, port ephemeral).
    async fn start_mock_server(
        router: axum::Router,
    ) -> (String, tokio::task::JoinHandle<()>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind mock server");
        let base = format!("http://127.0.0.1:{}", listener.local_addr().unwrap().port());
        let handle = tokio::spawn(async move {
            axum::serve(listener, router).await.unwrap();
        });
        (base, handle)
    }

    /// Mock Sondev lokal: endpoint yang dipakai alur create_order —
    /// GET /profile (saldo) + GET /products (harga grosir).
    fn mock_sondev_router() -> axum::Router {
        axum::Router::new()
            .route(
                "/profile",
                axum::routing::get(|| async {
                    axum::Json(serde_json::json!({
                        "balance_idr": 100000,
                        "balance_usdt": 0,
                    }))
                }),
            )
            .route(
                "/products",
                axum::routing::get(|| async {
                    axum::Json(serde_json::json!([{
                        "code": "muse_token",
                        "name": "Muse AI Token",
                        "price_idr": 800,
                        "available_stock": 30,
                    }]))
                }),
            )
    }

    /// Mock Midtrans Snap lokal: POST /snap/v1/transactions.
    fn mock_snap_router() -> axum::Router {
        axum::Router::new().route(
            "/snap/v1/transactions",
            axum::routing::post(|body: axum::body::Bytes| async move {
                let v: serde_json::Value =
                    serde_json::from_slice(&body).expect("body snap harus JSON");
                assert_eq!(v["transaction_details"]["gross_amount"], 2000);
                assert_eq!(v["enabled_payments"], serde_json::json!(["qris"]));
                axum::Json(serde_json::json!({
                    "token": "SNAP-TEST-TOKEN",
                    "redirect_url": "https://app.sandbox.midtrans.com/snap/v1/redirection/test",
                }))
            }),
        )
    }

    // Env lock sengaja dipegang selama semua await: env var global harus
    // tetap menunjuk mock selama tes berjalan (lihat MIDTRANS_ENV_LOCK).
    #[allow(clippy::await_holding_lock)]
    #[tokio::test]
    async fn create_order_end_to_end_with_mock_sondev_and_snap() {
        // Gap 3 (S28): POST /api/orders level HTTP melawan mock Sondev +
        // mock Snap — order valid membuat store order di DB + mengembalikan
        // payload pembayaran; GET /api/orders/{id} mengembalikannya lagi.
        // Port 3224 + file DB sendiri (beda dari web_smoke 3223).
        use crate::lib::payments::midtrans::MIDTRANS_ENV_LOCK;
        let _env_guard = MIDTRANS_ENV_LOCK.lock().unwrap();
        let db_path = "/tmp/web-orders-gap3-test.db";
        let _ = std::fs::remove_file(db_path);
        let _ = std::fs::remove_file(format!("{db_path}-wal"));
        let _ = std::fs::remove_file(format!("{db_path}-shm"));

        let (sondev_base, sondev_server) = start_mock_server(mock_sondev_router()).await;
        let (midtrans_base, midtrans_server) = start_mock_server(mock_snap_router()).await;
        // Seam Gap 2: arahkan Snap/status Midtrans ke mock lokal.
        std::env::set_var("MIDTRANS_SNAP_BASE_URL", &midtrans_base);
        std::env::set_var("MIDTRANS_API_BASE_URL", &midtrans_base);

        let mut cfg = test_cfg();
        cfg.sondev_base_url = sondev_base;
        cfg.port = 3224;
        cfg.database_path = db_path.to_string();
        let db = db::open(&cfg.database_path).expect("db test");
        let server = tokio::spawn(crate::web::serve(cfg, db.clone()));
        tokio::time::sleep(std::time::Duration::from_millis(800)).await;
        let client = reqwest::Client::new();
        let base = "http://127.0.0.1:3224";

        let resp = client
            .post(format!("{base}/api/orders"))
            .json(&serde_json::json!({
                "productId": "muse-token-1b",
                "quantity": 1,
                "target": "UVI0QD",
                "buyerRef": "tg:111",
            }))
            .send()
            .await
            .expect("POST /api/orders");
        assert_eq!(resp.status(), 201);
        let body: serde_json::Value = resp.json().await.unwrap();
        assert_eq!(body["ok"], true);
        let order_id = body["order"]["id"]
            .as_str()
            .expect("order id")
            .to_string();
        assert!(
            order_id.starts_with("UD-"),
            "order id harus UD-...: {order_id}"
        );
        assert_eq!(body["order"]["status"], "awaiting_payment");
        assert_eq!(body["order"]["totalIdr"], 2000);
        assert_eq!(body["payment"]["snapToken"], "SNAP-TEST-TOKEN");
        assert_eq!(
            body["payment"]["redirectUrl"],
            "https://app.sandbox.midtrans.com/snap/v1/redirection/test"
        );
        assert_eq!(body["payment"]["reference"], order_id);

        // Order tersimpan di DB.
        let stored = get_store_order(&db, &order_id).expect("order harus ada di DB");
        assert_eq!(stored.status.as_str(), "awaiting_payment");
        assert_eq!(stored.retail_total_idr, 2000);
        assert_eq!(stored.catalog_product_id, "muse-token-1b");

        // GET /api/orders/{id} mengembalikan order yang sama.
        let got: serde_json::Value = client
            .get(format!("{base}/api/orders/{order_id}"))
            .send()
            .await
            .expect("GET /api/orders/{id}")
            .json()
            .await
            .unwrap();
        assert_eq!(got["ok"], true);
        assert_eq!(got["order"]["id"], order_id);
        assert_eq!(got["order"]["status"], "awaiting_payment");
        assert_eq!(got["order"]["totalIdr"], 2000);

        server.abort();
        sondev_server.abort();
        midtrans_server.abort();
        std::env::remove_var("MIDTRANS_SNAP_BASE_URL");
        std::env::remove_var("MIDTRANS_API_BASE_URL");
        let _ = std::fs::remove_file(db_path);
        let _ = std::fs::remove_file(format!("{db_path}-wal"));
        let _ = std::fs::remove_file(format!("{db_path}-shm"));
    }
}
