pub mod gopay;
pub mod midtrans;
pub mod midtrans_poller;
pub mod qris;
pub mod qris_card;
pub mod qris_poller;
pub mod qris_settings;

use chrono::{DateTime, SecondsFormat, Utc};

/// Format satu `DateTime<Utc>` seperti `toISOString()` TS.
pub(crate) fn iso_of(dt: DateTime<Utc>) -> String {
    dt.to_rfc3339_opts(SecondsFormat::Millis, true)
}

/// Parse timestamp ISO 8601 yang disimpan di tabel.
pub(crate) fn parse_iso(s: &str) -> anyhow::Result<DateTime<Utc>> {
    Ok(DateTime::parse_from_rfc3339(s)?.with_timezone(&Utc))
}
