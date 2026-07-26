use axum::{
    extract::{ConnectInfo, State},
    http::StatusCode,
    response::Json,
    routing::{get, post},
    Router,
};
use std::{net::SocketAddr, sync::Arc};
use tower_http::cors::{Any, CorsLayer};
use serde::{Deserialize, Serialize};

mod runner;
mod limiter;

use limiter::RateLimiter;

#[derive(Clone)]
struct AppState {
    limiter: Arc<RateLimiter>,
}

#[derive(Deserialize)]
struct RunRequest {
    code: String,
}

#[derive(Serialize)]
struct RunResponse {
    stdout: String,
    stderr: String,
    ok: bool,
    time_ms: u64,
    /// Tiempo que reporta el intérprete para el programa en sí. Ausente cuando
    /// no llegó a ejecutarse, por ejemplo si expiró el límite de tiempo.
    #[serde(skip_serializing_if = "Option::is_none")]
    exec_ms: Option<f64>,
}

async fn run_handler(
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    State(state): State<AppState>,
    Json(payload): Json<RunRequest>,
) -> Result<Json<RunResponse>, StatusCode> {
    if !state.limiter.check(addr.ip()).await {
        return Err(StatusCode::TOO_MANY_REQUESTS);
    }

    if payload.code.len() > 10 * 1024 {
        return Err(StatusCode::PAYLOAD_TOO_LARGE);
    }

    let result = runner::run_code(&payload.code)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    Ok(Json(RunResponse {
        stdout: result.stdout,
        stderr: result.stderr,
        ok: result.ok,
        time_ms: result.time_ms,
        exec_ms: result.exec_ms,
    }))
}

async fn health_handler() -> &'static str {
    "ok"
}

#[tokio::main]
async fn main() {
    let state = AppState {
        limiter: Arc::new(RateLimiter::new()),
    };

    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);

    let app = Router::new()
        .route("/run", post(run_handler))
        .route("/health", get(health_handler))
        .layer(cors)
        .with_state(state)
        .into_make_service_with_connect_info::<SocketAddr>();

    let port = std::env::var("PORT").unwrap_or_else(|_| "3001".to_string());
    let addr: SocketAddr = format!("0.0.0.0:{port}").parse().unwrap();

    println!("playground-api listening on {addr}");

    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    axum::serve(listener, app).await.unwrap();
}
