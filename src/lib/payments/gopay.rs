//! Client GoBiz / GoPay — port dari `src/lib/payments/gopay.ts`.
//!
//! Auth OTP + profil merchant + feed transaksi. Ditulis dari pola library Go
//! paygateme (ATM: amati, tiru, modifikasi) — BUKAN salinan kode: endpoint +
//! bentuk request/response diringkas dari gopay/*.go, implementasi HTTP-nya
//! TypeScript sendiri (di sini: Rust sendiri).
//!
//! ⚠️ STATUS REFERENSI: provider GoPay di paygateme dinyatakan
//! "in development — untested against the live GoBiz API". Semua endpoint
//! di bawah ini adalah HIPOTESIS yang belum terverifikasi live. Karena itu
//! modul ini dibangun defensif:
//!   - setiap request punya timeout (20 detik),
//!   - error dibedakan jelas (auth / captcha / api / http),
//!   - 401 -> refresh sekali -> retry sekali, lalu menyerah dengan
//!     AuthRequired (bukan retry tanpa henti),
//!   - sesi kedaluwarsa terdeteksi SEBELUM request (refresh proaktif 5 menit),
//!   - token TIDAK PERNAH ditulis ke log / dikembalikan ke client.
//!     Kalau GoBiz menolak endpoint ini di live-check, perbaiki mapping-nya di
//!     sini (satu tempat), bukan di pemanggil.
//!
//! Sesi disimpan di tabel settings (key gopay_session) — satu baris upsert,
//! jadi tulisnya atomik. OTP context (otp_token) disimpan di settings juga
//! (key gopay_otp), OTP-nya sendiri TIDAK PERNAH disimpan.
//!
//! JALUR DORMANT: OTP GoBiz diblokir server Gojek (GoPay-1000) dan Midtrans
//! adalah jalur bayar aktif. Modul ini mem-port perilaku apa adanya —
//! JANGAN "memperbaiki" OTP-nya di sini.

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

use base64::Engine;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::config::Config;
use crate::db::{get_setting, set_setting, Db};

use super::qris::FeedTransaction;
use super::qris_settings::get_gopay_static_qris;

/// Base URL GoID (login/token) — dari komentar konstanta paygateme.
pub const GOID_BASE_URL: &str = "https://api.gojekapi.com";
/// Base URL GoBiz (merchant/feed) — dari komentar konstanta paygateme.
pub const GOBIZ_BASE_URL: &str = "https://api.gobiz.co.id";
/// Timeout tiap request HTTP (paygateme: 20 detik).
pub const GOPAY_REQUEST_TIMEOUT_MS: u64 = 20_000;
/// Refresh proaktif 5 menit sebelum access token kedaluwarsa.
pub const GOPAY_REFRESH_BEFORE_EXPIRY_MS: i64 = 5 * 60_000;
/// OTP context berlaku 10 menit.
pub const GOPAY_OTP_TTL_MS: i64 = 10 * 60_000;
/// Jendela feed default: 1 jam ke belakang (cukup untuk QR 5 menit +
/// transaksi yang terindeks telat; matcher tetap memvalidasi waktunya).
pub const GOPAY_FEED_LOOKBACK_MS: i64 = 60 * 60_000;
/// Ukuran halaman feed maksimum (server menolak >100 dengan 422).
pub const GOPAY_FEED_PAGE_SIZE: i64 = 100;
/// Batas halaman per tick (paygateme: MaxTransactionPagesPerTick = 10).
pub const GOPAY_FEED_MAX_PAGES: usize = 10;
/// Fallback kedaluwarsa access token kalau response + JWT tidak memberi tahu.
pub const GOPAY_TOKEN_FALLBACK_TTL_MS: i64 = 30 * 60_000;

const SESSION_KEY: &str = "gopay_session";
const OTP_KEY: &str = "gopay_otp";
const GOPAY_CLIENT_ID: &str = "go-biz-web-new";

// ---------------------------------------------------------------------------
// Error
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GopayErrorCode {
    /// Sesi habis / refresh ditolak -> login OTP ulang.
    AuthRequired,
    /// OTP salah / token response tidak berisi access token.
    AuthFailed,
    /// Server meminta captcha (belum ada jalur GoPay-nya).
    CaptchaRequired,
    /// GoBiz/GoID menjawab error bisnis.
    ApiError,
    /// HTTP gagal (jaringan / status tak terduga).
    HttpError,
    /// Input tidak valid (nomor HP / OTP kosong).
    ConfigInvalid,
}

impl GopayErrorCode {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::AuthRequired => "auth_required",
            Self::AuthFailed => "auth_failed",
            Self::CaptchaRequired => "captcha_required",
            Self::ApiError => "api_error",
            Self::HttpError => "http_error",
            Self::ConfigInvalid => "config_invalid",
        }
    }
}

/// Error GoBiz dengan kode yang bisa dicek pemanggil (tanpa bocor token).
#[derive(Debug, Clone)]
pub struct GopayError {
    pub code: GopayErrorCode,
    pub message: String,
    pub status: Option<u16>,
}

impl GopayError {
    pub fn new(code: GopayErrorCode, message: impl Into<String>) -> Self {
        Self { code, message: message.into(), status: None }
    }

    pub fn with_status(code: GopayErrorCode, message: impl Into<String>, status: u16) -> Self {
        Self { code, message: message.into(), status: Some(status) }
    }
}

impl std::fmt::Display for GopayError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "[{}] {}", self.code.as_str(), self.message)
    }
}

impl std::error::Error for GopayError {}

pub fn is_auth_required(err: &GopayError) -> bool {
    err.code == GopayErrorCode::AuthRequired
}

// ---------------------------------------------------------------------------
// Sesi
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GopaySession {
    pub access_token: String,
    pub refresh_token: String,
    #[serde(default = "default_token_type")]
    pub token_type: String,
    /// Epoch ms kapan access token kedaluwarsa.
    pub expires_at: i64,
    /// Nomor HP (ternormalisasi, tanpa 62/0 depan) yang dipakai login.
    #[serde(default)]
    pub phone: String,
    /// merchant_id GoBiz (diisi setelah profil pertama kali dibaca).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub merchant_id: Option<String>,
}

fn default_token_type() -> String {
    "Bearer".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct OtpContext {
    otp_token: String,
    phone: String,
    created_at: i64,
}

/// Normalisasi nomor HP Indonesia: hanya digit, buang awalan 62 lalu 0.
pub fn normalize_phone(raw: &str) -> String {
    let mut digits: String = raw.chars().filter(|c| c.is_ascii_digit()).collect();
    if let Some(rest) = digits.strip_prefix("62") {
        digits = rest.to_string();
    }
    if let Some(rest) = digits.strip_prefix('0') {
        digits = rest.to_string();
    }
    digits
}

fn valid_phone(digits: &str) -> bool {
    digits.len() >= 9
        && digits.len() <= 13
        && digits.starts_with('8')
        && digits.bytes().all(|b| b.is_ascii_digit())
}

/// Ambil klaim exp (detik) dari JWT access token, None kalau bukan JWT.
fn jwt_expiry_ms(access_token: &str) -> Option<i64> {
    let parts: Vec<&str> = access_token.split('.').collect();
    if parts.len() != 3 {
        return None;
    }
    let payload_bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(parts[1])
        .ok()?;
    let payload: serde_json::Value = serde_json::from_slice(&payload_bytes).ok()?;
    payload.get("exp")?.as_i64().map(|exp| exp * 1000)
}

fn default_now_ms() -> i64 {
    Utc::now().timestamp_millis()
}

fn static_headers() -> Vec<(String, String)> {
    [
        ("Accept", "application/json, text/plain, */*"),
        ("Authentication-Type", "go-id"),
        ("X-PhoneMake", "Web"),
        ("X-PhoneModel", "Node.js Client"),
        ("x-DeviceOS", "Web"),
        ("X-User-Locale", "id"),
        ("Gojek-Country-Code", "ID"),
        ("Gojek-Timezone", "Asia/Jakarta"),
        ("X-Platform", "Web"),
        ("X-User-Type", "merchant"),
        ("x-appId", "go-biz-web-dashboard"),
    ]
    .iter()
    .map(|(k, v)| (k.to_string(), v.to_string()))
    .collect()
}

// ---------------------------------------------------------------------------
// Transport HTTP (bisa di-inject — dipakai tes, cerminan fetchImpl TS)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct HttpResponse {
    pub status: u16,
    pub body: serde_json::Value,
}

pub trait GopayHttp: Send + Sync {
    fn request(
        &self,
        method: &str,
        url: &str,
        headers: Vec<(String, String)>,
        body: Option<String>,
    ) -> Pin<Box<dyn Future<Output = Result<HttpResponse, GopayError>> + Send + '_>>;
}

/// Transport nyata berbasis reqwest.
pub struct ReqwestGopayHttp {
    client: reqwest::Client,
}

impl ReqwestGopayHttp {
    pub fn new() -> Self {
        Self {
            client: reqwest::Client::builder()
                .timeout(Duration::from_millis(GOPAY_REQUEST_TIMEOUT_MS))
                .build()
                .unwrap_or_else(|_| reqwest::Client::new()),
        }
    }
}

impl Default for ReqwestGopayHttp {
    fn default() -> Self {
        Self::new()
    }
}

impl GopayHttp for ReqwestGopayHttp {
    fn request(
        &self,
        method: &str,
        url: &str,
        headers: Vec<(String, String)>,
        body: Option<String>,
    ) -> Pin<Box<dyn Future<Output = Result<HttpResponse, GopayError>> + Send + '_>> {
        let client = self.client.clone();
        let method = method.to_string();
        let url = url.to_string();
        Box::pin(async move {
            let req_method = reqwest::Method::from_bytes(method.as_bytes()).map_err(|_| {
                GopayError::new(GopayErrorCode::HttpError, format!("Method HTTP tidak valid: {method}"))
            })?;
            let mut req = client.request(req_method, &url);
            for (k, v) in headers {
                req = req.header(k, v);
            }
            if let Some(b) = body {
                req = req.body(b);
            }
            let res = req.send().await.map_err(|e| {
                GopayError::new(
                    GopayErrorCode::HttpError,
                    format!("GoBiz tidak bisa dihubungi: {e}"),
                )
            })?;
            let status = res.status().as_u16();
            let text = res.text().await.unwrap_or_default();
            let body_json: serde_json::Value =
                serde_json::from_str(&text).unwrap_or(serde_json::json!({}));
            Ok(HttpResponse { status, body: body_json })
        })
    }
}

// ---------------------------------------------------------------------------
// Client
// ---------------------------------------------------------------------------

#[derive(Default)]
pub struct GopayClientOptions {
    pub http: Option<Arc<dyn GopayHttp>>,
    pub now_ms: Option<fn() -> i64>,
    pub goid_base: Option<String>,
    pub gobiz_base: Option<String>,
    /// Nomor HP default (GOBIZ_PHONE dari Config).
    pub default_phone: Option<String>,
}

pub struct GopayClient {
    db: Db,
    http: Arc<dyn GopayHttp>,
    now_ms: fn() -> i64,
    goid_base: String,
    gobiz_base: String,
    default_phone: String,
}

impl GopayClient {
    /// Client default (reqwest + settings SQLite + nomor default dari Config).
    /// Dipakai poller + route admin.
    pub fn new(db: Db, cfg: &Config) -> Self {
        Self::with_options(
            db,
            GopayClientOptions {
                default_phone: Some(cfg.gobiz_phone.clone()),
                ..Default::default()
            },
        )
    }

    pub fn with_options(db: Db, opts: GopayClientOptions) -> Self {
        Self {
            db,
            http: opts.http.unwrap_or_else(|| Arc::new(ReqwestGopayHttp::new())),
            now_ms: opts.now_ms.unwrap_or(default_now_ms),
            goid_base: opts
                .goid_base
                .unwrap_or_else(|| GOID_BASE_URL.to_string())
                .trim_end_matches('/')
                .to_string(),
            gobiz_base: opts
                .gobiz_base
                .unwrap_or_else(|| GOBIZ_BASE_URL.to_string())
                .trim_end_matches('/')
                .to_string(),
            default_phone: opts.default_phone.unwrap_or_default(),
        }
    }

    fn now(&self) -> i64 {
        (self.now_ms)()
    }

    // ------------------------------------------------------------ sesi ---

    pub fn get_session(&self) -> Result<Option<GopaySession>, GopayError> {
        let raw = get_setting(&self.db, SESSION_KEY);
        let Some(raw) = raw else { return Ok(None) };
        let s: GopaySession = match serde_json::from_str(&raw) {
            Ok(s) => s,
            Err(_) => return Ok(None),
        };
        if s.access_token.is_empty() || s.refresh_token.is_empty() {
            return Ok(None);
        }
        Ok(Some(s))
    }

    pub fn save_session(&self, session: &GopaySession) -> Result<(), GopayError> {
        let raw = serde_json::to_string(session).map_err(|e| {
            GopayError::new(GopayErrorCode::HttpError, format!("Simpan sesi gagal: {e}"))
        })?;
        set_setting(&self.db, SESSION_KEY, &raw).map_err(|e| {
            GopayError::new(GopayErrorCode::HttpError, format!("Simpan sesi gagal: {e}"))
        })
    }

    pub fn clear_session(&self) -> Result<(), GopayError> {
        set_setting(&self.db, SESSION_KEY, "").map_err(|e| {
            GopayError::new(GopayErrorCode::HttpError, format!("Hapus sesi gagal: {e}"))
        })
    }

    /// Sesi ada DAN access token masih berlaku (di luar margin refresh).
    pub fn is_authenticated(&self, now_ms: Option<i64>) -> Result<bool, GopayError> {
        let Some(s) = self.get_session()? else {
            return Ok(false);
        };
        let now = now_ms.unwrap_or_else(|| self.now());
        Ok(s.expires_at > now + GOPAY_REFRESH_BEFORE_EXPIRY_MS)
    }

    /// Status sesi untuk panel admin (TANPA token).
    pub fn session_status(&self) -> Result<GopaySessionStatus, GopayError> {
        let Some(s) = self.get_session()? else {
            return Ok(GopaySessionStatus {
                authenticated: false,
                phone: None,
                expires_at: None,
                merchant_id: None,
            });
        };
        Ok(GopaySessionStatus {
            authenticated: self.is_authenticated(None)?,
            phone: if s.phone.is_empty() { None } else { Some(s.phone.clone()) },
            expires_at: Some(
                DateTime::from_timestamp_millis(s.expires_at)
                    .map(|dt| dt.to_rfc3339())
                    .unwrap_or_default(),
            ),
            merchant_id: s.merchant_id.clone(),
        })
    }

    // ------------------------------------------------------------ http ---

    async fn call(
        &self,
        base: GopayBase,
        path: &str,
        method: Option<&str>,
        body: Option<serde_json::Value>,
        auth: bool,
        _retried: bool,
    ) -> Result<serde_json::Value, GopayError> {
        // Loop 2 attempt (bukan rekursi — async fn rekursif butuh boxing):
        // attempt 0 = request asli; kalau 401 + auth, refresh sekali lalu
        // ulang sekali (attempt 1).
        let mut refreshed = false;
        loop {
            let res = self
                .call_once(base, path, method, body.clone(), auth)
                .await;
            match res {
                Err(GopayError {
                    code: GopayErrorCode::AuthRequired,
                    status: Some(401),
                    ..
                }) if auth && !refreshed => {
                    // Token kedaluwarsa di tengah jalan: refresh sekali, ulang sekali.
                    match self.refresh_session().await {
                        Ok(_) => {}
                        Err(refresh_err) => {
                            if refresh_err.code == GopayErrorCode::AuthRequired {
                                return Err(refresh_err);
                            }
                            return Err(GopayError::new(
                                GopayErrorCode::AuthRequired,
                                "Sesi GoBiz kedaluwarsa dan refresh gagal — login OTP ulang di panel admin.",
                            ));
                        }
                    }
                    refreshed = true;
                    continue;
                }
                other => return other,
            }
        }
    }

    /// Satu request HTTP mentah (tanpa logika refresh/retry).
    async fn call_once(
        &self,
        base: GopayBase,
        path: &str,
        method: Option<&str>,
        body: Option<serde_json::Value>,
        auth: bool,
    ) -> Result<serde_json::Value, GopayError> {
        let base_url = match base {
            GopayBase::Goid => &self.goid_base,
            GopayBase::Gobiz => &self.gobiz_base,
        };
        let url = format!("{base_url}{path}");
        let mut headers = static_headers();
        if body.is_some() {
            headers.push(("Content-Type".to_string(), "application/json".to_string()));
        }
        if auth {
            let Some(session) = self.get_session()? else {
                return Err(GopayError::new(
                    GopayErrorCode::AuthRequired,
                    "Sesi GoBiz belum ada — login OTP dulu di panel admin.",
                ));
            };
            headers.push((
                "Authorization".to_string(),
                format!("{} {}", session.token_type, session.access_token),
            ));
        } else {
            // Endpoint auth sengaja mengirim Bearer kosong (pola paygateme).
            headers.push(("Authorization".to_string(), "Bearer".to_string()));
        }
        let method = method.unwrap_or(if body.is_some() { "POST" } else { "GET" });
        let body_str = body.map(|b| serde_json::to_string(&b).unwrap_or_default());
        let res = self.http.request(method, &url, headers, body_str).await?;
        let status = res.status;
        let resp_body = res.body;
        if status == 401 {
            return Err(GopayError::with_status(
                GopayErrorCode::AuthRequired,
                "GoBiz menolak sesi ini (401) — login OTP ulang di panel admin.",
                401,
            ));
        }
        if status == 401 {
            return Err(GopayError::with_status(
                GopayErrorCode::AuthRequired,
                "GoBiz menolak sesi ini (401) — login OTP ulang di panel admin.",
                401,
            ));
        }
        if !(200..300).contains(&status) {
            return Err(GopayError::with_status(
                GopayErrorCode::HttpError,
                format!(
                    "GoBiz menjawab HTTP {status}: {}",
                    go_id_error_message(&resp_body).unwrap_or_else(|| "tanpa pesan".to_string())
                ),
                status,
            ));
        }
        // Sinyal captcha (defensif — paygateme tidak mendokumentasikan jalur
        // captcha GoPay; kalau server memintanya, jangan diam-diam gagal).
        if resp_body.to_string().to_lowercase().contains("captcha") {
            return Err(GopayError::new(
                GopayErrorCode::CaptchaRequired,
                "GoBiz meminta verifikasi captcha — selesaikan di aplikasi GoBiz, lalu login ulang di sini.",
            ));
        }
        Ok(resp_body)
    }

    // ------------------------------------------------------------ auth ---

    /// Minta OTP ke nomor HP merchant. Tanpa argumen, pakai nomor default
    /// dari env GOBIZ_PHONE (diisi Dim di .env.local). otp_token disimpan
    /// server-side (settings) — TIDAK dikembalikan ke pemanggil / client.
    pub async fn request_otp(
        &self,
        raw_phone: Option<&str>,
    ) -> Result<OtpRequest, GopayError> {
        let raw = raw_phone.unwrap_or(&self.default_phone);
        let phone = normalize_phone(raw);
        if !valid_phone(&phone) {
            return Err(GopayError::new(
                GopayErrorCode::ConfigInvalid,
                if raw_phone.is_some() {
                    "Nomor HP tidak valid — pakai format 08xx (contoh: 081234567890)."
                } else {
                    "Nomor HP GoBiz belum diisi — isi GOBIZ_PHONE di .env.local atau ketik nomornya di panel admin."
                },
            ));
        }
        let body = self
            .call(
                GopayBase::Goid,
                "/goid/login/request",
                None,
                Some(serde_json::json!({
                    "client_id": GOPAY_CLIENT_ID,
                    "phone_number": phone,
                    "country_code": "62",
                })),
                false,
                false,
            )
            .await?;
        let envelope = go_id_envelope(&body);
        if !envelope.0 {
            return Err(GopayError::new(
                GopayErrorCode::ApiError,
                format!("Gagal minta OTP: {}", envelope.1),
            ));
        }
        let data = body.get("data").cloned().unwrap_or(serde_json::json!({}));
        let otp_token = data
            .get("otp_token")
            .and_then(|v| v.as_str())
            .or_else(|| data.get("token").and_then(|v| v.as_str()))
            .unwrap_or("")
            .to_string();
        if otp_token.is_empty() {
            return Err(GopayError::new(
                GopayErrorCode::ApiError,
                "GoBiz tidak mengembalikan otp_token — coba lagi sebentar.",
            ));
        }
        let ctx = OtpContext { otp_token, phone: phone.clone(), created_at: self.now() };
        let raw_ctx = serde_json::to_string(&ctx).map_err(|e| {
            GopayError::new(GopayErrorCode::HttpError, format!("Simpan OTP gagal: {e}"))
        })?;
        set_setting(&self.db, OTP_KEY, &raw_ctx).map_err(|e| {
            GopayError::new(GopayErrorCode::HttpError, format!("Simpan OTP gagal: {e}"))
        })?;
        Ok(OtpRequest {
            phone,
            sent_at: DateTime::from_timestamp_millis(ctx.created_at)
                .map(|dt| dt.to_rfc3339())
                .unwrap_or_default(),
        })
    }

    /// Verifikasi OTP (diketik Dim di panel) -> simpan sesi.
    pub async fn verify_otp(&self, otp: &str) -> Result<GopaySession, GopayError> {
        let clean = otp.trim();
        if clean.len() < 4
            || clean.len() > 8
            || !clean.bytes().all(|b| b.is_ascii_digit())
        {
            return Err(GopayError::new(
                GopayErrorCode::ConfigInvalid,
                "Kode OTP harus 4–8 digit angka.",
            ));
        }
        let raw = get_setting(&self.db, OTP_KEY).unwrap_or_default();
        if raw.is_empty() {
            return Err(GopayError::new(
                GopayErrorCode::AuthRequired,
                "Belum ada permintaan OTP — tekan 'Minta OTP' dulu.",
            ));
        }
        let ctx: OtpContext = serde_json::from_str(&raw).map_err(|_| {
            GopayError::new(
                GopayErrorCode::AuthRequired,
                "Permintaan OTP rusak — tekan 'Minta OTP' lagi.",
            )
        })?;
        if ctx.otp_token.is_empty() || self.now() - ctx.created_at > GOPAY_OTP_TTL_MS {
            return Err(GopayError::new(
                GopayErrorCode::AuthRequired,
                "OTP sudah kedaluwarsa (10 menit) — minta OTP baru.",
            ));
        }
        let body = self
            .call(
                GopayBase::Goid,
                "/goid/token",
                None,
                Some(serde_json::json!({
                    "client_id": GOPAY_CLIENT_ID,
                    "grant_type": "otp",
                    "data": { "otp": clean, "otp_token": ctx.otp_token },
                })),
                false,
                false,
            )
            .await?;
        let session = session_from_token_response(&body, &ctx.phone, self.now())?;
        self.save_session(&session)?;
        set_setting(&self.db, OTP_KEY, "").map_err(|e| {
            GopayError::new(GopayErrorCode::HttpError, format!("Hapus OTP gagal: {e}"))
        })?;
        Ok(session)
    }

    /// Tukar refresh token dengan access token baru. Refresh ditolak
    /// (400/401) = sesi mati -> AuthRequired (login OTP ulang).
    pub async fn refresh_session(&self) -> Result<GopaySession, GopayError> {
        let current = self.get_session()?.ok_or_else(|| {
            GopayError::new(
                GopayErrorCode::AuthRequired,
                "Tidak ada sesi GoBiz untuk di-refresh — login OTP dulu.",
            )
        })?;
        let body = match self
            // Pakai call_once (bukan call): endpoint auth tidak butuh
            // logika refresh/retry — dan ini memutus siklus async
            // call -> refresh_session -> call.
            .call_once(
                GopayBase::Goid,
                "/goid/token",
                None,
                Some(serde_json::json!({
                    "client_id": GOPAY_CLIENT_ID,
                    "grant_type": "refresh_token",
                    "data": { "refresh_token": current.refresh_token },
                })),
                false,
            )
            .await
        {
            Ok(b) => b,
            Err(e) => {
                if matches!(e.status, Some(400) | Some(401)) {
                    let _ = self.clear_session();
                    return Err(GopayError::new(
                        GopayErrorCode::AuthRequired,
                        "Refresh token ditolak GoBiz — login OTP ulang di panel admin.",
                    ));
                }
                return Err(e);
            }
        };
        let mut next = session_from_token_response(&body, &current.phone, self.now())?;
        // Response refresh kadang tidak menyertakan refresh_token baru —
        // pertahankan yang lama (pola paygateme).
        if next.refresh_token.is_empty() {
            next.refresh_token = current.refresh_token.clone();
        }
        next.merchant_id = current.merchant_id.clone();
        self.save_session(&next)?;
        Ok(next)
    }

    /// Pastikan sesi segar: refresh proaktif kalau masuk margin 5 menit.
    pub async fn ensure_fresh_session(&self) -> Result<GopaySession, GopayError> {
        let s = self.get_session()?.ok_or_else(|| {
            GopayError::new(
                GopayErrorCode::AuthRequired,
                "Sesi GoBiz belum ada — login OTP dulu di panel admin.",
            )
        })?;
        if s.expires_at <= self.now() + GOPAY_REFRESH_BEFORE_EXPIRY_MS {
            return self.refresh_session().await;
        }
        Ok(s)
    }

    // ---------------------------------------------------------- merchant ---

    /// Profil merchant + cocokkan QRIS statis outlet dengan yang tersimpan
    /// di settings (peringatan kalau beda — QR yang dibuat bot harus sama
    /// dengan outlet yang dicek feed-nya).
    pub async fn get_merchant_profile(&self) -> Result<MerchantProfileResult, GopayError> {
        self.ensure_fresh_session().await?;
        let me = self
            .call(GopayBase::Gobiz, "/v1/users/me", Some("GET"), None, true, false)
            .await?;
        let user = me.get("user").cloned().unwrap_or(serde_json::json!({}));
        let merchant_id = user
            .get("merchant_id")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        if merchant_id.is_empty() {
            return Err(GopayError::new(
                GopayErrorCode::ApiError,
                "GoBiz tidak mengembalikan merchant_id untuk akun ini.",
            ));
        }
        let raw = self
            .call(
                GopayBase::Gobiz,
                &format!("/v1/merchants/{}", encode_segment(&merchant_id)),
                Some("GET"),
                None,
                true,
                false,
            )
            .await?;
        let pops = raw.get("pops").and_then(|v| v.as_array()).cloned().unwrap_or_default();
        let profile = GopayMerchantProfile {
            id: raw
                .get("id")
                .and_then(|v| v.as_str())
                .map(str::to_string)
                .unwrap_or_else(|| merchant_id.clone()),
            merchant_name: raw
                .get("merchant_name")
                .and_then(|v| v.as_str())
                .map(str::to_string),
            outlet_name: raw
                .get("outlet_name")
                .and_then(|v| v.as_str())
                .map(str::to_string),
            phone: raw.get("phone").and_then(|v| v.as_str()).map(str::to_string),
            email: raw.get("email").and_then(|v| v.as_str()).map(str::to_string),
            outlets: pops
                .iter()
                .map(|p| {
                    let gopay = p.get("gopay").cloned().unwrap_or(serde_json::json!({}));
                    GopayOutlet {
                        pop_id: p
                            .get("pop_id")
                            .map(json_string)
                            .unwrap_or_default(),
                        name: p
                            .get("name")
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .to_string(),
                        status: p
                            .get("status")
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .to_string(),
                        receiver_id: gopay
                            .get("gopay_receiver_id")
                            .and_then(|v| v.as_str())
                            .map(str::to_string),
                        qr_string: gopay
                            .get("aspi_qr_string")
                            .and_then(|v| v.as_str())
                            .map(str::to_string),
                    }
                })
                .collect(),
        };
        // Simpan merchant_id ke sesi supaya feed tidak perlu memanggil /me lagi.
        if let Some(mut session) = self.get_session()? {
            session.merchant_id = Some(profile.id.clone());
            self.save_session(&session)?;
        }
        let stored = get_gopay_static_qris(&self.db).map_err(|e| {
            GopayError::new(GopayErrorCode::HttpError, format!("Baca QRIS statis gagal: {e}"))
        })?;
        let mut warnings: Vec<String> = Vec::new();
        let mut matches = false;
        match stored {
            None => warnings.push("QRIS statis belum dipasang di panel admin.".to_string()),
            Some(stored) => {
                matches = profile
                    .outlets
                    .iter()
                    .any(|o| o.qr_string.as_deref() == Some(stored.as_str()));
                if !matches {
                    warnings.push(
                        "QRIS statis di panel berbeda dari outlet GoBiz — perbarui string QRIS dari aplikasi GoBiz supaya QR yang dibuat bot cocok dengan feed."
                            .to_string(),
                    );
                }
            }
        }
        if profile.outlets.is_empty() {
            warnings.push("GoBiz tidak mengembalikan outlet untuk merchant ini.".to_string());
        }
        Ok(MerchantProfileResult { profile, static_qris_matches: matches, warnings })
    }

    // ------------------------------------------------------------- feed ---

    /// Transaksi terbaru (jendela lookback, status settlement/capture,
    /// payment qris/gopay). Dinormalisasi ke FeedTransaction (qris.rs):
    /// nominal = gross/100 (satuan minor -> rupiah, harus bulat),
    /// waktu = transaction_time. Transaksi yang nominalnya tidak bulat /
    /// tidak ada id-nya dilewati (tidak akan match).
    pub async fn list_feed_transactions(
        &self,
        opts: FeedListOptions,
    ) -> Result<FeedListResult, GopayError> {
        let session = self.ensure_fresh_session().await?;
        let mut merchant_id = session.merchant_id.clone();
        if merchant_id.is_none() {
            let result = self.get_merchant_profile().await?;
            merchant_id = Some(result.profile.id);
        }
        let merchant_id = merchant_id.unwrap_or_default();
        let lookback = opts.lookback_ms.unwrap_or(GOPAY_FEED_LOOKBACK_MS);
        let end = DateTime::from_timestamp_millis(self.now())
            .map(|dt| dt.to_rfc3339())
            .unwrap_or_default();
        let start = DateTime::from_timestamp_millis(self.now() - lookback)
            .map(|dt| dt.to_rfc3339())
            .unwrap_or_default();
        let statuses = opts.statuses.unwrap_or_else(|| {
            vec!["settlement".to_string(), "capture".to_string()]
        });
        let payment_types = opts
            .payment_types
            .unwrap_or_else(|| vec!["qris".to_string(), "gopay".to_string()]);
        let mut transactions: Vec<FeedTransaction> = Vec::new();
        let mut truncated = false;
        let mut from: i64 = 0;
        let mut total: i64 = i64::MAX;
        let mut pages = 0usize;
        while from < total && pages < GOPAY_FEED_MAX_PAGES {
            let query = [
                ("from", from.to_string()),
                ("size", GOPAY_FEED_PAGE_SIZE.to_string()),
                ("start_time", start.clone()),
                ("end_time", end.clone()),
                ("merchant_ids", merchant_id.clone()),
                ("statuses", statuses.join(",")),
                ("payment_types", payment_types.join(",")),
            ]
            .iter()
            .map(|(k, v)| format!("{}={}", encode_segment(k), encode_segment(v)))
            .collect::<Vec<_>>()
            .join("&");
            let body = self
                .call(
                    GopayBase::Gobiz,
                    &format!("/merchant-analytics/v2/merchants/transactions?{query}"),
                    Some("GET"),
                    None,
                    true,
                    false,
                )
                .await?;
            let list = body
                .get("transactions")
                .and_then(|v| v.as_array())
                .cloned()
                .unwrap_or_default();
            total = body
                .get("total")
                .and_then(|v| v.as_i64())
                .unwrap_or(from + list.len() as i64);
            for raw_tx in &list {
                let id = raw_tx
                    .get("id")
                    .map(json_string)
                    .unwrap_or_default();
                if id.is_empty() {
                    continue;
                }
                let raw_amount = raw_tx
                    .get("real_gross_amount")
                    .and_then(|v| v.as_i64())
                    .or_else(|| raw_tx.get("gross_amount").and_then(|v| v.as_i64()));
                let Some(raw_amount) = raw_amount else {
                    continue;
                };
                // gross dalam satuan minor (sen) -> rupiah; harus bulat.
                if raw_amount % 100 != 0 {
                    continue;
                }
                let amount_idr = raw_amount / 100;
                if amount_idr <= 0 {
                    continue;
                }
                let time = raw_tx
                    .get("transaction_time")
                    .and_then(|v| v.as_str())
                    .and_then(|s| DateTime::parse_from_rfc3339(s).ok())
                    .map(|dt| dt.with_timezone(&Utc));
                transactions.push(FeedTransaction {
                    id,
                    amount_idr,
                    time,
                    status: raw_tx
                        .get("transaction_status")
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_lowercase()),
                });
            }
            from += list.len() as i64;
            pages += 1;
            if list.is_empty() {
                break;
            }
        }
        if from < total {
            truncated = true;
        }
        Ok(FeedListResult { transactions, truncated })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum GopayBase {
    Goid,
    Gobiz,
}

#[derive(Debug, Clone)]
pub struct GopaySessionStatus {
    pub authenticated: bool,
    pub phone: Option<String>,
    pub expires_at: Option<String>,
    pub merchant_id: Option<String>,
}

#[derive(Debug, Clone)]
pub struct OtpRequest {
    pub phone: String,
    pub sent_at: String,
}

#[derive(Debug, Clone)]
pub struct GopayOutlet {
    pub pop_id: String,
    pub name: String,
    pub status: String,
    pub receiver_id: Option<String>,
    /// String QRIS statis outlet (aspi_qr_string).
    pub qr_string: Option<String>,
}

#[derive(Debug, Clone)]
pub struct GopayMerchantProfile {
    pub id: String,
    pub merchant_name: Option<String>,
    pub outlet_name: Option<String>,
    pub phone: Option<String>,
    pub email: Option<String>,
    pub outlets: Vec<GopayOutlet>,
}

#[derive(Debug, Clone)]
pub struct MerchantProfileResult {
    pub profile: GopayMerchantProfile,
    pub static_qris_matches: bool,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Default)]
pub struct FeedListOptions {
    pub lookback_ms: Option<i64>,
    pub statuses: Option<Vec<String>>,
    pub payment_types: Option<Vec<String>>,
}

#[derive(Debug, Clone)]
pub struct FeedListResult {
    pub transactions: Vec<FeedTransaction>,
    pub truncated: bool,
}

// ---------------------------------------------------------------------------
// Helper response
// ---------------------------------------------------------------------------

fn json_string(v: &serde_json::Value) -> String {
    match v {
        serde_json::Value::String(s) => s.clone(),
        serde_json::Value::Number(n) => n.to_string(),
        _ => String::new(),
    }
}

fn encode_segment(s: &str) -> String {
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

/// Pesan error dari envelope GoID {success, errors[0]} (atau None).
fn go_id_error_message(body: &serde_json::Value) -> Option<String> {
    if let Some(errors) = body.get("errors").and_then(|v| v.as_array()) {
        if let Some(e) = errors.first() {
            if let Some(msg) = e.get("message").and_then(|v| v.as_str()) {
                return Some(msg.to_string());
            }
            if let Some(msg) = e.get("message_title").and_then(|v| v.as_str()) {
                return Some(msg.to_string());
            }
            if let Some(code) = e.get("code").and_then(|v| v.as_str()) {
                return Some(code.to_string());
            }
        }
    }
    body.get("message").and_then(|v| v.as_str()).map(str::to_string)
}

/// (success, message). success=false kalau body.success === false.
fn go_id_envelope(body: &serde_json::Value) -> (bool, String) {
    if body.get("success") == Some(&serde_json::Value::Bool(false)) {
        return (
            false,
            go_id_error_message(body).unwrap_or_else(|| "GoID menolak permintaan".to_string()),
        );
    }
    (true, String::new())
}

/// Bangun sesi dari response POST /goid/token (OTP maupun refresh).
fn session_from_token_response(
    body: &serde_json::Value,
    phone: &str,
    now_ms: i64,
) -> Result<GopaySession, GopayError> {
    let data = body.get("data").unwrap_or(body);
    let access_token = data
        .get("access_token")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    if access_token.is_empty() {
        return Err(GopayError::new(
            GopayErrorCode::AuthFailed,
            "GoID tidak mengembalikan access_token — OTP mungkin salah, coba lagi.",
        ));
    }
    let refresh_token = data
        .get("refresh_token")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let expires_in = data.get("expires_in").and_then(|v| v.as_i64());
    let expires_at = match expires_in {
        Some(sec) if sec > 0 => now_ms + sec * 1000,
        _ => jwt_expiry_ms(&access_token).unwrap_or(now_ms + GOPAY_TOKEN_FALLBACK_TTL_MS),
    };
    Ok(GopaySession {
        access_token,
        refresh_token,
        token_type: data
            .get("token_type")
            .and_then(|v| v.as_str())
            .unwrap_or("Bearer")
            .to_string(),
        expires_at,
        phone: phone.to_string(),
        merchant_id: None,
    })
}

// ---------------------------------------------------------------------------
// Tes (cerminan gopay.test.ts — semua respons di-mock, tanpa jaringan)
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lib::payments::qris_settings::set_gopay_static_qris;
    use rusqlite::Connection;
    use std::sync::Mutex as StdMutex;
    use std::sync::{Arc, Mutex};

    fn test_db() -> Db {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE settings (
              key TEXT PRIMARY KEY,
              value TEXT NOT NULL,
              updated_at TEXT NOT NULL
            );",
        )
        .unwrap();
        Arc::new(Mutex::new(conn))
    }

    /// Mock transport: handler memutuskan respons dari (method, url, body).
    /// Handler mock transport: (method, url, body) -> (status, json).
    type MockHandler = Arc<
        dyn Fn(&str, &str, Option<&str>) -> Option<(u16, serde_json::Value)> + Send + Sync,
    >;

    struct MockHttp {
        calls: StdMutex<Vec<(String, String, Option<String>)>>,
        handler: MockHandler,
    }

    impl MockHttp {
        fn new(
            handler: impl Fn(&str, &str, Option<&str>) -> Option<(u16, serde_json::Value)>
                + Send
                + Sync
                + 'static,
        ) -> Self {
            Self { calls: StdMutex::new(Vec::new()), handler: Arc::new(handler) }
        }
    }

    impl GopayHttp for MockHttp {
        fn request(
            &self,
            method: &str,
            url: &str,
            _headers: Vec<(String, String)>,
            body: Option<String>,
        ) -> Pin<Box<dyn Future<Output = Result<HttpResponse, GopayError>> + Send + '_>> {
            self.calls
                .lock()
                .unwrap()
                .push((method.to_string(), url.to_string(), body.clone()));
            let res = (self.handler)(method, url, body.as_deref());
            let method = method.to_string();
            let url = url.to_string();
            Box::pin(async move {
                match res {
                    Some((status, body)) => Ok(HttpResponse { status, body }),
                    None => Err(GopayError::new(
                        GopayErrorCode::HttpError,
                        format!("MOCK: tidak ada handler untuk {method} {url}"),
                    )),
                }
            })
        }
    }

    fn client_with(
        db: &Db,
        handler: impl Fn(&str, &str, Option<&str>) -> Option<(u16, serde_json::Value)>
            + Send
            + Sync
            + 'static,
    ) -> GopayClient {
        GopayClient::with_options(
            db.clone(),
            GopayClientOptions {
                http: Some(Arc::new(MockHttp::new(handler))),
                now_ms: Some(|| 1_790_000_000_000),
                ..Default::default()
            },
        )
    }

    fn token_ok() -> serde_json::Value {
        serde_json::json!({
            "access_token": "eyJhbGciOiJIUzI1NiJ9.eyJleHAiOjE3OTAwMDM2MDB9.sig",
            "refresh_token": "REFRESH-1",
            "token_type": "Bearer",
            "expires_in": 3600,
        })
    }

    #[test]
    fn phone_normalization() {
        assert_eq!(normalize_phone("0812-3456-7890"), "81234567890");
        assert_eq!(normalize_phone("+62 812 3456 7890"), "81234567890");
        assert_eq!(normalize_phone("6281234567890"), "81234567890");
        assert!(!valid_phone("123"));
        assert!(valid_phone("81234567890"));
    }

    #[test]
    fn jwt_expiry() {
        // Header.payload.signature, payload {"exp":1790003600}.
        let payload = base64::engine::general_purpose::URL_SAFE_NO_PAD
            .encode(r#"{"exp":1790003600}"#);
        let token = format!("eyJhbGciOiJIUzI1NiJ9.{payload}.sig");
        assert_eq!(jwt_expiry_ms(&token), Some(1_790_003_600_000));
        assert_eq!(jwt_expiry_ms("bukan-jwt"), None);
        assert_eq!(jwt_expiry_ms("a.b"), None);
    }

    #[tokio::test]
    async fn request_otp_flow() {
        let db = test_db();
        let client = client_with(&db, |_, url, _| {
            if url.contains("/goid/login/request") {
                Some((200, serde_json::json!({ "success": true, "data": { "otp_token": "OTP-CTX-1" } })))
            } else {
                None
            }
        });
        let req = client.request_otp(Some("081234567890")).await.unwrap();
        assert_eq!(req.phone, "81234567890");
        let stored: OtpContext =
            serde_json::from_str(&get_setting(&db, OTP_KEY).unwrap()).unwrap();
        assert_eq!(stored.otp_token, "OTP-CTX-1");
        assert_eq!(stored.phone, "81234567890");
        // Nomor tidak valid -> config_invalid.
        let err = client.request_otp(Some("123")).await.unwrap_err();
        assert_eq!(err.code, GopayErrorCode::ConfigInvalid);
        // Tanpa argumen + tanpa default -> config_invalid.
        let err = client.request_otp(None).await.unwrap_err();
        assert_eq!(err.code, GopayErrorCode::ConfigInvalid);
    }

    #[tokio::test]
    async fn request_otp_envelope_fail() {
        let db = test_db();
        let client = client_with(&db, |_, url, _| {
            if url.contains("/goid/login/request") {
                Some((
                    200,
                    serde_json::json!({
                        "success": false,
                        "errors": [{ "code": "E1", "message": "Nomor belum terdaftar" }],
                    }),
                ))
            } else {
                None
            }
        });
        let err = client.request_otp(Some("081234567890")).await.unwrap_err();
        assert_eq!(err.code, GopayErrorCode::ApiError);
        assert!(err.message.contains("Nomor belum terdaftar"), "{}", err.message);
    }

    #[tokio::test]
    async fn verify_otp_flow() {
        let db = test_db();
        let client = client_with(&db, |_, url, _| {
            if url.contains("/goid/token") {
                Some((200, token_ok()))
            } else {
                None
            }
        });
        // Tanpa request OTP dulu -> auth_required.
        let err = client.verify_otp("123456").await.unwrap_err();
        assert_eq!(err.code, GopayErrorCode::AuthRequired);
        // Simpan konteks OTP lalu verifikasi.
        set_setting(
            &db,
            OTP_KEY,
            &serde_json::to_string(&OtpContext {
                otp_token: "OTP-CTX-1".into(),
                phone: "81234567890".into(),
                created_at: 1_790_000_000_000,
            })
            .unwrap(),
        )
        .unwrap();
        let session = client.verify_otp("123456").await.unwrap();
        assert!(!session.access_token.is_empty());
        assert_eq!(session.expires_at, 1_790_000_000_000 + 3600 * 1000);
        assert_eq!(session.phone, "81234567890");
        // Sesi tersimpan; OTP context dibersihkan.
        assert!(client.get_session().unwrap().is_some());
        assert_eq!(get_setting(&db, OTP_KEY).as_deref(), Some(""));
        assert!(client.is_authenticated(None).unwrap());
        // OTP format salah -> config_invalid.
        let err = client.verify_otp("12").await.unwrap_err();
        assert_eq!(err.code, GopayErrorCode::ConfigInvalid);
    }

    #[tokio::test]
    async fn refresh_rejected_clears_session() {
        let db = test_db();
        let client = client_with(&db, |_, url, _| {
            if url.contains("/goid/token") {
                Some((401, serde_json::json!({ "message": "invalid_grant" })))
            } else {
                None
            }
        });
        client
            .save_session(&GopaySession {
                access_token: "OLD-ACCESS".into(),
                refresh_token: "OLD-REFRESH".into(),
                token_type: "Bearer".into(),
                expires_at: 1_790_000_000_000 + 1000,
                phone: "81234567890".into(),
                merchant_id: None,
            })
            .unwrap();
        let err = client.refresh_session().await.unwrap_err();
        assert_eq!(err.code, GopayErrorCode::AuthRequired);
        assert!(client.get_session().unwrap().is_none(), "sesi dibersihkan");
    }

    #[tokio::test]
    async fn feed_pagination_and_normalization() {
        let db = test_db();
        let client = client_with(&db, |_, url, _| {
            if url.contains("/merchant-analytics/v2/merchants/transactions") {
                if url.contains("from=0") {
                    Some((
                        200,
                        serde_json::json!({
                            "total": 3,
                            "transactions": [
                                { "id": "tx1", "real_gross_amount": 150100, "transaction_time": "2026-11-21T09:00:00Z", "transaction_status": "settlement" },
                                { "id": "tx2", "real_gross_amount": 150150, "transaction_time": "2026-11-21T09:01:00Z", "transaction_status": "settlement" },
                            ],
                        }),
                    ))
                } else {
                    Some((
                        200,
                        serde_json::json!({
                            "total": 3,
                            "transactions": [
                                { "id": "tx3", "real_gross_amount": 200000, "transaction_time": "2026-11-21T09:02:00Z", "transaction_status": "capture" },
                            ],
                        }),
                    ))
                }
            } else {
                None
            }
        });
        // Sesi segar + merchant_id sudah ada -> tidak perlu /me.
        client
            .save_session(&GopaySession {
                access_token: "ACCESS".into(),
                refresh_token: "REFRESH".into(),
                token_type: "Bearer".into(),
                expires_at: 1_790_000_000_000 + 3600 * 1000,
                phone: "81234567890".into(),
                merchant_id: Some("M-1".into()),
            })
            .unwrap();
        let feed = client
            .list_feed_transactions(FeedListOptions::default())
            .await
            .unwrap();
        // tx2 nominal 1501.5 (150150 sen) -> tidak bulat -> dilewati.
        assert_eq!(feed.transactions.len(), 2);
        assert_eq!(feed.transactions[0].id, "tx1");
        assert_eq!(feed.transactions[0].amount_idr, 1501);
        assert_eq!(feed.transactions[1].id, "tx3");
        assert_eq!(feed.transactions[1].amount_idr, 2000);
        assert!(!feed.truncated);
    }

    #[tokio::test]
    async fn merchant_profile_warnings() {
        let db = test_db();
        set_gopay_static_qris(&db, "QRIS-STATIS-BEDA").unwrap();
        let client = client_with(&db, |_, url, _| {
            if url.contains("/v1/users/me") {
                Some((200, serde_json::json!({ "user": { "merchant_id": "M-9" } })))
            } else if url.contains("/v1/merchants/M-9") {
                Some((
                    200,
                    serde_json::json!({
                        "id": "M-9",
                        "merchant_name": "TOKO TEST",
                        "pops": [
                            { "pop_id": "P1", "name": "Outlet 1", "status": "active",
                              "gopay": { "gopay_receiver_id": "R1", "aspi_qr_string": "QRIS-STATIS-ASLI" } },
                        ],
                    }),
                ))
            } else {
                None
            }
        });
        client
            .save_session(&GopaySession {
                access_token: "ACCESS".into(),
                refresh_token: "REFRESH".into(),
                token_type: "Bearer".into(),
                expires_at: 1_790_000_000_000 + 3600 * 1000,
                phone: "81234567890".into(),
                merchant_id: None,
            })
            .unwrap();
        let result = client.get_merchant_profile().await.unwrap();
        assert_eq!(result.profile.id, "M-9");
        assert_eq!(result.profile.outlets.len(), 1);
        assert!(!result.static_qris_matches, "QRIS statis beda dari outlet");
        assert!(result.warnings.iter().any(|w| w.contains("berbeda dari outlet")));
        // merchant_id tersimpan ke sesi.
        assert_eq!(
            client.get_session().unwrap().unwrap().merchant_id.as_deref(),
            Some("M-9")
        );
    }
}
