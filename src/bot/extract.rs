//! Baca kode invite dari apa pun yang dikirim pembeli — ala bot Sondev.
//! Port `extractInviteCode` dari `src/bot/bot.ts` (versi TS).
//!
//! - Kode polos ("UVI0QD"), link invite lengkap, atau teks bagikan Muse.
//! - Yang dikirim ke Sondev sebagai target adalah KODEnya.
//! - Kode polos: satu token 4–12 karakter alfanumerik. Kode Muse TIDAK
//!   selalu mengandung angka — "TZVXJR" (huruf semua) adalah kode valid.
//! - Teks bagikan: cari kode hanya kalau ada konteks invite/kode/muse.ai.
//!   Kata biasa seperti "halo dunia" BUKAN kode.
//! - Quirk TS (dipertahankan verbatim): pada teks bagikan tanpa link, yang
//!   terambil adalah token 4–12 huruf pertama yang bukan stopword —
//!   "Invite friends to Muse, use code UVI0QD!" menghasilkan "FRIENDS".
//!   Teks bagikan Muse asli selalu berisi link invite, jadi jalur link yang
//!   menangani kasus nyata; memperbaiki heuristik ini akan mengubah perilaku
//!   bot, jadi dibiarkan apa adanya (kandidat perbaikan, bukan perubahan
//!   diam-diam).

use regex::Regex;
use std::sync::LazyLock;

static LINK_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)muse\.ai/invite/([A-Za-z0-9_-]+)").unwrap());
static PLAIN_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^[A-Za-z0-9_-]{4,12}$").unwrap());
static CONTEXT_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)invite|kode|muse\.ai").unwrap());
static TOKEN_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"[A-Z0-9]{4,12}").unwrap());

/// Kembalikan kode invite (huruf besar) atau `None` kalau tidak ketemu.
pub fn extract_invite_code(text: &str) -> Option<String> {
    let t = text.trim();
    if t.is_empty() {
        return None;
    }
    if let Some(cap) = LINK_RE.captures(t) {
        return Some(cap[1].to_uppercase());
    }
    // Kode polos: satu token 4–12 karakter alfanumerik.
    if PLAIN_RE.is_match(t) {
        return Some(t.to_uppercase());
    }
    // Teks bagikan: cari kode hanya kalau teksnya memang konteks bagikan.
    if !CONTEXT_RE.is_match(t) {
        return None;
    }
    const STOP: &[&str] = &[
        "MUSE", "HTTPS", "HTTP", "INVITE", "TOKEN", "TOKENS", "COM", "WWW", "AI",
    ];
    let upper = t.to_uppercase();
    TOKEN_RE
        .find_iter(&upper)
        .map(|m| m.as_str())
        .find(|tok| !STOP.contains(tok))
        .map(|tok| tok.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_code_letters_only_is_valid() {
        // Ditemukan saat tes Dim 2026-11-21: TZVXJR (huruf semua) valid.
        assert_eq!(extract_invite_code("TZVXJR").as_deref(), Some("TZVXJR"));
        assert_eq!(extract_invite_code("UVI0QD").as_deref(), Some("UVI0QD"));
        assert_eq!(extract_invite_code("  uvi0qd  ").as_deref(), Some("UVI0QD"));
    }

    #[test]
    fn full_invite_link_is_parsed() {
        assert_eq!(
            extract_invite_code("https://muse.ai/invite/UVI0QD").as_deref(),
            Some("UVI0QD")
        );
        assert_eq!(
            extract_invite_code("Join me! https://muse.ai/invite/abc123").as_deref(),
            Some("ABC123")
        );
    }

    #[test]
    fn share_text_needs_invite_context() {
        // Port TS verbatim: token pertama bukan-stopword yang diambil.
        assert_eq!(
            extract_invite_code("invite: UVI0QD").as_deref(),
            Some("UVI0QD")
        );
        // Quirk TS (dipertahankan supaya perilaku sama): pada teks bagikan
        // tanpa link, yang terambil adalah token pertama bukan-stopword —
        // "FRIENDS", bukan kodenya. Teks bagikan Muse asli selalu berisi
        // link invite, jadi jalur link yang menangani kasus nyata.
        assert_eq!(
            extract_invite_code("Invite friends to Muse, use code UVI0QD!").as_deref(),
            Some("FRIENDS")
        );
        // Tanpa konteks invite/kode/muse.ai: bukan kode.
        assert_eq!(extract_invite_code("halo dunia"), None);
        // "hello" lolos sebagai kode polos 4–12 alnum — persis komentar TS:
        // "Format saja tidak bisa membedakan kata asal-asalan ("hello")
        // dari kode huruf-semua". Kode mati baru ketahuan saat Sondev
        // menebus (lalu refund).
        assert_eq!(extract_invite_code("hello").as_deref(), Some("HELLO"));
        assert_eq!(extract_invite_code(""), None);
    }

    #[test]
    fn too_short_or_too_long_is_not_a_plain_code() {
        assert_eq!(extract_invite_code("ABC"), None);
        assert_eq!(extract_invite_code("ABCDEFGHIJKLM"), None);
        assert_eq!(extract_invite_code("UVI 0QD"), None);
        // Kasus harness S4/S20: "hi" terlalu pendek; spasi saja = kosong.
        assert_eq!(extract_invite_code("hi"), None);
        assert_eq!(extract_invite_code("   "), None);
        assert_eq!(extract_invite_code("a"), None);
    }

    #[test]
    fn plain_code_with_dash_and_underscore() {
        // PLAIN_RE mengizinkan '-' dan '_' (4–12 karakter).
        assert_eq!(extract_invite_code("AB-12_CD").as_deref(), Some("AB-12_CD"));
        assert_eq!(extract_invite_code("ab_12").as_deref(), Some("AB_12"));
        // Link dengan kode ber-dash.
        assert_eq!(
            extract_invite_code("https://muse.ai/invite/ab-12_cd").as_deref(),
            Some("AB-12_CD")
        );
    }
}
