//! State percakapan bot per chat — port `src/bot/session.ts`.
//!
//! In-memory; hilang kalau proses restart — checkout yang sudah jadi order
//! tetap aman karena order tersimpan di DB. Satu chat hanya punya SATU alur
//! aktif dalam satu waktu (draft checkout / input yang ditunggu), supaya
//! pesan teks tidak nyasar ke handler yang salah.
//!
//! Perbedaan dari TS: TS memakai `Map` global + `getSession` yang
//! mengembalikan referensi mutable. Di Rust, state dipegang `SessionStore`
//! (dibungkus `Mutex`) yang dipegang bot; akses lewat method
//! `get`/`set`/`clear`/`cancel_awaiting`.

use std::collections::HashMap;
use std::sync::Mutex;

#[derive(Debug, Clone, Default)]
pub struct CheckoutDraft {
    pub product_id: String,
    pub qty: i64,
    pub target: Option<String>,
    pub promo_code: Option<String>,
    pub discount_idr: i64,
    /// Order sudah dibuat (dipakai ulang saat ganti cara bayar / retry).
    pub order_id: Option<String>,
    /// Promo sudah ditukar untuk orderId ini (jangan tukar dua kali).
    pub promo_redeemed: bool,
}

#[derive(Debug, Clone)]
pub enum AwaitingInput {
    /// Kode invite Muse (ala Sondev: kode dulu, baru jumlah)
    Code { product_id: String },
    /// Target produk on-demand (jalur lama)
    /// Varian lama — jalur Sondev baru memakai `Code`; disimpan untuk
    /// kompatibilitas sesi lama + tes.
    #[allow(dead_code)]
    Target { draft: CheckoutDraft },
    /// Ketik jumlah sendiri
    CustomQty { product_id: String },
    /// Ketik kode promo (ada draft)
    Promo { draft: CheckoutDraft },
    /// Ketik kode promo (tanpa draft — cek saja)
    PromoCheck,
    /// Ketik nominal isi saldo
    CustomTopup,
    /// Tulis pesan pertama / lanjutan untuk tiket support.
    /// `order_id` = order yang dipilih di langkah pilih order
    /// (None = "Tanpa order" / pesan lanjutan ke tiket yang sudah terbuka).
    Ticket { order_id: Option<String> },
}

#[derive(Debug, Clone, Default)]
pub struct Session {
    pub awaiting: Option<AwaitingInput>,
    pub draft: Option<CheckoutDraft>,
}

#[derive(Default)]
pub struct SessionStore {
    sessions: Mutex<HashMap<i64, Session>>,
}

impl SessionStore {
    pub fn new() -> Self {
        Self::default()
    }

    /// Ambil salinan sesi chat (kosong kalau belum ada).
    pub fn get(&self, chat_id: i64) -> Session {
        self.sessions
            .lock()
            .map(|m| m.get(&chat_id).cloned().unwrap_or_default())
            .unwrap_or_default()
    }

    /// Simpan sesi chat (mengganti seluruhnya).
    /// Dipakai tes + jalur bot lama; jalur baru memakai `update`.
    #[allow(dead_code)]
    pub fn set(&self, chat_id: i64, session: Session) {
        if let Ok(mut m) = self.sessions.lock() {
            m.insert(chat_id, session);
        }
    }

    /// Ubah sesi chat secara atomik lewat closure.
    pub fn update(&self, chat_id: i64, f: impl FnOnce(&mut Session)) {
        if let Ok(mut m) = self.sessions.lock() {
            let s = m.entry(chat_id).or_default();
            f(s);
        }
    }

    /// Hapus sesi chat. Dipakai tes + jalur bot lama.
    #[allow(dead_code)]
    pub fn clear(&self, chat_id: i64) {
        if let Ok(mut m) = self.sessions.lock() {
            m.remove(&chat_id);
        }
    }

    /// Batalkan input yang sedang ditunggu (dipakai /cancel).
    /// Mengembalikan true kalau memang ada input yang dibatalkan.
    /// Dipakai tes + jalur bot lama.
    #[allow(dead_code)]
    pub fn cancel_awaiting(&self, chat_id: i64) -> bool {
        if let Ok(mut m) = self.sessions.lock() {
            if let Some(s) = m.get_mut(&chat_id) {
                if s.awaiting.is_some() {
                    s.awaiting = None;
                    return true;
                }
            }
        }
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn set_get_clear_round_trip() {
        let store = SessionStore::new();
        assert!(store.get(111).awaiting.is_none());
        assert!(store.get(111).draft.is_none());
        store.set(
            111,
            Session {
                awaiting: Some(AwaitingInput::Code {
                    product_id: "muse-token-1b".to_string(),
                }),
                draft: Some(CheckoutDraft {
                    product_id: "muse-token-1b".to_string(),
                    qty: 5,
                    target: Some("UVI0QD".to_string()),
                    ..Default::default()
                }),
            },
        );
        let s = store.get(111);
        assert!(matches!(s.awaiting, Some(AwaitingInput::Code { .. })));
        assert_eq!(s.draft.as_ref().unwrap().qty, 5);
        // Chat lain tidak terpengaruh.
        assert!(store.get(222).awaiting.is_none());
        store.clear(111);
        assert!(store.get(111).awaiting.is_none());
        assert!(store.get(111).draft.is_none());
    }

    #[test]
    fn cancel_awaiting_only_when_waiting() {
        // S29: /cancel membatalkan input yang ditunggu.
        let store = SessionStore::new();
        assert!(!store.cancel_awaiting(111));
        store.set(
            111,
            Session {
                awaiting: Some(AwaitingInput::CustomTopup),
                draft: None,
            },
        );
        assert!(store.cancel_awaiting(111));
        assert!(store.get(111).awaiting.is_none());
        // Kedua kali: sudah tidak ada yang dibatalkan.
        assert!(!store.cancel_awaiting(111));
    }

    #[test]
    fn update_mutates_atomically() {
        let store = SessionStore::new();
        store.update(111, |s| {
            s.draft = Some(CheckoutDraft {
                product_id: "muse-token-1b".to_string(),
                qty: 1,
                ..Default::default()
            });
        });
        store.update(111, |s| {
            if let Some(d) = &mut s.draft {
                d.qty = 7;
                d.promo_code = Some("TEST10".to_string());
            }
        });
        let s = store.get(111);
        assert_eq!(s.draft.as_ref().unwrap().qty, 7);
        assert_eq!(s.draft.as_ref().unwrap().promo_code.as_deref(), Some("TEST10"));
    }

    #[test]
    fn awaiting_variants_cover_checkout_flow() {
        // Alur S4/S9/S21: Code -> CustomQty -> Promo -> CustomTopup.
        let store = SessionStore::new();
        store.set(
            111,
            Session {
                awaiting: Some(AwaitingInput::Promo {
                    draft: CheckoutDraft {
                        product_id: "muse-token-1b".to_string(),
                        qty: 10,
                        target: Some("UVI0QD".to_string()),
                        ..Default::default()
                    },
                }),
                draft: None,
            },
        );
        assert!(matches!(store.get(111).awaiting, Some(AwaitingInput::Promo { .. })));
    }
}
