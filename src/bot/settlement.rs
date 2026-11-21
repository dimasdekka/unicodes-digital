//! Implementasi `PaymentSettlement` (trait dari
//! `src/lib/payments/midtrans_poller.rs`) untuk bot.
//!
//! Dipakai KEDUA poller (QRIS GoBiz + Midtrans) saat tick: poller hanya
//! tahu "pembayaran lunas / gagal / kedaluwarsa", efek ke order/dompet
//! didelegasikan ke sini. Ketujuh metode umum didelegasikan ke
//! [`CoreSettlement`] (`crate::lib::settlement`); yang spesifik bot
//! hanyalah `spawn_fulfill_and_notify` (fulfill + notify lewat
//! `BotState`). Semua method memakai fungsi asli kelompok A
//! (`orders` / `wallet`).
//!
//! `spawn_fulfill_and_notify` menjalankan fulfillment di background
//! (tidak menahan tick — fulfillment bisa ~10 menit) lalu `notifyBuyer`.

use std::sync::Arc;

use crate::bot::handlers::fulfill_in_background;
use crate::bot::BotState;
use crate::db::Db;
use crate::lib::payments::midtrans_poller::PaymentSettlement;
use crate::lib::settlement::CoreSettlement;

/// Settlement bot: memegang `BotState` (untuk fulfill + notify).
pub struct BotSettlement {
    pub state: Arc<BotState>,
    core: CoreSettlement,
}

impl BotSettlement {
    pub fn new(state: Arc<BotState>) -> Self {
        Self {
            state,
            core: CoreSettlement,
        }
    }
}

impl PaymentSettlement for BotSettlement {
    fn credit_topup(&self, db: &Db, topup_id: &str) -> anyhow::Result<bool> {
        self.core.credit_topup(db, topup_id)
    }

    fn mark_order_paid(
        &self,
        db: &Db,
        order_id: &str,
        paid_via: &str,
    ) -> anyhow::Result<bool> {
        self.core.mark_order_paid(db, order_id, paid_via)
    }

    fn mark_order_failed(
        &self,
        db: &Db,
        order_id: &str,
        reason: &str,
    ) -> anyhow::Result<()> {
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

    fn order_buyer_tg_id(
        &self,
        db: &Db,
        order_id: &str,
    ) -> anyhow::Result<Option<i64>> {
        self.core.order_buyer_tg_id(db, order_id)
    }

    fn maybe_reward_referrer(
        &self,
        db: &Db,
        referred_tg_id: i64,
    ) -> anyhow::Result<bool> {
        self.core.maybe_reward_referrer(db, referred_tg_id)
    }

    fn spawn_fulfill_and_notify(&self, order_id: String) {
        fulfill_in_background(&self.state, order_id);
    }
}
