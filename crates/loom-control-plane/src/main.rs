//! `loom-cp` — the control-plane MVP binary.
//!
//! Serves org CRUD, roster with the free-tier seat cap, Ed25519 machine-key
//! enrollment with OAuth 2.1 sender-constrained (DPoP) machine tokens, and
//! membership/license download for hosts. One binary, plain HTTP on `--addr`
//! (default `127.0.0.1:7333`), SQLite storage at `--db`.

use anyhow::{Context, Result};
use clap::Parser;
use loom_control_plane::{AppState, build_app};
use std::net::SocketAddr;
use std::path::Path;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

#[derive(Parser, Debug)]
#[command(name = "loom-cp", about = "Loom control plane MVP")]
struct Args {
    /// Bind address for the HTTP server
    #[arg(long, default_value = "127.0.0.1:7333")]
    addr: SocketAddr,

    /// Path to the SQLite database file
    #[arg(long, default_value = "{loom_dir}/org/control-plane.db")]
    db: String,

    /// Loom ecosystem directory (overrides `LOOM_AGENTS_DIR` / `~/.loom`)
    #[arg(long)]
    loom_dir: Option<String>,

    /// Stripe API key for checkout session creation (sk_test_... or sk_live_...)
    #[arg(long)]
    stripe_api_key: Option<String>,

    /// Stripe webhook signing secret (whsec_...)
    #[arg(long)]
    stripe_webhook_secret: Option<String>,
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::registry()
        .with(tracing_subscriber::EnvFilter::new(
            std::env::var("RUST_LOG").unwrap_or_else(|_| "loom_cp=info".into()),
        ))
        .with(tracing_subscriber::fmt::layer())
        .init();

    let args = Args::parse();

    let loom_dir = args
        .loom_dir
        .or_else(|| std::env::var("LOOM_AGENTS_DIR").ok())
        .unwrap_or_else(loom_core::loom_dir);

    let db = shellexpand::tilde(&args.db).to_string();
    let db = if db == "{loom_dir}/org/control-plane.db" {
        std::path::Path::new(&loom_dir).join("org").join("control-plane.db")
    } else {
        std::path::PathBuf::from(db)
    };

    let state = AppState::new(
        Path::new(&loom_dir),
        &db,
        args.stripe_api_key,
        args.stripe_webhook_secret,
    )
    .await?;
    let app = build_app(state);

    let listener = tokio::net::TcpListener::bind(args.addr)
        .await
        .with_context(|| format!("binding to {}", args.addr))?;
    tracing::info!("loom-cp listening on {}", args.addr);

    axum::serve(listener, app)
        .await
        .context("control-plane server exited")
}
