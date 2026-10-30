//! Caché en disco con vencimiento, y limitador de pedidos (token bucket).
//! Sirven para no castigar a AniList ni a las fuentes repitiendo los mismos pedidos.

use crate::paths;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use std::hash::{Hash, Hasher};
use std::path::PathBuf;
use std::time::{Duration, Instant};

#[derive(Serialize, Deserialize)]
struct Entry<T> {
    saved_at: u64,
    value: T,
}

fn entry_path(namespace: &str, key: &str) -> PathBuf {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    key.hash(&mut h);
    paths::http_cache().join(namespace).join(format!("{:016x}.json", h.finish()))
}

/// Devuelve el valor guardado si existe y tiene menos de `ttl`.
pub fn get<T: DeserializeOwned>(namespace: &str, key: &str, ttl: Duration) -> Option<T> {
    let bytes = std::fs::read(entry_path(namespace, key)).ok()?;
    let entry: Entry<T> = serde_json::from_slice(&bytes).ok()?;
    (crate::library::now().saturating_sub(entry.saved_at) < ttl.as_secs()).then_some(entry.value)
}

pub fn put<T: Serialize>(namespace: &str, key: &str, value: &T) {
    let path = entry_path(namespace, key);
    let entry = Entry { saved_at: crate::library::now(), value };
    if let (Some(dir), Ok(bytes)) = (path.parent(), serde_json::to_vec(&entry)) {
        let _ = std::fs::create_dir_all(dir);
        let _ = std::fs::write(path, bytes);
    }
}

/// Token bucket: permite ráfagas de hasta `burst` pedidos y después uno cada `interval`.
pub struct RateLimiter {
    state: tokio::sync::Mutex<(f64, Instant)>,
    burst: f64,
    interval: Duration,
}

impl RateLimiter {
    pub fn new(burst: u32, interval: Duration) -> Self {
        RateLimiter { state: tokio::sync::Mutex::new((burst as f64, Instant::now())), burst: burst as f64, interval }
    }

    /// Espera hasta que haya cupo para un pedido.
    pub async fn acquire(&self) {
        let mut state = self.state.lock().await;
        let (tokens, last) = &mut *state;
        let refill = last.elapsed().as_secs_f64() / self.interval.as_secs_f64();
        *tokens = (*tokens + refill).min(self.burst);
        *last = Instant::now();
        if *tokens < 1.0 {
            let wait = self.interval.mul_f64(1.0 - *tokens);
            tokio::time::sleep(wait).await;
            *tokens = 1.0;
            *last = Instant::now();
        }
        *tokens -= 1.0;
    }

    /// Vacía el cupo y bloquea durante `pause` (cuando el servidor pide frenar).
    pub async fn back_off(&self, pause: Duration) {
        let mut state = self.state.lock().await;
        tokio::time::sleep(pause).await;
        *state = (0.0, Instant::now());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn put_get_and_expiry() {
        put("test", "clave", &vec![1, 2, 3]);
        assert_eq!(get::<Vec<i32>>("test", "clave", Duration::from_secs(60)), Some(vec![1, 2, 3]));
        assert_eq!(get::<Vec<i32>>("test", "clave", Duration::ZERO), None);
        assert_eq!(get::<Vec<i32>>("test", "otra", Duration::from_secs(60)), None);
    }

    #[tokio::test]
    async fn limiter_allows_burst_then_spaces_requests() {
        let limiter = RateLimiter::new(3, Duration::from_millis(100));
        let start = Instant::now();
        for _ in 0..3 {
            limiter.acquire().await;
        }
        assert!(start.elapsed() < Duration::from_millis(50), "la ráfaga no debería esperar");
        limiter.acquire().await;
        limiter.acquire().await;
        assert!(start.elapsed() >= Duration::from_millis(180), "pasada la ráfaga, uno cada 100 ms");
    }
}
