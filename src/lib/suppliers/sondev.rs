//! Adapter Sondev — buat Muse.
//! Port dari `src/lib/suppliers/sondev.ts` (versi TS).
//! Docs v1.2: `~/workspace/user/files/SONDEV_RESELLER_API_EN.md`
//! (juga template resmi sondev_reseller_bot.py, Aiogram 3, API v1.1).
//!
//! Auth: API key + HMAC-SHA256. Setiap request kirim:
//!   `X-API-KEY`, `X-TIMESTAMP` (detik Unix, ±300 detik dari jam server),
//!   `X-SIGNATURE` = hex HMAC-SHA256(secret, timestamp + "\n" + METHOD
//!   + "\n" + path + "\n" + body)
//!   - path = path LENGKAP termasuk "/api/v1" (mis. "/api/v1/profile"),
//!     TANPA query string (query tidak ikut signature!)
//!   - body JSON harus compact tanpa spasi
//!   - body kosong untuk GET/DELETE
//!
//! KRITIS (bug yang pernah terjadi, diperbaiki commit `bbc7b1f`):
//! server menandatangani path LENGKAP ("/api/v1/profile"), BUKAN path
//! relatif ("/profile"). Dulu adapter menandatangani path relatif saja
//! -> semua endpoint signed ditolak INVALID_SIGNATURE. Jangan ulangi.
//!
//! Bentuk respons (envelope): sukses `{ status: "success", request_id,
//! data: ... }`, gagal `{ status: "error", error, message, request_id,
//! retry_after? }`. Harga grosir = price_idr / price_usd di /products.
//! Saldo = balance_idr / balance_usdt (flat) di /profile. Stok =
//! available_stock ("UNLIMITED" atau angka) — untuk produk muse_service
//! JANGAN jadikan patokan jumlah; pakai sellable_quantity / available_now
//! dari /stock (keduanya bisa null kalau pool tidak terbaca — null = tidak
//! diketahui, BUKAN 0).
//!
//! Semua pemanggilan di sini HANYA dari server — jangan pernah dari kode
//! client/browser. Secret dibaca dari env via [`crate::config::Config`],
//! tidak pernah di-hardcode dan tidak pernah ditulis ke log.

use std::collections::VecDeque;
use std::sync::Arc;
use std::time::{Duration, Instant};

use chrono::Utc;
use hmac::{Hmac, Mac};
use rand::Rng;
use reqwest::Method;
use serde_json::{json, Value};
use sha2::Sha256;
use tokio::sync::Mutex;

use crate::config::Config;

use super::types::{
    json_f64, json_i64, json_str, CreateOrderInput, SupplierAdapter, SupplierBalance,
    SupplierId, SupplierOrderResult, SupplierOrderStatus, SupplierProduct,
};

/// TTL cache katalog — docs: cache 60–300 detik di sisi kita (TS: 120 detik).
pub const SONDEV_PRODUCTS_CACHE_TTL: Duration = Duration::from_secs(120);

/// Limit server: 120 request/menit per reseller (docs §7).
/// Adapter menegakkannya di sisi klien supaya tidak kena 429.
pub const SONDEV_RATE_LIMIT_PER_MIN: usize = 120;

/// Timeout per request (TS: `AbortSignal.timeout(25000)`).
const REQUEST_TIMEOUT: Duration = Duration::from_secs(25);
/// Maksimum percobaan per request (TS: 4).
const MAX_ATTEMPTS: u32 = 4;

/// User-Agent wajib diisi — tanpanya bisa kena block Cloudflare 1010.
const USER_AGENT: &str = "unicodes-digital/0.1";

/// Ambil path dari base URL, mis. "https://api.sondevs.tech/api/v1" -> "/api/v1".
/// Dipakai untuk membangun path LENGKAP yang ditandatangani.
fn base_path_of(base_url: &str) -> String {
    let without_scheme = base_url.split("://").nth(1).unwrap_or(base_url);
    let path = without_scheme
        .split('/')
        .skip(1)
        .collect::<Vec<_>>()
        .join("/");
    if path.is_empty() {
        String::new()
    } else {
        format!("/{path}").trim_end_matches('/').to_string()
    }
}

/// `X-SIGNATURE` = hex HMAC-SHA256(secret, timestamp + "\n" + METHOD
/// + "\n" + path + "\n" + body). Path = path LENGKAP termasuk "/api/v1".
pub fn sign(secret: &str, timestamp: &str, method: &str, path: &str, body: &str) -> String {
    let mut mac = Hmac::<Sha256>::new_from_slice(secret.as_bytes())
        .expect("HMAC-SHA256 menerima kunci dengan panjang berapa pun");
    mac.update(format!("{timestamp}\n{method}\n{path}\n{body}").as_bytes());
    hex::encode(mac.finalize().into_bytes())
}

/// Backoff eksponensial 2/4/8/16 detik dengan jitter ±30%, maks 30 detik (docs).
fn backoff(attempt: u32) -> Duration {
    let base_secs = (2u64.pow(attempt)).min(30) as f64;
    let jitter: f64 = rand::thread_rng().gen_range(0.7..1.3);
    Duration::from_secs_f64(base_secs * jitter)
}

/// Error bisnis — JANGAN di-retry (4xx kecuali 429 RATE_LIMIT_MINUTE
/// & 409 REPLAY_DETECTED). Dipakai internal sebagai penanda alur retry.
struct NonRetryable(anyhow::Error);

/// Cache produk Sondev: (diambil kapan, daftar produk).
type ProductsCache = Arc<Mutex<Option<(Instant, Vec<SupplierProduct>)>>>;

#[derive(Debug, Clone)]
pub struct SondevAdapter {
    client: reqwest::Client,
    api_key: String,
    api_secret: String,
    base_url: String,
    /// Path dasar dari base URL (mis. "/api/v1") — inilah yang ikut signature.
    base_path: String,
    products_cache: ProductsCache,
    /// Sliding window 60 detik untuk limit 120 req/menit (sisi klien).
    rate_window: Arc<Mutex<VecDeque<Instant>>>,
}

impl SondevAdapter {
    pub fn new(api_key: String, api_secret: String, base_url: String) -> anyhow::Result<Self> {
        let base_url = base_url.trim_end_matches('/').to_string();
        let base_path = base_path_of(&base_url);
        let client = reqwest::Client::builder()
            .timeout(REQUEST_TIMEOUT)
            .user_agent(USER_AGENT)
            .build()?;
        Ok(Self {
            client,
            api_key,
            api_secret,
            base_url,
            base_path,
            products_cache: Arc::new(Mutex::new(None)),
            rate_window: Arc::new(Mutex::new(VecDeque::new())),
        })
    }

    /// Konstruktor dari [`Config`] — baca `SONDEV_API_KEY`,
    /// `SONDEV_API_SECRET`, `SONDEV_BASE_URL` (default `https://api.sondevs.tech/api/v1`).
    pub fn from_config(cfg: &Config) -> anyhow::Result<Self> {
        Self::new(
            cfg.sondev_api_key.clone(),
            cfg.sondev_api_secret.clone(),
            cfg.sondev_base_url.clone(),
        )
    }

    /// Tunggu giliran di sliding window 120 req / 60 detik.
    async fn acquire_rate_slot(&self) {
        loop {
            let wait = {
                let mut q = self.rate_window.lock().await;
                let now = Instant::now();
                while q
                    .front()
                    .map(|t| now.duration_since(*t) > Duration::from_secs(60))
                    .unwrap_or(false)
                {
                    q.pop_front();
                }
                if q.len() < SONDEV_RATE_LIMIT_PER_MIN {
                    q.push_back(now);
                    None
                } else {
                    q.front()
                        .map(|oldest| (*oldest + Duration::from_secs(60)).saturating_duration_since(now))
                }
            };
            match wait {
                None => break,
                Some(d) => tokio::time::sleep(d).await,
            }
        }
    }

    /// Request signed ke Sondev dengan retry seperti TS (`call`).
    ///
    /// Mengembalikan isi envelope (`data`) kalau ada, atau seluruh body.
    /// `path_with_query` ditulis relatif terhadap base URL, mis.
    /// `"/profile"` atau `"/stock?code=muse_token"`.
    async fn call(
        &self,
        path_with_query: &str,
        method: Method,
        body: Option<&Value>,
    ) -> anyhow::Result<Value> {
        let body_str = match body {
            Some(b) => serde_json::to_string(b)?, // compact, tanpa spasi
            None => String::new(),                // body kosong untuk GET/DELETE
        };
        // PENTING: yang ditandatangani adalah path LENGKAP ("/api/v1/..."),
        // TANPA query string (query tidak ikut signature).
        let sign_path_rel = path_with_query.split('?').next().unwrap_or(path_with_query);
        let sign_path = format!("{}{}", self.base_path, sign_path_rel);
        let url = format!("{}{}", self.base_url, path_with_query);
        let method_str = method.as_str().to_string();

        let mut last_error: Option<anyhow::Error> = None;
        for attempt in 1..=MAX_ATTEMPTS {
            // Timestamp + signature BARU tiap percobaan —
            // pakai ulang = 409 REPLAY_DETECTED.
            self.acquire_rate_slot().await;
            let timestamp = Utc::now().timestamp().to_string();
            let signature = sign(
                &self.api_secret,
                &timestamp,
                &method_str,
                &sign_path,
                &body_str,
            );
            let req = self
                .client
                .request(method.clone(), &url)
                .header("X-API-KEY", &self.api_key)
                .header("X-TIMESTAMP", &timestamp)
                .header("X-SIGNATURE", &signature)
                .header("Content-Type", "application/json")
                .header("Accept", "application/json");
            let req = if body.is_some() {
                req.body(body_str.clone())
            } else {
                req
            };
            match req.send().await {
                Ok(res) => {
                    let status = res.status();
                    let request_id_header = res
                        .headers()
                        .get("X-Request-Id")
                        .and_then(|v| v.to_str().ok())
                        .unwrap_or("")
                        .to_string();
                    let retry_after_header: u64 = res
                        .headers()
                        .get("Retry-After")
                        .and_then(|v| v.to_str().ok())
                        .and_then(|v| v.parse().ok())
                        .unwrap_or(0);
                    let text = res.text().await.unwrap_or_default();
                    let data: Value =
                        serde_json::from_str(&text).unwrap_or(Value::Object(Default::default()));
                    if status.is_success() {
                        // Semua respons dibungkus envelope `{ data: ... }`.
                        if let Some(inner) = data.get("data") {
                            if !inner.is_null() {
                                return Ok(inner.clone());
                            }
                        }
                        return Ok(data);
                    }
                    let code = json_str(&data, "error")
                        .or_else(|| json_str(&data, "code"))
                        .unwrap_or_else(|| format!("HTTP_{}", status.as_u16()));
                    let msg = json_str(&data, "message")
                        .unwrap_or_else(|| "Sondev request gagal".to_string());
                    let request_id = json_str(&data, "request_id")
                        .filter(|s| !s.is_empty())
                        .unwrap_or(request_id_header);
                    let err_msg = if request_id.is_empty() {
                        format!("{code}: {msg}")
                    } else {
                        format!("{code}: {msg} (req {request_id})")
                    };
                    // Retryable: 429 RATE_LIMIT_MINUTE, 409 REPLAY_DETECTED
                    // (dengan signature baru), dan semua 5xx.
                    // TIDAK retry: DAILY_LIMIT, 402, 400, 401, 403, 409 SOLD_OUT.
                    let retryable = status.as_u16() >= 500
                        || code == "REPLAY_DETECTED"
                        || (status.as_u16() == 429 && code != "DAILY_LIMIT");
                    if !retryable || attempt == MAX_ATTEMPTS {
                        return Err(NonRetryable(anyhow::anyhow!(err_msg)).0);
                    }
                    // Hormati Retry-After (header dulu, lalu field retry_after di body).
                    let retry_after_body: u64 =
                        json_i64(&data, "retry_after").unwrap_or(0).max(0) as u64;
                    let wait_sec = retry_after_header.max(retry_after_body);
                    let wait = if wait_sec > 0 {
                        Duration::from_secs(wait_sec.min(30))
                    } else {
                        backoff(attempt)
                    };
                    last_error = Some(anyhow::anyhow!(err_msg));
                    tokio::time::sleep(wait).await;
                }
                Err(err) => {
                    // Network error / timeout: aman diulang (belum tentu sampai ke server).
                    if attempt == MAX_ATTEMPTS {
                        return Err(last_error.unwrap_or_else(|| anyhow::anyhow!(err)));
                    }
                    last_error = Some(anyhow::anyhow!(err));
                    tokio::time::sleep(backoff(attempt)).await;
                }
            }
        }
        Err(last_error.unwrap_or_else(|| anyhow::anyhow!("Sondev request gagal")))
    }
}

fn map_status(status: &Value) -> SupplierOrderStatus {
    match status.as_str().unwrap_or("").to_uppercase().as_str() {
        "COMPLETED" | "DELIVERED" | "SUCCESS" => SupplierOrderStatus::Success,
        // REFUNDED = gagal + saldo grosir dikembalikan; PARTIAL_DELIVERED =
        // gagal sebagian — keduanya diperlakukan sebagai failed di sisi pembeli.
        "FAILED" | "REFUNDED" | "PARTIAL_DELIVERED" => SupplierOrderStatus::Failed,
        "CANCELLED" => SupplierOrderStatus::Cancelled,
        "PROCESSING" | "PENDING" => SupplierOrderStatus::Processing,
        _ => SupplierOrderStatus::Unknown,
    }
}

/// Cari URL pertama di dalam sebuah string ("email|https://..." dsb).
fn first_url(s: &str) -> Option<String> {
    for proto in ["https://", "http://"] {
        if let Some(idx) = s.find(proto) {
            let rest = &s[idx..];
            let end = rest.find(char::is_whitespace).unwrap_or(rest.len());
            return Some(rest[..end].to_string());
        }
    }
    None
}

/// Ambil barang hasil fulfillment dari respons order.
/// Format resmi (docs): delivered_data.items = array string mentah —
/// "email|mailbox_link", URL aktivasi mentah, "email:password", atau teks
/// bebas; token/kode ada di delivered_data.code (+ redeemed).
/// Bentuk lama (delivered_items / items) tetap didukung sebagai fallback.
fn extract_delivered(order: &Value) -> (Vec<String>, Option<String>) {
    let mut items: Vec<String> = Vec::new();
    let delivered_data = order.get("delivered_data").cloned().unwrap_or(Value::Null);
    let raw_items = delivered_data
        .get("items")
        .or_else(|| order.get("delivered_items"))
        .or_else(|| order.get("items"));
    if let Some(Value::Array(arr)) = raw_items {
        for it in arr {
            match it {
                Value::String(s) if !s.trim().is_empty() => items.push(s.clone()),
                Value::String(_) => {}
                other => items.push(other.to_string()),
            }
        }
    }
    if let Some(code) = json_str(&delivered_data, "code") {
        if !code.trim().is_empty() {
            items.push(code);
        }
    }
    let mut result_link = json_str(order, "result_link");
    if result_link.is_none() {
        // Cari URL pertama di dalam items (mis. mailbox_link dari "email|link").
        for it in &items {
            if let Some(u) = first_url(it) {
                result_link = Some(u);
                break;
            }
        }
    }
    (items, result_link)
}

fn to_order_result(body: &Value, fallback_id: &str) -> SupplierOrderResult {
    let order = body.get("order").unwrap_or(body);
    // ALREADY_EXISTS = ref_id sudah dipakai — body adalah order aslinya,
    // jadi baca hasil dari delivery_status-nya, bukan dari status itu sendiri.
    let raw_status = order
        .get("status")
        .and_then(|s| s.as_str())
        .unwrap_or("")
        .to_uppercase();
    let null = Value::Null;
    let effective_status = if raw_status == "ALREADY_EXISTS" {
        order
            .get("delivery_status")
            .or_else(|| order.get("status"))
            .unwrap_or(&null)
    } else {
        order
            .get("status")
            .or_else(|| order.get("delivery_status"))
            .unwrap_or(&null)
    };
    let (items, result_link) = extract_delivered(order);
    let error_message = match json_str(order, "refund_note") {
        Some(s) if !s.is_empty() => Some(s),
        _ => json_str(order, "message"),
    };
    SupplierOrderResult {
        supplier_order_id: json_str(order, "ref_id")
            .or_else(|| json_str(order, "id"))
            .unwrap_or_else(|| fallback_id.to_string()),
        status: map_status(effective_status),
        delivered_items: items,
        result_link,
        error_code: json_str(body, "code").or_else(|| json_str(body, "error")),
        error_message,
        raw: body.clone(),
    }
}

impl SupplierAdapter for SondevAdapter {
    fn id(&self) -> SupplierId {
        SupplierId::Sondev
    }

    fn is_configured(&self) -> bool {
        !self.api_key.is_empty() && !self.api_secret.is_empty()
    }

    async fn get_balance(&self) -> anyhow::Result<SupplierBalance> {
        // Docs: /profile mengembalikan balance_idr / balance_usdt FLAT (bukan nested).
        let body = self.call("/profile", Method::GET, None).await?;
        Ok(SupplierBalance {
            idr: json_f64(&body, "balance_idr").unwrap_or(0.0),
            // USDT dipakai dulu baru IDR (docs) — simpan di kolom usd.
            usd: json_f64(&body, "balance_usdt").unwrap_or(0.0),
            credit: 0.0,
            raw: body,
        })
    }

    async fn list_products(&self) -> anyhow::Result<Vec<SupplierProduct>> {
        // Docs: cache katalog 60–300 detik di sisi kita.
        {
            let cache = self.products_cache.lock().await;
            if let Some((at, items)) = cache.as_ref() {
                if at.elapsed() < SONDEV_PRODUCTS_CACHE_TTL {
                    return Ok(items.clone());
                }
            }
        }
        let body = self.call("/products", Method::GET, None).await?;
        let arr: Vec<Value> = if let Some(a) = body.as_array() {
            a.clone()
        } else {
            body.get("products")
                .and_then(|p| p.as_array())
                .cloned()
                .unwrap_or_default()
        };
        let items: Vec<SupplierProduct> = arr
            .iter()
            .map(|p| {
                // available_stock = "UNLIMITED" (on-demand, mis. Muse) atau angka.
                let stock = json_i64(p, "available_stock").or_else(|| json_i64(p, "stock"));
                SupplierProduct {
                    code: json_str(p, "code")
                        .or_else(|| json_str(p, "product_code"))
                        .unwrap_or_default(),
                    name: json_str(p, "name")
                        .or_else(|| json_str(p, "code"))
                        .or_else(|| json_str(p, "product_code"))
                        .unwrap_or_default(),
                    wholesale_price_idr: json_f64(p, "price_idr"),
                    wholesale_price_usd: json_f64(p, "price_usd"),
                    stock,
                    min_qty: json_i64(p, "min_qty"),
                    max_qty: json_i64(p, "max_qty"),
                }
            })
            .collect();
        let mut cache = self.products_cache.lock().await;
        *cache = Some((Instant::now(), items.clone()));
        Ok(items)
    }

    async fn get_stock<'a>(
        &'a self,
        supplier_product_code: &'a str,
    ) -> anyhow::Result<Option<i64>> {
        // Docs: /stock?code=... — param-nya "code" (1–50 karakter a-z0-9_).
        // Jangan jadikan patokan available_stock untuk muse_service (string);
        // pakai sellable_quantity (= min(available_now, max_qty)) atau available_now.
        // available_now BISA null kalau pool tidak terbaca — null = tidak
        // diketahui, BUKAN 0.
        let code: String = supplier_product_code
            .chars()
            .filter(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || *c == '_')
            .take(50)
            .collect();
        let body = self
            .call(&format!("/stock?code={code}"), Method::GET, None)
            .await?;
        let arr: Vec<Value> = if let Some(a) = body.as_array() {
            a.clone()
        } else {
            body.get("data")
                .and_then(|d| d.as_array())
                .cloned()
                .unwrap_or_default()
        };
        let found = arr
            .iter()
            .find(|p| json_str(p, "code").as_deref() == Some(code.as_str()))
            .or_else(|| arr.first());
        let Some(found) = found else {
            return Ok(None);
        };
        if let Some(v) = json_i64(found, "sellable_quantity") {
            return Ok(Some(v));
        }
        if let Some(v) = json_i64(found, "available_now") {
            return Ok(Some(v));
        }
        // available_stock angka = stok akun; "UNLIMITED" = tak terbatas.
        if let Some(v) = json_i64(found, "available_stock") {
            return Ok(Some(v));
        }
        Ok(None)
    }

    async fn create_order(&self, input: CreateOrderInput) -> anyhow::Result<SupplierOrderResult> {
        let mut payload = json!({
            "ref_id": input.idempotency_key, // 1–100 karakter, unik per reseller
            "product_code": input.supplier_product_code,
            "quantity": input.quantity,
        });
        // target WAJIB untuk produk on-demand (invite code / email target) —
        // tanpa ini order ditolak 400 INVALID_ORDER.
        if let Some(target) = &input.target {
            payload
                .as_object_mut()
                .expect("payload objek JSON")
                .insert("target".to_string(), Value::String(target.clone()));
        }
        let body = self
            .call("/orders/create", Method::POST, Some(&payload))
            .await?;
        // Reuse ref_id -> supplier kembalikan order asli + ALREADY_EXISTS,
        // tanpa charge ulang.
        Ok(to_order_result(&body, &input.idempotency_key))
    }

    async fn get_order<'a>(
        &'a self,
        supplier_order_id: &'a str,
    ) -> anyhow::Result<SupplierOrderResult> {
        let body = self
            .call(
                &format!("/orders/{}", percent_encode(supplier_order_id)),
                Method::GET,
                None,
            )
            .await?;
        Ok(to_order_result(&body, supplier_order_id))
    }
}

/// Percent-encode segmen path (pengganti `encodeURIComponent` TS).
/// Karakter unreserved (A-Z a-z 0-9 - _ . ~) dibiarkan apa adanya.
pub(crate) fn percent_encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sign_matches_ts_formula() {
        // Pesan yang ditandatangani: timestamp + "\n" + METHOD + "\n" + path + "\n" + body.
        // Verifikasi independen dengan `openssl` / implementasi lain harus
        // menghasilkan hex yang sama untuk input yang sama.
        let sig = sign("secret", "1700000000", "GET", "/api/v1/profile", "");
        assert_eq!(sig.len(), 64);
        assert!(sig.chars().all(|c| c.is_ascii_hexdigit()));
        // Deterministik: input sama -> signature sama.
        assert_eq!(sig, sign("secret", "1700000000", "GET", "/api/v1/profile", ""));
        // Path relatif menghasilkan signature BERBEDA — inilah bug lama
        // (commit bbc7b1f): path yang ditandatangani harus LENGKAP.
        assert_ne!(sig, sign("secret", "1700000000", "GET", "/profile", ""));
    }

    #[test]
    fn base_path_of_works() {
        assert_eq!(base_path_of("https://api.sondevs.tech/api/v1"), "/api/v1");
        assert_eq!(base_path_of("https://api.sondevs.tech/api/v1/"), "/api/v1");
        assert_eq!(base_path_of("https://api.sondevs.tech"), "");
        assert_eq!(base_path_of("http://localhost:3000/api/v1"), "/api/v1");
    }

    #[test]
    fn map_status_works() {
        assert_eq!(map_status(&json!("DELIVERED")), SupplierOrderStatus::Success);
        assert_eq!(map_status(&json!("PROCESSING")), SupplierOrderStatus::Processing);
        assert_eq!(map_status(&json!("REFUNDED")), SupplierOrderStatus::Failed);
        assert_eq!(map_status(&json!("PARTIAL_DELIVERED")), SupplierOrderStatus::Failed);
        assert_eq!(map_status(&json!("CANCELLED")), SupplierOrderStatus::Cancelled);
        assert_eq!(map_status(&json!("NOPE")), SupplierOrderStatus::Unknown);
    }

    #[test]
    fn to_order_result_already_exists_uses_delivery_status() {
        let body = json!({
            "status": "success",
            "order": {
                "status": "ALREADY_EXISTS",
                "delivery_status": "DELIVERED",
                "ref_id": "TRX-1",
                "delivered_data": { "items": ["a|https://example.com/x"] }
            }
        });
        let r = to_order_result(&body, "TRX-1");
        assert_eq!(r.status, SupplierOrderStatus::Success);
        assert_eq!(r.supplier_order_id, "TRX-1");
        assert_eq!(r.delivered_items, vec!["a|https://example.com/x"]);
        assert_eq!(r.result_link.as_deref(), Some("https://example.com/x"));
    }

    #[test]
    fn to_order_result_delivered_harness_shape() {
        // Bentuk SONDEV_ORDER di harness TS (setelah envelope `data`
        // dibuka `call`): order DELIVERED + delivered_data items + code.
        let body = json!({
            "order": {
                "ref_id": "TEST-ORDER",
                "status": "DELIVERED",
                "delivery_status": "DELIVERED",
                "delivered_data": { "items": ["MUSE-TOKEN-TEST-1B"], "code": "TESTCODE1B" }
            }
        });
        let r = to_order_result(&body, "TEST-ORDER");
        assert_eq!(r.status, SupplierOrderStatus::Success);
        assert_eq!(r.supplier_order_id, "TEST-ORDER");
        assert_eq!(r.delivered_items, vec!["MUSE-TOKEN-TEST-1B", "TESTCODE1B"]);
        assert_eq!(r.result_link, None);
    }

    #[test]
    fn to_order_result_failed_keeps_message() {
        // S27: order UD-FAIL-* dijawab FAILED oleh stub Sondev.
        let body = json!({
            "order": {
                "ref_id": "UD-FAIL-1",
                "status": "FAILED",
                "delivery_status": "FAILED",
                "delivered_data": { "items": [] },
                "message": "Kode invite tidak valid"
            }
        });
        let r = to_order_result(&body, "UD-FAIL-1");
        assert_eq!(r.status, SupplierOrderStatus::Failed);
        assert_eq!(r.supplier_order_id, "UD-FAIL-1");
        assert!(r.delivered_items.is_empty());
        assert_eq!(r.error_message.as_deref(), Some("Kode invite tidak valid"));
    }

    #[test]
    fn percent_encode_works() {
        assert_eq!(percent_encode("UD-ABC_123"), "UD-ABC_123");
        assert_eq!(percent_encode("a b/c?d"), "a%20b%2Fc%3Fd");
        assert_eq!(percent_encode(""), "");
    }

    #[test]
    fn backoff_grows_and_caps() {
        assert!(backoff(1) >= Duration::from_millis(1400));
        assert!(backoff(1) <= Duration::from_millis(2600));
        assert!(backoff(10) <= Duration::from_secs(30) + Duration::from_millis(9000));
    }
}
