// State percakapan bot per chat (in-memory; hilang kalau proses restart —
// checkout yang sudah jadi order tetap aman karena order tersimpan di DB).
// Satu chat hanya punya SATU alur aktif dalam satu waktu (draft checkout /
// input yang ditunggu), supaya pesan teks tidak nyasar ke handler yang salah.

export interface CheckoutDraft {
  productId: string;
  qty: number;
  target?: string;
  promoCode?: string;
  discountIdr: number;
  /** Order sudah dibuat (dipakai ulang saat ganti cara bayar / retry). */
  orderId?: string;
  /** Promo sudah ditukar untuk orderId ini (jangan tukar dua kali). */
  promoRedeemed?: boolean;
}

export type AwaitingInput =
  | { kind: "code"; productId: string } // kode invite Muse (ala Sondev: kode dulu, baru jumlah)
  | { kind: "target"; draft: CheckoutDraft } // target produk on-demand (jalur lama)
  | { kind: "custom_qty"; productId: string } // ketik jumlah sendiri
  | { kind: "promo"; draft: CheckoutDraft } // ketik kode promo (ada draft)
  | { kind: "promo_check" } // ketik kode promo (tanpa draft — cek saja)
  | { kind: "custom_topup" } // ketik nominal isi saldo
  | { kind: "ticket" }; // tulis pesan pertama / lanjutan untuk tiket support

export interface Session {
  awaiting?: AwaitingInput;
  draft?: CheckoutDraft;
}

const sessions = new Map<number, Session>();

export function getSession(chatId: number): Session {
  let s = sessions.get(chatId);
  if (!s) {
    s = {};
    sessions.set(chatId, s);
  }
  return s;
}

export function clearSession(chatId: number): void {
  sessions.delete(chatId);
}

/** Batalkan input yang sedang ditunggu (dipakai /cancel). */
export function cancelAwaiting(chatId: number): boolean {
  const s = sessions.get(chatId);
  if (!s || !s.awaiting) return false;
  delete s.awaiting;
  return true;
}
