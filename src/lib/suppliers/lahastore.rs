//! Adapter LahaStore — buat AI lain (Gemini dkk).
//! Port dari `src/lib/suppliers/lahastore.ts` (versi TS).
//! Docs: <https://lahastore.up.railway.app/api/v1/docs>
//!
//! Auth: header `X-API-Key` di semua request. Key harus diaktifkan admin dulu.
//! Semua pemanggilan di sini HANYA dari server — jangan pernah panggil dari
//! kode client/browser. Kunci dibaca dari env via [`crate::config::Config`]
//! (catatan: kunci LahaStore di `.env.local` versi TS memang INVALID /
//! 401 — adapter cukup membaca env, jangan cari kunci di mana pun).
//!
//! Harga di LahaStore umumnya VND atau USD — konversi ke IDR dilakukan di
//! sisi kita (lihat [`LahaStoreAdapter::usd_to_idr`] / `vnd_to_idr`,
//! kurs dari `Config.idr_per_usd`, default 16500 — cerminan `IDR_PER_USD`
//! di `pricing.ts` sampai modul pricing Rust selesai).

use std::time::Duration;

use reqwest::Method;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::config::Config;

use super::types::{
    json_f64, json_i64, json_str, CreateOrderInput, SupplierAdapter, SupplierBalance, SupplierId,
    SupplierOrderResult, SupplierOrderStatus, SupplierProduct,
};

/// Base URL default LahaStore (sama seperti `lahastoreBaseUrl()` di TS).
/// Dipakai tes; produksi memakai `from_config` dengan base dari env.
#[allow(dead_code)]
pub const LAHASTORE_DEFAULT_BASE_URL: &str = "https://lahastore.up.railway.app/api/v1";

/// VND per USD saat dicek 2026-11-17 (cerminan `VND_PER_USD` di `pricing.ts`).
/// TODO: ambil kurs live (atau set manual tiap hari) sebelum dipakai produksi.
/// Dipakai `vnd_to_idr` (tes + referensi); jalur produksi memakai harga
/// dari /products.
#[allow(dead_code)]
pub const VND_PER_USD: f64 = 26500.0;

/// Timeout per request (TS tidak memasang timeout eksplisit di LahaStore;
/// 25 detik dipakai supaya satu supplier lambat tidak menggantung selamanya).
const REQUEST_TIMEOUT: Duration = Duration::from_secs(25);

/// Mata uang untuk order Xiaomi (`currency` di body request).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LahaCurrency {
    Usd,
    Vnd,
}

/// Input order Gemini Pro Xiaomi 3 bulan.
/// `accounts`: 1–200 baris "email|password|2FA-Base32-32-karakter
/// atau kode cadangan 8 angka". Satu akun = satu order supplier.
#[derive(Debug, Clone)]
pub struct CreateXiaomiOrderInput {
    pub client_order_id: String,
    pub currency: LahaCurrency,
    pub accounts: Vec<String>,
}

/// Hasil POST /xiaomi3/orders: satu order supplier per akun.
/// `duplicated` dipakai tes (memastikan idempotensi clientOrderId).
#[allow(dead_code)]
#[derive(Debug, Clone)]
pub struct XiaomiOrdersResult {
    pub orders: Vec<SupplierOrderResult>,
    /// `true` kalau `clientOrderId` sudah pernah dipakai — order tidak dibuat ulang.
    pub duplicated: bool,
}

/// Hasil job verifikasi SheerID — `raw` disimpan untuk debugging
/// (cerminan respons supplier); tidak dibaca jalur produksi.
#[allow(dead_code)]
#[derive(Debug, Clone)]
pub struct SheeridJobResult {
    pub status: String,
    pub result_link: Option<String>,
    pub raw: Value,
}

/// Hasil pembuatan job verifikasi SheerID — `raw` disimpan untuk
/// debugging (cerminan respons supplier); tidak dibaca jalur produksi.
#[allow(dead_code)]
#[derive(Debug, Clone)]
pub struct SheeridVerificationResult {
    pub job_id: String,
    pub status: String,
    pub raw: Value,
}

#[derive(Debug, Clone)]
pub struct LahaStoreAdapter {
    client: reqwest::Client,
    api_key: String,
    base_url: String,
    /// Kurs IDR per USD (dari env `IDR_PER_USD`, default 16500).
    /// Dipakai `usd_to_idr`/`vnd_to_idr` (tes + referensi).
    #[allow(dead_code)]
    idr_per_usd: i64,
}

impl LahaStoreAdapter {
    pub fn new(api_key: String, base_url: String, idr_per_usd: i64) -> anyhow::Result<Self> {
        let base_url = base_url.trim_end_matches('/').to_string();
        let client = reqwest::Client::builder()
            .timeout(REQUEST_TIMEOUT)
            .build()?;
        Ok(Self {
            client,
            api_key,
            base_url,
            idr_per_usd,
        })
    }

    /// Konstruktor dari [`Config`] — baca `LAHASTORE_API_KEY`,
    /// `LAHASTORE_BASE_URL` (default `LAHASTORE_DEFAULT_BASE_URL`),
    /// dan `IDR_PER_USD` (default 16500).
    pub fn from_config(cfg: &Config) -> anyhow::Result<Self> {
        Self::new(
            cfg.lahastore_api_key.clone(),
            cfg.lahastore_base_url.clone(),
            cfg.idr_per_usd,
        )
    }

    /// Konversi USD -> IDR (cerminan `usdToIdr` di `pricing.ts`).
    /// Konversi USD -> IDR (tes + referensi; jalur produksi memakai
    /// harga dari /products).
    #[allow(dead_code)]
    pub fn usd_to_idr(&self, usd: f64) -> i64 {
        (usd * self.idr_per_usd as f64).round() as i64
    }

    /// Konversi VND -> IDR (cerminan `vndToIdr` di `pricing.ts`).
    /// Konversi VND -> IDR (tes + referensi; jalur produksi memakai
    /// harga dari /products).
    #[allow(dead_code)]
    pub fn vnd_to_idr(&self, vnd: f64) -> i64 {
        ((vnd / VND_PER_USD) * self.idr_per_usd as f64).round() as i64
    }

    /// Request ke LahaStore (mirror `call` di TS): header `X-API-Key`,
    /// error kalau HTTP tidak OK atau `body.ok === false`.
    /// Seluruh body dikembalikan (tidak ada envelope `data` seperti Sondev).
    async fn call(
        &self,
        path: &str,
        method: Method,
        body: Option<&Value>,
    ) -> anyhow::Result<Value> {
        let mut req = self
            .client
            .request(method, format!("{}{}", self.base_url, path))
            .header("X-API-Key", &self.api_key)
            .header("Content-Type", "application/json");
        if let Some(b) = body {
            req = req.body(serde_json::to_string(b)?);
        }
        let res = req.send().await?;
        let status = res.status();
        let text = res.text().await.unwrap_or_default();
        let data: Value =
            serde_json::from_str(&text).unwrap_or(Value::Object(Default::default()));
        let ok_flag = data.get("ok").and_then(|v| v.as_bool()).unwrap_or(true);
        if !status.is_success() || !ok_flag {
            let code = json_str(&data, "code")
                .unwrap_or_else(|| format!("HTTP_{}", status.as_u16()));
            let msg = json_str(&data, "error")
                .unwrap_or_else(|| "LahaStore request gagal".to_string());
            return Err(anyhow::anyhow!("{code}: {msg}"));
        }
        Ok(data)
    }

    // ------------------------------------------------------------------
    // Produk Gemini — jalurnya BEDA dari order shop biasa (docs LahaStore).
    // Dipakai order service kalau catalog.product.fulfillment = "xiaomi3"
    // atau "sheerid". Harga cek dulu via /xiaomi3/services
    // & /sheerid/service_info.
    // ------------------------------------------------------------------

    /// Gemini Pro Xiaomi 3 bulan. Satu akun = satu order supplier.
    pub async fn create_xiaomi_order(
        &self,
        input: CreateXiaomiOrderInput,
    ) -> anyhow::Result<XiaomiOrdersResult> {
        let payload = json!({
            "taskType": "default",
            "currency": input.currency,
            "clientOrderId": input.client_order_id,
            "accounts": input.accounts,
        });
        let body = self
            .call("/xiaomi3/orders", Method::POST, Some(&payload))
            .await?;
        let orders = body
            .get("orders")
            .and_then(|o| o.as_array())
            .cloned()
            .unwrap_or_default()
            .iter()
            .map(|o| SupplierOrderResult {
                supplier_order_id: json_str(o, "id").unwrap_or_default(),
                status: map_status(o.get("status")),
                delivered_items: vec![],
                result_link: json_str(o, "resultLink"),
                error_code: json_str(o, "errorCode"),
                error_message: json_str(o, "errorEn"),
                raw: o.clone(),
            })
            .collect();
        Ok(XiaomiOrdersResult {
            orders,
            duplicated: body.get("duplicated").and_then(|v| v.as_bool()).unwrap_or(false),
        })
    }

    /// Cek satu order Xiaomi berdasarkan ID numerik supplier.
    pub async fn get_xiaomi_order(&self, id: i64) -> anyhow::Result<SupplierOrderResult> {
        let body = self
            .call(&format!("/xiaomi3/orders/{id}"), Method::GET, None)
            .await?;
        let Some(order) = body.get("order") else {
            return Err(anyhow::anyhow!("LahaStore: order Xiaomi tidak ditemukan"));
        };
        Ok(SupplierOrderResult {
            supplier_order_id: json_str(order, "id").unwrap_or_default(),
            status: map_status(order.get("status")),
            delivered_items: vec![],
            result_link: json_str(order, "resultLink"),
            error_code: json_str(order, "errorCode"),
            error_message: json_str(order, "errorEn"),
            raw: order.clone(),
        })
    }

    /// Cek progress + hasil job SheerID. Dipakai polling di fulfillOrder.
    pub async fn get_sheerid_job(&self, job_id: &str) -> anyhow::Result<SheeridJobResult> {
        let body = self
            .call(
                &format!("/sheerid/jobs/{}", super::sondev::percent_encode(job_id)),
                Method::GET,
                None,
            )
            .await?;
        let Some(job) = body.get("job") else {
            return Err(anyhow::anyhow!("LahaStore: job SheerID tidak ditemukan"));
        };
        Ok(SheeridJobResult {
            status: json_str(job, "status").unwrap_or_else(|| "pending".to_string()),
            result_link: json_str(job, "result_link"),
            raw: job.clone(),
        })
    }

    /// Gemini Student Pro — buat job verifikasi SheerID.
    pub async fn create_sheerid_verification(
        &self,
        sheerid_url: &str,
        client_order_id: &str,
        discount_code: Option<&str>,
    ) -> anyhow::Result<SheeridVerificationResult> {
        let mut payload = json!({
            "sheerid_url": sheerid_url,
            "client_order_id": client_order_id,
        });
        if let Some(code) = discount_code {
            payload
                .as_object_mut()
                .expect("payload objek JSON")
                .insert("discount_code".to_string(), Value::String(code.to_string()));
        }
        let body = self
            .call("/sheerid/verify", Method::POST, Some(&payload))
            .await?;
        let Some(job) = body.get("job") else {
            return Err(anyhow::anyhow!(
                "LahaStore: respons /sheerid/verify tanpa job"
            ));
        };
        Ok(SheeridVerificationResult {
            job_id: json_str(job, "job_id").unwrap_or_default(),
            status: json_str(job, "status").unwrap_or_else(|| "pending".to_string()),
            raw: job.clone(),
        })
    }
}

fn map_status(status: Option<&Value>) -> SupplierOrderStatus {
    match status.and_then(|v| v.as_str()) {
        Some("completed") | Some("success") => SupplierOrderStatus::Success,
        Some("failed") => SupplierOrderStatus::Failed,
        Some("cancelled") => SupplierOrderStatus::Cancelled,
        Some("pending") | Some("processing") | Some("running") | Some("submitting") => {
            SupplierOrderStatus::Processing
        }
        _ => SupplierOrderStatus::Unknown,
    }
}

fn map_order(order: &Value) -> SupplierOrderResult {
    let accounts: Vec<String> = order
        .get("accounts")
        .and_then(|a| a.as_array())
        .map(|arr| {
            arr.iter()
                .map(|v| match v {
                    Value::String(s) => s.clone(),
                    other => other.to_string(),
                })
                .collect()
        })
        .unwrap_or_default();
    SupplierOrderResult {
        supplier_order_id: json_str(order, "orderCode")
            .or_else(|| json_str(order, "id"))
            .unwrap_or_default(),
        status: map_status(order.get("status")),
        delivered_items: accounts,
        result_link: None,
        error_code: None,
        error_message: None,
        raw: order.clone(),
    }
}

impl SupplierAdapter for LahaStoreAdapter {
    fn id(&self) -> SupplierId {
        SupplierId::LahaStore
    }

    fn is_configured(&self) -> bool {
        !self.api_key.is_empty()
    }

    async fn get_balance(&self) -> anyhow::Result<SupplierBalance> {
        let body = self.call("/balance", Method::GET, None).await?;
        let wallet = body.get("wallet").cloned().unwrap_or(Value::Null);
        let vnd = json_f64(&wallet, "vnd").unwrap_or(0.0);
        let mut raw = body.clone();
        if let Some(obj) = raw.as_object_mut() {
            // TS: raw = { vnd, ...body } — body menang kalau sudah punya vnd.
            obj.entry("vnd").or_insert(Value::from(vnd));
        }
        Ok(SupplierBalance {
            // LahaStore tidak memberi saldo IDR langsung;
            // VND dikonversi di pricing (vnd_to_idr di atas).
            idr: 0.0,
            usd: json_f64(&wallet, "usd").unwrap_or(0.0),
            credit: json_f64(&wallet, "credit").unwrap_or(0.0),
            raw,
        })
    }

    async fn list_products(&self) -> anyhow::Result<Vec<SupplierProduct>> {
        let body = self
            .call("/products?page=1&limit=50", Method::GET, None)
            .await?;
        let products = body
            .get("products")
            .and_then(|p| p.as_array())
            .cloned()
            .unwrap_or_default();
        Ok(products
            .iter()
            .map(|p| SupplierProduct {
                code: json_str(p, "id")
                    .or_else(|| json_str(p, "code"))
                    .unwrap_or_default(),
                name: json_str(p, "name")
                    .or_else(|| json_str(p, "code"))
                    .unwrap_or_default(),
                // Harga di LahaStore umumnya VND — konversi di pricing.
                wholesale_price_idr: None,
                wholesale_price_usd: json_f64(p, "priceUsd"),
                stock: json_i64(p, "stock"),
                min_qty: None,
                max_qty: None,
            })
            .collect())
    }

    async fn get_stock<'a>(
        &'a self,
        supplier_product_code: &'a str,
    ) -> anyhow::Result<Option<i64>> {
        // LahaStore tidak punya endpoint /stock — stok dibaca dari /products.
        let products = self.list_products().await?;
        Ok(products
            .iter()
            .find(|p| p.code == supplier_product_code)
            .and_then(|p| p.stock))
    }

    /// Order produk shop biasa (bukan Gemini Xiaomi / SheerID — lihat di atas).
    async fn create_order(&self, input: CreateOrderInput) -> anyhow::Result<SupplierOrderResult> {
        let payload = json!({
            "productId": input.supplier_product_code,
            "qty": input.quantity,
            "currency": "vnd",
            "shopOrderId": input.idempotency_key,
        });
        let body = self.call("/orders", Method::POST, Some(&payload)).await?;
        let Some(order) = body.get("order") else {
            return Err(anyhow::anyhow!("LahaStore: respons /orders tanpa order"));
        };
        Ok(map_order(order))
    }

    async fn get_order<'a>(
        &'a self,
        supplier_order_id: &'a str,
    ) -> anyhow::Result<SupplierOrderResult> {
        let body = self
            .call(
                &format!("/orders/{}", super::sondev::percent_encode(supplier_order_id)),
                Method::GET,
                None,
            )
            .await?;
        let Some(order) = body.get("order") else {
            return Err(anyhow::anyhow!("LahaStore: order tidak ditemukan"));
        };
        Ok(map_order(order))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn map_status_works() {
        assert_eq!(map_status(Some(&json!("completed"))), SupplierOrderStatus::Success);
        assert_eq!(map_status(Some(&json!("running"))), SupplierOrderStatus::Processing);
        assert_eq!(map_status(Some(&json!("submitting"))), SupplierOrderStatus::Processing);
        assert_eq!(map_status(Some(&json!("failed"))), SupplierOrderStatus::Failed);
        assert_eq!(map_status(Some(&json!("weird"))), SupplierOrderStatus::Unknown);
        assert_eq!(map_status(None), SupplierOrderStatus::Unknown);
    }

    #[test]
    fn map_order_uses_order_code_first() {
        let order = json!({
            "orderCode": "LH-1",
            "id": "999",
            "status": "completed",
            "accounts": ["a@b.c|pass"]
        });
        let r = map_order(&order);
        assert_eq!(r.supplier_order_id, "LH-1");
        assert_eq!(r.status, SupplierOrderStatus::Success);
        assert_eq!(r.delivered_items, vec!["a@b.c|pass"]);
    }

    #[test]
    fn currency_conversions_match_pricing_ts() {
        let a = LahaStoreAdapter::new("k".into(), LAHASTORE_DEFAULT_BASE_URL.into(), 16500)
            .expect("adapter");
        assert_eq!(a.usd_to_idr(1.0), 16500);
        assert_eq!(a.usd_to_idr(0.54), 8910);
        // VND 26.500 = USD 1 = IDR 16.500 (kurs saat dicek 2026-11-17).
        assert_eq!(a.vnd_to_idr(26500.0), 16500);
    }
}
