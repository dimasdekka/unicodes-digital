#![allow(special_module_name)]
//! Unicodes Digital — Rust rewrite.
//! Satu binary: bot Telegram toko + bot news + web (storefront, Mini App,
//! admin) dalam satu proses, seperti `scripts/vps-start.sh` di versi TS.
//!
//! Supervisor: tiap task (bot toko, bot news, web) diawasi sendiri oleh
//! loop `supervise()`. Kalau satu task selesai / error / panic, task itu
//! di-restart setelah backoff — task lain TIDAK ikut mati. Proses hanya
//! berhenti saat sinyal shutdown (Ctrl-C / SIGTERM).
//!
//! Subcommand sekali jalan:
//!   `cargo run -- set-menu-button` — set Menu Button bot jadi Web App
//!   "Buka Toko" (butuh `NEXT_PUBLIC_APP_URL` publik). Cerminan CLI
//!   `npx tsx src/bot/set-menu-button.ts` di versi TS.

mod bot;
mod catalog;
mod config;
mod db;
mod i18n;
mod lib;
mod web;

use std::future::Future;
use std::pin::Pin;
use std::time::{Duration, Instant};

use tokio::sync::watch;
use tracing::{error, info, warn};

/// Backoff restart (detik): 1, 2, 5, 10, lalu mentok 30.
/// Reset ke 1 dtk kalau task sempat hidup sehat > 60 dtk.
const BACKOFF_SECS: [u64; 5] = [1, 2, 5, 10, 30];
/// Task dianggap "sehat" kalau hidup minimal selama ini — backoff reset.
const HEALTHY_SECS: u64 = 60;
/// Waktu maksimal menunggu supervisor berhenti saat shutdown.
const SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(5);

/// Factory yang membuat ulang future task tiap restart.
/// `Config` dan `Db` keduanya `Clone` (`Db` = `Arc<Mutex<Connection>>`),
/// jadi tiap restart mendapat clone segar — tidak ada state basi.
type TaskFactory = Box<
    dyn FnMut() -> Pin<Box<dyn Future<Output = anyhow::Result<()>> + Send>> + Send,
>;

/// Pesan panic dari payload `JoinError::into_panic()`.
fn panic_message(payload: Box<dyn std::any::Any + Send>) -> String {
    if let Some(s) = payload.downcast_ref::<&str>() {
        (*s).to_string()
    } else if let Some(s) = payload.downcast_ref::<String>() {
        s.clone()
    } else {
        "pesan panic tidak diketahui".to_string()
    }
}

/// Awasi satu task: jalankan, tunggu selesai/error/panic, lalu restart
/// setelah backoff. Berhenti hanya saat `shutdown` bernilai `true`
/// (atau saat task ini di-abort dari luar).
async fn supervise(name: &'static str, mut factory: TaskFactory, mut shutdown: watch::Receiver<bool>) {
    let mut backoff_idx = 0usize;
    loop {
        let started = Instant::now();
        info!("[supervisor] {name}: start.");
        let mut handle = tokio::spawn(factory());
        let outcome = tokio::select! {
            _ = shutdown.changed() => {
                handle.abort();
                info!("[supervisor] {name}: shutdown — task dihentikan.");
                return;
            }
            result = &mut handle => result,
        };
        match outcome {
            Ok(Ok(())) => {
                error!("[supervisor] {name}: selesai tanpa error — restart.");
            }
            Ok(Err(e)) => {
                error!("[supervisor] {name}: error: {e:#} — restart.");
            }
            Err(join_err) => {
                if join_err.is_cancelled() {
                    // Di-abort dari luar (shutdown) — keluar diam-diam.
                    info!("[supervisor] {name}: dibatalkan (shutdown).");
                    return;
                } else if join_err.is_panic() {
                    let msg = panic_message(join_err.into_panic());
                    error!("[supervisor] {name}: PANIC: {msg} — restart.");
                } else {
                    error!("[supervisor] {name}: task gagal: {join_err} — restart.");
                }
            }
        }
        if *shutdown.borrow() {
            return;
        }
        // Task sempat hidup sehat > 60 dtk → backoff reset ke awal.
        if started.elapsed() >= Duration::from_secs(HEALTHY_SECS) {
            backoff_idx = 0;
        }
        let delay = BACKOFF_SECS[backoff_idx.min(BACKOFF_SECS.len() - 1)];
        backoff_idx = backoff_idx.saturating_add(1);
        info!("[supervisor] {name}: restart dalam {delay} dtk...");
        tokio::select! {
            _ = tokio::time::sleep(Duration::from_secs(delay)) => {}
            _ = shutdown.changed() => {
                info!("[supervisor] {name}: shutdown saat backoff — batal restart.");
                return;
            }
        }
    }
}

/// Tunggu sinyal shutdown: Ctrl-C (SIGINT) atau SIGTERM (unix).
async fn shutdown_signal() {
    #[cfg(unix)]
    {
        use tokio::signal::unix::{signal, SignalKind};
        let mut term = match signal(SignalKind::terminate()) {
            Ok(s) => s,
            Err(e) => {
                warn!("tidak bisa pasang handler SIGTERM: {e} — hanya Ctrl-C yang didengar.");
                let _ = tokio::signal::ctrl_c().await;
                return;
            }
        };
        tokio::select! {
            _ = tokio::signal::ctrl_c() => {}
            _ = term.recv() => {}
        }
    }
    #[cfg(not(unix))]
    {
        let _ = tokio::signal::ctrl_c().await;
    }
}

/// Subcommand `set-menu-button`: sekali jalan, lalu keluar.
/// Exit 0 kalau sukses, 1 kalau gagal. Bot/web TIDAK dijalankan.
async fn run_set_menu_button() -> i32 {
    dotenvy::dotenv().ok();
    let cfg = match config::Config::from_env() {
        Ok(cfg) => cfg,
        Err(e) => {
            eprintln!("set-menu-button: gagal baca config: {e:#}");
            return 1;
        }
    };
    match bot::menu_button::set_web_app_menu_button(&cfg).await {
        Ok(msg) => {
            println!("set-menu-button: OK — {msg}");
            0
        }
        Err(e) => {
            eprintln!("set-menu-button: GAGAL — {e:#}");
            1
        }
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive("unicodes_digital=info".parse()?),
        )
        .init();

    // Subcommand sekali jalan — sebelum mode supervisor.
    let mut args = std::env::args();
    args.next(); // nama binary
    if args.next().as_deref() == Some("set-menu-button") {
        std::process::exit(run_set_menu_button().await);
    }

    dotenvy::dotenv().ok();
    let cfg = config::Config::from_env()?;
    let pool = db::open(&cfg.database_path)?;

    info!("Unicodes Digital (Rust) starting");

    let (shutdown_tx, shutdown_rx) = watch::channel(false);
    let mut supervisors = Vec::new();

    // Bot toko: jangan spawn kalau token kosong — run_store_bot langsung
    // bail, jadi tanpa cek ini supervisor akan hot-loop restart.
    if cfg.telegram_bot_token.trim().is_empty() {
        warn!("[supervisor] TELEGRAM_BOT_TOKEN kosong — bot toko tidak di-spawn.");
    } else {
        let cfg_b = cfg.clone();
        let pool_b = pool.clone();
        let factory: TaskFactory = Box::new(move || {
            let cfg = cfg_b.clone();
            let pool = pool_b.clone();
            Box::pin(bot::run_store_bot(cfg, pool))
                as Pin<Box<dyn Future<Output = anyhow::Result<()>> + Send>>
        });
        supervisors.push(tokio::spawn(supervise(
            "store-bot",
            factory,
            shutdown_rx.clone(),
        )));
    }

    // Bot news: sama — jangan spawn kalau token kosong.
    if cfg.telegram_news_bot_token.trim().is_empty() {
        warn!("[supervisor] TELEGRAM_NEWS_BOT_TOKEN kosong — bot news tidak di-spawn.");
    } else {
        let cfg_n = cfg.clone();
        let pool_n = pool.clone();
        let factory: TaskFactory = Box::new(move || {
            let cfg = cfg_n.clone();
            let pool = pool_n.clone();
            Box::pin(bot::run_news_bot(cfg, pool))
                as Pin<Box<dyn Future<Output = anyhow::Result<()>> + Send>>
        });
        supervisors.push(tokio::spawn(supervise(
            "news-bot",
            factory,
            shutdown_rx.clone(),
        )));
    }

    // Web: selalu di-spawn.
    {
        let cfg_w = cfg.clone();
        let pool_w = pool.clone();
        let factory: TaskFactory = Box::new(move || {
            let cfg = cfg_w.clone();
            let pool = pool_w.clone();
            Box::pin(web::serve(cfg, pool))
                as Pin<Box<dyn Future<Output = anyhow::Result<()>> + Send>>
        });
        supervisors.push(tokio::spawn(supervise("web", factory, shutdown_rx)));
    }

    // Proses hanya berhenti saat sinyal shutdown.
    shutdown_signal().await;
    info!("Sinyal shutdown diterima — menghentikan semua task...");
    let _ = shutdown_tx.send(true);
    let stop = async {
        for handle in supervisors {
            let _ = handle.await;
        }
    };
    if tokio::time::timeout(SHUTDOWN_TIMEOUT, stop).await.is_err() {
        warn!("beberapa supervisor belum berhenti dalam {SHUTDOWN_TIMEOUT:?} — keluar paksa.");
    }
    info!("Unicodes Digital (Rust) berhenti dengan tenang.");
    Ok(())
}
