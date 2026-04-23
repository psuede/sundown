mod api;
mod auth;
mod bridge;
mod config;

use actix_web::{App, HttpResponse, HttpServer, middleware, web};
use clap::Parser;
use std::path::PathBuf;
use std::sync::Arc;

const INDEX_HTML: &str = include_str!("../static/index.html");
const MANIFEST_JSON: &str = include_str!("../static/manifest.json");
const ICON_SVG: &str = include_str!("../static/icon.svg");
const SW_JS: &str = include_str!("../static/sw.js");

#[derive(Parser)]
#[command(
    name = "sundown-daemon",
    version,
    about = "Remote parental control for timekpr-next"
)]
struct Cli {
    /// Path to config file
    #[arg(short, long, default_value = "/etc/sundown/config.toml")]
    config: PathBuf,

    /// Run in mock mode (no timekpr-next required)
    #[arg(long)]
    mock: bool,

    /// Show the pairing QR code and exit
    #[arg(long)]
    show_pairing: bool,

    /// Generate a new auth token and exit
    #[arg(long)]
    rotate_token: bool,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    let cli = Cli::parse();

    // Load or create config
    let config = if cli.config.exists() {
        config::Config::load(&cli.config)?
    } else {
        let config = config::Config::default_for(&cli.config);
        tracing::info!(
            "no config found, creating default at {}",
            cli.config.display()
        );
        config.save(&cli.config)?;
        config
    };

    // Handle --rotate-token
    if cli.rotate_token {
        let token = auth::generate_token();
        std::fs::write(&config.auth.token_file, &token)?;
        println!("new token: {token}");
        return Ok(());
    }

    // Load or create auth token
    let token = auth::load_or_create_token(&config.auth.token_file)?;

    // Handle --show-pairing
    if cli.show_pairing {
        show_pairing(&config, &token);
        return Ok(());
    }

    // Create the bridge
    let bridge = if cli.mock {
        bridge::TimekprBridge::mock(&config.timekpr.user)
    } else {
        match bridge::TimekprBridge::connect(&config.timekpr.user).await {
            Ok(b) => b,
            Err(e) => {
                tracing::error!("failed to connect to timekpr-next: {e}");
                tracing::info!("hint: run with --mock for testing without timekpr-next");
                return Err(e);
            }
        }
    };

    let state = web::Data::new(api::AppState {
        bridge: Arc::new(bridge),
        token: token.clone(),
    });

    let bind_addr = config.bind_addr();
    tracing::info!("starting sundown on http://{bind_addr}");

    // Show pairing info on first run
    show_pairing(&config, &token);

    HttpServer::new(move || {
        let cors = actix_cors::Cors::permissive();

        App::new()
            .wrap(cors)
            .wrap(middleware::Logger::default())
            .app_data(state.clone())
            .route("/api/status", web::get().to(api::get_status))
            .route("/api/config", web::get().to(api::get_config))
            .route("/api/time", web::post().to(api::adjust_time))
            .route("/api/limits/daily", web::post().to(api::set_limits))
            .route("/api/limits/weekly", web::post().to(api::set_weekly_limit))
            .route(
                "/api/limits/monthly",
                web::post().to(api::set_monthly_limit),
            )
            .route("/api/allowed-days", web::post().to(api::set_allowed_days))
            .route("/api/allowed-hours", web::post().to(api::set_allowed_hours))
            .route(
                "/api/track-inactive",
                web::post().to(api::set_track_inactive),
            )
            .route("/api/hide-tray", web::post().to(api::set_hide_tray_icon))
            .route("/api/lockout-type", web::post().to(api::set_lockout_type))
            .route("/api/lock", web::post().to(api::lock_user))
            .route("/api/unlock", web::post().to(api::unlock_user))
            .route(
                "/manifest.json",
                web::get().to(|| async {
                    HttpResponse::Ok()
                        .content_type("application/json")
                        .body(MANIFEST_JSON)
                }),
            )
            .route(
                "/icon.svg",
                web::get().to(|| async {
                    HttpResponse::Ok()
                        .content_type("image/svg+xml")
                        .body(ICON_SVG)
                }),
            )
            .route(
                "/sw.js",
                web::get().to(|| async {
                    HttpResponse::Ok()
                        .content_type("application/javascript")
                        .body(SW_JS)
                }),
            )
            .route(
                "/",
                web::get().to(|| async {
                    HttpResponse::Ok()
                        .content_type("text/html; charset=utf-8")
                        .body(INDEX_HTML)
                }),
            )
    })
    .bind(&bind_addr)?
    .run()
    .await?;

    Ok(())
}

fn get_local_ip() -> Option<String> {
    let socket = std::net::UdpSocket::bind("0.0.0.0:0").ok()?;
    // Connect to a public IP to determine which local interface is used
    // No actual traffic is sent
    socket.connect("8.8.8.8:80").ok()?;
    let addr = socket.local_addr().ok()?;
    Some(addr.ip().to_string())
}

fn show_pairing(config: &config::Config, token: &str) {
    let port = config.server.port;
    let local_ip = get_local_ip().unwrap_or_else(|| "localhost".to_string());

    let url = format!("http://{}:{}/?token={}", local_ip, port, token);

    println!();
    println!("=== Sundown Pairing ===");
    println!();
    if qr2term::print_qr(&url).is_err() {
        println!("(could not render QR code in terminal)");
    }
    println!();
    println!("scan the QR code or open:");
    println!("  {url}");
    println!();
}
