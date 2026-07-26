use std::collections::HashMap;
use std::net::IpAddr;
use std::time::{Duration, Instant};
use tokio::sync::Mutex;

const MAX_REQUESTS: usize = 10;
const WINDOW: Duration = Duration::from_secs(60);

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
