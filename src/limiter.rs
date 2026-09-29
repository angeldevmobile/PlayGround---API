use std::collections::HashMap;
use std::net::IpAddr;
use std::time::{Duration, Instant};
use tokio::sync::Mutex;

const MAX_REQUESTS: usize = 10;
const WINDOW: Duration = Duration::from_secs(60);

/// A partir de cuántas IP guardadas se hace limpieza general. Antes una IP que
/// pasaba una vez se quedaba en el mapa para siempre: una fuga de memoria
/// lenta, pero sin fondo en un servicio público.
const LIMPIAR_DESDE: usize = 1024;

pub struct RateLimiter {
    map: Mutex<HashMap<IpAddr, Vec<Instant>>>,
}

impl RateLimiter {
    pub fn new() -> Self {
        Self {
            map: Mutex::new(HashMap::new()),
        }
    }

    pub async fn check(&self, ip: IpAddr) -> bool {
        let mut map = self.map.lock().await;
        let now = Instant::now();

        if map.len() >= LIMPIAR_DESDE {
            map.retain(|_, visitas| {
                visitas.retain(|t| now.duration_since(*t) < WINDOW);
                !visitas.is_empty()
            });
        }

        let entry = map.entry(ip).or_default();
        entry.retain(|t| now.duration_since(*t) < WINDOW);

        if entry.len() < MAX_REQUESTS {
            entry.push(now);
            true
        } else {
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn corta_al_pasar_el_limite() {
        let l = RateLimiter::new();
        let ip: IpAddr = "10.0.0.1".parse().unwrap();
        for _ in 0..MAX_REQUESTS {
            assert!(l.check(ip).await);
        }
        assert!(!l.check(ip).await);
        // Otra IP no se ve afectada.
        assert!(l.check("10.0.0.2".parse().unwrap()).await);
    }
}
