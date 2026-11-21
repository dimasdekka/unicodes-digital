//! Implementasi nyata [`PaymentSettlement`] (kelompok C) untuk jalur web:
//! webhook Midtrans + tick GoBiz manual dari panel admin.
//!
//! Ketujuh metode umum didelegasikan ke [`CoreSettlement`]
//! (`crate::lib::settlement`); yang spesifik web hanyalah
//! `spawn_fulfill_and_notify` (menjalankan `fulfill_order` +
//! `notify_buyer` di background — bisa ~10 menit — jangan pernah
//! menahan tick/webhook). Semua efek samping didelegasikan ke orders /
//! wallet / notify yang sudah ada (kelompok A) — tidak ada logika
//! fulfillment / saldo yang diduplikasi di sini.

use std::sync::Arc;

use crate::config::Config;
use crate::db::Db;
use crate::lib::notify::{notify_buyer, NotifyCtx};
use crate::lib::orders::{fulfill_order, get_store_order, StoreOrderStatus};
use crate::lib::payments::midtrans_poller::PaymentSettlement;
use crate::lib::settlement::CoreSettlement;
use crate::lib::suppliers::{get_supplier, SupplierId};

/// Settlement untuk proses web. `db` + `cfg` disimpan supaya
/// `spawn_fulfill_and_notify` (sync, tanpa `&Db`) bisa menjalankan
/// fulfillment di task background.
#[derive(Debug, Clone)]
pub struct WebSettlement {
    pub db: Db,
    pub cfg: Config,
    core: CoreSettlement,
}

impl WebSettlement {
    pub fn new(db: Db, cfg: Config) -> Self {
        Self {
            db,
            cfg,
            core: CoreSettlement,
        }
    }

    pub fn as_arc(&self) -> Arc<Self> {
        Arc::new(self.clone())
    }
}

impl PaymentSettlement for WebSettlement {
    fn credit_topup(&self, db: &Db, topup_id: &str) -> anyhow::Result<bool> {
        self.core.credit_topup(db, topup_id)
    }

    fn mark_order_paid(&self, db: &Db, order_id: &str, paid_via: &str) -> anyhow::Result<bool> {
        self.core.mark_order_paid(db, order_id, paid_via)
    }

    fn mark_order_failed(&self, db: &Db, order_id: &str, reason: &str) -> anyhow::Result<()> {
        self.core.mark_order_failed(db, order_id, reason)
    }

    fn cancel_order_expired(
        &self,
        db: &Db,
        order_id: &str,
        reason: &str,
    ) -> anyhow::Result<()> {
        self.core.cancel_order_expired(db, order_id, reason)
    }

    fn order_status(&self, db: &Db, order_id: &str) -> anyhow::Result<Option<String>> {
        self.core.order_status(db, order_id)
    }

    fn order_buyer_tg_id(&self, db: &Db, order_id: &str) -> anyhow::Result<Option<i64>> {
        self.core.order_buyer_tg_id(db, order_id)
    }

    fn maybe_reward_referrer(&self, db: &Db, referred_tg_id: i64) -> anyhow::Result<bool> {
        self.core.maybe_reward_referrer(db, referred_tg_id)
    }

    fn spawn_fulfill_and_notify(&self, order_id: String) {
        let db = self.db.clone();
        let cfg = self.cfg.clone();
        tokio::spawn(async move {
            let Some(order) = get_store_order(&db, &order_id) else {
                tracing::error!("[web-settlement] order {order_id} tidak ditemukan untuk fulfill");
                return;
            };
            let Some(supplier_id) = SupplierId::from_str(&order.supplier) else {
                tracing::error!(
                    "[web-settlement] supplier tidak dikenal '{}' untuk order {order_id}",
                    order.supplier
                );
                return;
            };
            let supplier = match get_supplier(supplier_id, &cfg) {
                Ok(s) => s,
                Err(e) => {
                    tracing::error!("[web-settlement] get_supplier gagal: {e}");
                    return;
                }
            };
            let notify = NotifyCtx::from_config(&cfg);
            match fulfill_order(&db, Some(&notify), &supplier, &order_id).await {
                Ok(done) => {
                    // Hanya kirim notifikasi kalau order benar-benar selesai
                    // (fulfill mengembalikan order apa pun statusnya).
                    if done.status == StoreOrderStatus::Delivered
                        || done.status == StoreOrderStatus::Failed
                    {
                        notify_buyer(&notify, &db, &done).await;
                    }
                }
                Err(e) => {
                    tracing::error!("[web-settlement] fulfill {order_id} gagal: {e}");
                }
            }
        });
    }
}
