use axum::{
    extract::{ConnectInfo, State},
    http::{header::CONTENT_TYPE, HeaderMap, HeaderValue, Method, StatusCode},
    response::Json,
    routing::{get, post},
    Router,
};
use std::{
    net::{IpAddr, SocketAddr},
    sync::Arc,
};
use tower_http::cors::{AllowOrigin, CorsLayer};
use serde::{Deserialize, Serialize};

/// Orígenes autorizados cuando no se define ALLOWED_ORIGINS (lista separada por
/// comas). El 8080 es el dev server de Vite; ver vite.config.ts del repo web.
const DEFAULT_ORIGINS: &str = "https://docs-orion.onrender.com,http://localhost:8080";

mod runner;
mod limiter;

use limiter::RateLimiter;

#[derive(Clone)]
struct AppState {
    limiter: Arc<RateLimiter>,
    /// Versión del compilador que sirve este contenedor. Se resuelve una sola
    /// vez al arrancar: lanzar un proceso por petición para averiguarla sería
    /// absurdo, y el binario no cambia mientras el contenedor vive.
    orion_version: Arc<OrionVersion>,
    /// Si hay un proxy de confianza delante (Render, Railway, nginx). Ver
    /// `ip_cliente`.
    confiar_en_proxy: bool,
}

/// IP para el límite de peticiones. Con TRUST_PROXY, la última de X-Forwarded-For
/// (la que añade el proxy de Render); sin él, la de la conexión.
fn ip_cliente(headers: &HeaderMap, conexion: SocketAddr, confiar_en_proxy: bool) -> IpAddr {
    if confiar_en_proxy {
        let ultima = headers
            .get_all("x-forwarded-for")
            .iter()
            .filter_map(|v| v.to_str().ok())
            .flat_map(|v| v.split(','))
            .filter_map(|ip| ip.trim().parse::<IpAddr>().ok())
            .last();
        if let Some(ip) = ultima {
            return ip;
        }
    }
    conexion.ip()
}

#[derive(Serialize)]
struct OrionVersion {
    /// "v0.1.3", o null si no se pudo determinar.
    version: Option<String>,
    /// Línea completa que imprime `orion --version`, útil cuando el parseo falla.
    raw: String,
}

/// Ejecuta `orion --version` y extrae el "vX.Y.Z".
///
/// Existe porque el Dockerfile fija el binario a un tag concreto y no había
/// forma de comprobar desde fuera qué versión corre en producción: dos releases
/// pueden ser idénticos en comportamiento y aun así importar cuál está
/// desplegado. Sin esto, la única fuente era el panel de Render.
fn resolve_orion_version() -> OrionVersion {
    match std::process::Command::new("orion").arg("--version").output() {
        Ok(out) => {
            let raw = String::from_utf8_lossy(&out.stdout).trim().to_string();
            // "Orion VM v0.1.3 (Rust) — ..." → "v0.1.3"
            let version = raw
                .split_whitespace()
                .find(|w| {
                    w.starts_with('v')
                        && w.len() > 1
                        && w[1..].chars().all(|c| c.is_ascii_digit() || c == '.')
                })
                .map(|s| s.to_string());
            OrionVersion { version, raw }
        }
        Err(e) => OrionVersion {
            version: None,
            raw: format!("no se pudo ejecutar `orion --version`: {e}"),
        },
    }
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
    headers: HeaderMap,
    Json(payload): Json<RunRequest>,
) -> Result<Json<RunResponse>, StatusCode> {
    let ip = ip_cliente(&headers, addr, state.confiar_en_proxy);
    if !state.limiter.check(ip).await {
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

/// Liveness. Se deja como texto plano a propósito: Render lo usa como
/// healthCheckPath y no conviene cambiarle la forma. La información de build
/// vive en /version.
async fn health_handler() -> &'static str {
    "ok"
}

/// Qué compilador sirve este contenedor.
async fn version_handler(State(state): State<AppState>) -> Json<serde_json::Value> {
    Json(serde_json::json!({
        "orion": state.orion_version.version,
        "orion_raw": state.orion_version.raw,
        "api": env!("CARGO_PKG_VERSION"),
    }))
}

#[tokio::main]
async fn main() {
    let orion_version = resolve_orion_version();
    println!("compilador: {}", orion_version.raw);

    let confiar_en_proxy = matches!(
        std::env::var("TRUST_PROXY").unwrap_or_default().trim().to_lowercase().as_str(),
        "1" | "true" | "yes"
    );
    println!("límite por IP: {}", if confiar_en_proxy {
        "X-Forwarded-For (TRUST_PROXY activo)"
    } else {
        "IP de la conexión"
    });

    let state = AppState {
        limiter: Arc::new(RateLimiter::new()),
        orion_version: Arc::new(orion_version),
        confiar_en_proxy,
    };

    let origins: Vec<HeaderValue> = std::env::var("ALLOWED_ORIGINS")
        .unwrap_or_else(|_| DEFAULT_ORIGINS.to_string())
        .split(',')
        .filter_map(|o| match o.trim() {
            "" => None,
            o => match HeaderValue::from_str(o) {
                Ok(v) => Some(v),
                Err(_) => {
                    eprintln!("ALLOWED_ORIGINS: origen inválido, ignorado: {o:?}");
                    None
                }
            },
        })
        .collect();

    if origins.is_empty() {
        panic!("ALLOWED_ORIGINS no contiene ningún origen válido; el navegador rechazaría toda petición");
    }

    println!("CORS permitido para: {origins:?}");

    let cors = CorsLayer::new()
        .allow_origin(AllowOrigin::list(origins))
        .allow_methods([Method::GET, Method::POST])
        .allow_headers([CONTENT_TYPE]);

    let app = Router::new()
        .route("/run", post(run_handler))
        .route("/health", get(health_handler))
        .route("/version", get(version_handler))
        .layer(cors)
        .with_state(state)
        .into_make_service_with_connect_info::<SocketAddr>();

    let port = std::env::var("PORT").unwrap_or_else(|_| "3001".to_string());
    let addr: SocketAddr = format!("0.0.0.0:{port}").parse().unwrap();

    println!("playground-api listening on {addr}");

    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    axum::serve(listener, app).await.unwrap();
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cabeceras(xff: &str) -> HeaderMap {
        let mut h = HeaderMap::new();
        h.insert("x-forwarded-for", HeaderValue::from_str(xff).unwrap());
        h
    }

    const PROXY: &str = "10.0.0.9:443";

    #[test]
    fn detras_del_proxy_cuenta_la_ultima_ip() {
        // El cliente mandó "1.1.1.1" por su cuenta; el proxy añadió la real.
        let ip = ip_cliente(&cabeceras("1.1.1.1, 203.0.113.7"), PROXY.parse().unwrap(), true);
        assert_eq!(ip, "203.0.113.7".parse::<IpAddr>().unwrap());
    }

    #[test]
    fn sin_trust_proxy_se_ignora_la_cabecera() {
        let ip = ip_cliente(&cabeceras("203.0.113.7"), PROXY.parse().unwrap(), false);
        assert_eq!(ip, "10.0.0.9".parse::<IpAddr>().unwrap());
    }

    #[test]
    fn cabecera_ausente_o_rota_usa_la_conexion() {
        let ip = ip_cliente(&HeaderMap::new(), PROXY.parse().unwrap(), true);
        assert_eq!(ip, "10.0.0.9".parse::<IpAddr>().unwrap());
        let ip = ip_cliente(&cabeceras("no-es-una-ip"), PROXY.parse().unwrap(), true);
        assert_eq!(ip, "10.0.0.9".parse::<IpAddr>().unwrap());
    }
}
