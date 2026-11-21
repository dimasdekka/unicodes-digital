//! Satu implementasi bersama untuk 7 metode umum [`PaymentSettlement`]
//! (trait di `crate::lib::payments::midtrans_poller`).
//!
//! `BotSettlement` (bot Telegram) dan `WebSettlement` (webhook Midtrans +
//! tick GoBiz admin) dulu menduplikasi ketujuh metode ini — isinya
//! identik kecuali `mark_order_paid`. Keduanya sekarang mendelegasikan
//! ke [`CoreSettlement`] dan hanya menyimpan `spawn_fulfill_and_notify`
//! masing-masing.
//!
//! ## `mark_order_paid`: bentuk yang disatukan
//!
//! Dipakai bentuk `BotSettlement`: cek status dulu via `get_store_order`
//! (Err kalau order tidak ditemukan, `Ok(false)` kalau sudah bukan
//! `awaiting_payment`), lalu `set_order_paid_via` + `mark_paid`.
//! `mark_paid` sendiri sudah klaim atomik
//! (`UPDATE ... WHERE status = 'awaiting_payment'`) + idempotent, jadi
//! dua pemanggil bersamaan (poller QRIS + poller/webhook Midtrans) tidak
//! bisa menandai lunas dua kali. Bentuk lama `WebSettlement` (panggil
//! `set_order_paid_via` tanpa guard lalu `UPDATE` mentah) punya efek
//! samping yang sama sekali tidak diinginkan: `paid_via` tertimpa bahkan
//! untuk order yang sudah lunas/diproses — bentuk itu dibuang.
//!
//! Catatan ras: dua pemanggil yang sama-sama lolos cek status lalu
//! sama-sama memanggil `mark_paid` — hanya satu yang benar-benar
//! membalik status (klaim atomik), tapi keduanya mengembalikan
//! `Ok(true)`. Dalam praktik tidak terjadi: kedua poller mengklaim
//! dulu baris pembayarannya (`mark_qris_payment_paid` /
//! `mark_midtrans_payment_paid`) + mengecek status sebelum memanggil
//! ke sini, jadi hanya satu jalur yang sampai ke `mark_order_paid`.

use crate::db::Db;
use crate::lib::orders::{
    cancel_order_for_expired_payment, get_store_order, mark_order_failed, mark_paid,
    set_order_paid_via, StoreOrderStatus,
};
use crate::lib::wallet::{mark_topup_paid, maybe_reward_referrer, tg_id_from_buyer_ref};

/// Implementasi bersama 7 metode umum settlement. Tanpa state — semua
/// metode menerima `&Db` dari pemanggil.
#[derive(Debug, Clone, Copy, Default)]
pub struct CoreSettlement;

impl CoreSettlement {
    /// Port `markTopupPaid` — idempotent; Ok(true) kalau saldo benar-benar
    /// masuk, Ok(false) kalau top-up sudah "paid"; Err kalau tidak ditemukan.
    pub fn credit_topup(&self, db: &Db, topup_id: &str) -> anyhow::Result<bool> {
        let (credited, topup) = mark_topup_paid(db, topup_id);
        match topup {
            Some(_) => Ok(credited),
            None => anyhow::bail!("Top-up tidak ditemukan: {topup_id}"),
        }
    }

    /// Port `setOrderPaidVia` + `markPaid` — klaim atomik
    /// `UPDATE ... WHERE status='awaiting_payment'`; Ok(true) kalau order
    /// berubah ke paid, Ok(false) kalau order sudah bukan awaiting_payment
    /// (sudah diproses jalur lain).
    pub fn mark_order_paid(
        &self,
        db: &Db,
        order_id: &str,
        paid_via: &str,
    ) -> anyhow::Result<bool> {
        let order = get_store_order(db, order_id)
            .ok_or_else(|| anyhow::anyhow!("Order tidak ditemukan: {order_id}"))?;
        if order.status != StoreOrderStatus::AwaitingPayment {
            return Ok(false);
        }
        set_order_paid_via(db, order_id, paid_via)?;
        mark_paid(db, order_id)?;
        Ok(true)
    }

    /// Port `markOrderFailed` — HANYA dari status processing/paid
    /// (dari awaiting_payment = no-op, seperti TS).
    pub fn mark_order_failed(
        &self,
        db: &Db,
        order_id: &str,
        reason: &str,
    ) -> anyhow::Result<()> {
        mark_order_failed(db, order_id, reason)?;
        Ok(())
    }

    /// Port `cancelOrderForExpiredPayment` — hanya dari awaiting_payment.
    pub fn cancel_order_expired(
        &self,
        db: &Db,
        order_id: &str,
        reason: &str,
    ) -> anyhow::Result<()> {
        cancel_order_for_expired_payment(db, order_id, reason)?;
        Ok(())
    }

    pub fn order_status(&self, db: &Db, order_id: &str) -> anyhow::Result<Option<String>> {
        Ok(get_store_order(db, order_id).map(|o| o.status.as_str().to_string()))
    }

    pub fn order_buyer_tg_id(
        &self,
        db: &Db,
        order_id: &str,
    ) -> anyhow::Result<Option<i64>> {
        Ok(get_store_order(db, order_id)
            .and_then(|o| tg_id_from_buyer_ref(o.buyer_ref.as_deref())))
    }

    pub fn maybe_reward_referrer(
        &self,
        db: &Db,
        referred_tg_id: i64,
    ) -> anyhow::Result<bool> {
        Ok(maybe_reward_referrer(db, referred_tg_id))
    }
}
