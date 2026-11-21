//! Handler halaman HTML: storefront `/`, Mini App `/tg`, `/admin`,
//! `/admin/tickets*` — port `src/app/page.tsx`, `tg/page.tsx`,
//! `admin/page.tsx`, `admin/tickets/**` (versi TS).

use std::collections::HashMap;

use axum::extract::{Path, Query, State};
use axum::http::HeaderMap;
use axum::response::Html;

use crate::catalog::{get_catalog_product, list_active_products, list_active_products_by_category, list_categories};
use crate::db::Db;
use crate::i18n::rupiah;
use crate::lib::orders::{get_store_order, get_store_stats, list_store_orders};
use crate::lib::product_prices::get_price_override;
use crate::lib::suppliers::{get_supplier, SupplierAdapter, SupplierId};
use crate::lib::support_tickets::{
    count_open_tickets, get_ticket, get_ticket_messages, list_tickets,
};
use crate::web::auth::{admin_password_hash, cookie_value, is_admin_cookie_valid, ADMIN_COOKIE};
use crate::web::views::{
    admin_dashboard, admin_layout, admin_login, layout, storefront as storefront_view,
    ticket_detail, ticket_not_found, tickets_list, tickets_login_prompt, tg_page,
    AdminDashboardData, AdminUserRow, TicketOrderView,
};
use crate::web::AppState;

/// Label status order (ID) untuk kartu "Order terkait" di detail tiket.
fn order_status_label_id(status: &str) -> String {
    match status {
        "delivered" => "Terkirim".to_string(),
        "failed" => "Gagal".to_string(),
        "refunded" => "Di-refund".to_string(),
        "paid" => "Dibayar".to_string(),
        "processing" => "Diproses".to_string(),
        _ => "Menunggu bayar".to_string(),
    }
}

/// Storefront `/` — katalog + filter kategori (port `page.tsx`).
pub async fn storefront_page(
    State(state): State<AppState>,
    Query(q): Query<HashMap<String, String>>,
) -> Html<String> {
    let categories = list_categories(&state.db);
    let active_cat = q
        .get("cat")
        .map(|s| s.as_str())
        .filter(|c| *c == "Semua" || categories.iter().any(|x| x == c))
        .unwrap_or("Semua");
    let products = list_active_products_by_category(&state.db, active_cat);
    Html(layout("Produk Digital", storefront_view(&products, &categories, active_cat)).into_string())
}

/// Mini App `/tg` — katalog + checkout Telegram (port `tg/page.tsx`).
pub async fn tg_page_handler(State(state): State<AppState>) -> Html<String> {
    let products = list_active_products(&state.db);
    Html(tg_page(&products).into_string())
}

fn fmt_num(n: f64) -> String {
    if n.fract() == 0.0 {
        format!("{}", n as i64)
    } else {
        format!("{n}")
    }
}

/// Saldo satu supplier untuk dashboard (port `supplierBalance` di
/// `admin/page.tsx`): gagal cek = teks "belum bisa dicek (...)", bukan error.
async fn supplier_balance(
    state: &AppState,
    id: SupplierId,
) -> (String, serde_json::Value) {
    let supplier = match get_supplier(id, &state.cfg) {
        Ok(s) => s,
        Err(e) => return (format!("belum bisa dicek ({e})"), serde_json::Value::Null),
    };
    match supplier.get_balance().await {
        Ok(b) => {
            let mut parts = vec![
                format!("USD {}", fmt_num(b.usd)),
                format!("IDR {}", fmt_num(b.idr)),
            ];
            if b.credit != 0.0 {
                parts.push(format!("credit {}", fmt_num(b.credit)));
            }
            (parts.join(" · "), b.raw)
        }
        Err(e) => (format!("belum bisa dicek ({e})"), serde_json::Value::Null),
    }
}

fn order_totals(db: &Db) -> (i64, i64) {
    let conn = match db.lock() {
        Ok(g) => g,
        Err(_) => return (0, 0),
    };
    conn.query_row(
        "SELECT COUNT(*), COALESCE(SUM(CASE WHEN status = 'delivered' THEN retail_total_idr ELSE 0 END), 0) FROM store_orders",
        [],
        |row| Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?)),
    )
    .unwrap_or((0, 0))
}

fn list_users(db: &Db) -> Vec<AdminUserRow> {
    let conn = match db.lock() {
        Ok(g) => g,
        Err(_) => return Vec::new(),
    };
    let mut stmt = match conn.prepare(
        "SELECT u.tg_id, u.first_name, u.username, COALESCE(w.balance_idr, 0), u.created_at
         FROM users u LEFT JOIN wallets w ON w.tg_id = u.tg_id
         ORDER BY u.created_at DESC LIMIT 50",
    ) {
        Ok(s) => s,
        Err(_) => return Vec::new(),
    };
    let rows = match stmt.query_map([], |row| {
        Ok(AdminUserRow {
            tg_id: row.get(0)?,
            first_name: row.get(1)?,
            username: row.get(2)?,
            balance_idr: row.get(3)?,
            created_at: row.get(4)?,
        })
    }) {
        Ok(r) => r,
        Err(_) => return Vec::new(),
    };
    rows.filter_map(|r| r.ok()).collect()
}

/// `/admin` — login kalau belum login, dashboard kalau sudah (port
/// `admin/page.tsx`).
pub async fn admin_page(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(q): Query<HashMap<String, String>>,
) -> Html<String> {
    let authed =
        is_admin_cookie_valid(&state.cfg, cookie_value(&headers, ADMIN_COOKIE).as_deref());
    if !authed {
        let configured = admin_password_hash(&state.cfg).is_some();
        let error = q.contains_key("error");
        return Html(admin_layout("Admin", admin_login(configured, error)).into_string());
    }

    let orders = list_store_orders(&state.db, 50);
    let (sondev_text, sondev_raw) = supplier_balance(&state, SupplierId::Sondev).await;
    let (lahastore_text, _) = supplier_balance(&state, SupplierId::LahaStore).await;
    let products = list_active_products(&state.db)
        .into_iter()
        .map(|p| {
            let ov = get_price_override(&state.db, p.id);
            (p, ov)
        })
        .collect();
    let (total_users, delivered_orders) = get_store_stats(&state.db);
    let (total_orders, revenue) = order_totals(&state.db);
    let open_tickets = count_open_tickets(&state.db);
    let users = list_users(&state.db);

    let data = AdminDashboardData {
        total_users,
        total_orders,
        delivered_orders,
        revenue_idr: revenue,
        open_tickets,
        sondev_balance: sondev_text,
        sondev_raw,
        lahastore_balance: lahastore_text,
        products,
        users,
        orders,
    };
    Html(admin_layout("Admin", admin_dashboard(&data)).into_string())
}

/// `/admin/tickets` — daftar tiket, terbuka dulu (port
/// `admin/tickets/page.tsx`).
pub async fn tickets_page(State(state): State<AppState>, headers: HeaderMap) -> Html<String> {
    let authed =
        is_admin_cookie_valid(&state.cfg, cookie_value(&headers, ADMIN_COOKIE).as_deref());
    if !authed {
        return Html(admin_layout("Admin", tickets_login_prompt()).into_string());
    }
    let tickets = list_tickets(&state.db, 100);
    let open_count = count_open_tickets(&state.db);
    Html(admin_layout("Tiket Support", tickets_list(&tickets, open_count)).into_string())
}

/// `/admin/tickets/:id` — detail thread + balas/tutup/buka lagi (port
/// `admin/tickets/[id]/page.tsx`).
pub async fn ticket_detail_page(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Html<String> {
    let authed =
        is_admin_cookie_valid(&state.cfg, cookie_value(&headers, ADMIN_COOKIE).as_deref());
    if !authed {
        return Html(admin_layout("Admin", tickets_login_prompt()).into_string());
    }
    let Some(ticket) = get_ticket(&state.db, &id) else {
        return Html(admin_layout("Tiket Support", ticket_not_found()).into_string());
    };
    let messages = get_ticket_messages(&state.db, &id);
    // Order yang dipilih user saat buka tiket (kalau ada) — tampilkan
    // di kartu "Order terkait". Tiket lama / "Tanpa order" -> tidak ada.
    let linked_order = ticket.order_id.as_deref().and_then(|oid| {
        get_store_order(&state.db, oid).map(|o| {
            let product_name = get_catalog_product(&state.db, &o.catalog_product_id)
                .map(|p| p.name.to_string())
                .unwrap_or_else(|_| o.catalog_product_id.clone());
            TicketOrderView {
                id: o.id.clone(),
                product_name,
                qty: o.quantity,
                status: order_status_label_id(o.status.as_str()),
                total_rp: rupiah(o.retail_total_idr),
            }
        })
    });
    Html(admin_layout(
        &format!("Tiket {}", ticket.id),
        ticket_detail(&ticket, &messages, linked_order.as_ref()),
    )
    .into_string())
}
