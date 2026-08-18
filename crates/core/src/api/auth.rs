//! Bearer auth + failed-attempt lockout (UC-106).

use std::collections::HashMap;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use axum::extract::{ConnectInfo, FromRequestParts};
use axum::http::request::Parts;
use axum::http::{header, HeaderMap, StatusCode};
use homeai_common::{AuthFail, Paths, Scope, TokenRecord, TokenStore};

/// Failures in this window count toward lockout.
pub const FAIL_WINDOW: Duration = Duration::from_secs(60);
/// Consecutive (windowed) failures that trigger lockout.
pub const FAIL_LIMIT: u32 = 5;
/// How long the IP stays locked after hitting [`FAIL_LIMIT`].
pub const LOCKOUT: Duration = Duration::from_secs(60);

#[derive(Clone)]
pub struct AuthLimiter {
    inner: Arc<Mutex<HashMap<IpAddr, Bucket>>>,
}

#[derive(Default)]
struct Bucket {
    failures: Vec<Instant>,
    locked_until: Option<Instant>,
}

impl AuthLimiter {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub fn blocked(&self, ip: IpAddr) -> Option<Duration> {
        let mut map = self.inner.lock().expect("auth limiter");
        prune_if_large(&mut map);
        let now = Instant::now();
        let bucket = map.get_mut(&ip)?;
        if let Some(until) = bucket.locked_until {
            if until > now {
                return Some(until.saturating_duration_since(now));
            }
            bucket.locked_until = None;
            bucket.failures.clear();
        }
        None
    }

    pub fn fail(&self, ip: IpAddr) {
        let mut map = self.inner.lock().expect("auth limiter");
        let now = Instant::now();
        let bucket = map.entry(ip).or_default();
        bucket
            .failures
            .retain(|t| now.saturating_duration_since(*t) <= FAIL_WINDOW);
        bucket.failures.push(now);
        if bucket.failures.len() as u32 >= FAIL_LIMIT {
            bucket.locked_until = Some(now + LOCKOUT);
        }
    }
}

fn prune_if_large(map: &mut HashMap<IpAddr, Bucket>) {
    if map.len() <= 1024 {
        return;
    }
    let now = Instant::now();
    map.retain(|_, b| {
        b.locked_until.map(|t| t > now).unwrap_or(false)
            || b.failures
                .iter()
                .any(|t| now.saturating_duration_since(*t) <= FAIL_WINDOW)
    });
}

pub fn peer_ip(parts: &Parts) -> IpAddr {
    parts
        .extensions
        .get::<ConnectInfo<SocketAddr>>()
        .map(|c| c.0.ip())
        .unwrap_or(IpAddr::V4(Ipv4Addr::UNSPECIFIED))
}

pub fn bearer_secret(headers: &HeaderMap, query: Option<&str>) -> Option<String> {
    if let Some(raw) = headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
    {
        if let Some(secret) = raw.strip_prefix("Bearer ") {
            if !secret.is_empty() {
                return Some(secret.to_string());
            }
        }
    }
    let q = query.unwrap_or("");
    for pair in q.split('&') {
        if let Some(v) = pair.strip_prefix("access_token=") {
            if !v.is_empty() {
                return Some(v.to_string());
            }
        }
    }
    None
}

pub fn authorize(
    paths: &Paths,
    limiter: &AuthLimiter,
    ip: IpAddr,
    secret: Option<&str>,
    required: Scope,
) -> Result<TokenRecord, (StatusCode, String)> {
    if limiter.blocked(ip).is_some() {
        return Err((
            StatusCode::TOO_MANY_REQUESTS,
            "too many failed auth attempts".into(),
        ));
    }
    let Some(secret) = secret else {
        limiter.fail(ip);
        return Err((StatusCode::UNAUTHORIZED, "missing authorization".into()));
    };
    if secret.is_empty() {
        limiter.fail(ip);
        return Err((StatusCode::UNAUTHORIZED, "missing authorization".into()));
    }
    let store = TokenStore::load(paths.tokens_dir()).map_err(|_| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            "token store unreadable".into(),
        )
    })?;
    match store.authorize(secret, required) {
        Ok(rec) => Ok(rec.clone()),
        Err(AuthFail::Unauthorized) => {
            limiter.fail(ip);
            Err((StatusCode::UNAUTHORIZED, "invalid token".into()))
        }
        Err(AuthFail::Forbidden) => Err((StatusCode::FORBIDDEN, "insufficient scope".into())),
    }
}

pub struct ReadAuth(pub TokenRecord);
pub struct ControlAuth(pub TokenRecord);
pub struct AdminAuth(pub TokenRecord);

impl FromRequestParts<super::ApiState> for ReadAuth {
    type Rejection = (StatusCode, String);

    async fn from_request_parts(
        parts: &mut Parts,
        state: &super::ApiState,
    ) -> Result<Self, Self::Rejection> {
        extract(parts, state, Scope::Read).map(Self)
    }
}

impl FromRequestParts<super::ApiState> for ControlAuth {
    type Rejection = (StatusCode, String);

    async fn from_request_parts(
        parts: &mut Parts,
        state: &super::ApiState,
    ) -> Result<Self, Self::Rejection> {
        extract(parts, state, Scope::Control).map(Self)
    }
}

impl FromRequestParts<super::ApiState> for AdminAuth {
    type Rejection = (StatusCode, String);

    async fn from_request_parts(
        parts: &mut Parts,
        state: &super::ApiState,
    ) -> Result<Self, Self::Rejection> {
        extract(parts, state, Scope::Admin).map(Self)
    }
}

fn extract(
    parts: &Parts,
    state: &super::ApiState,
    required: Scope,
) -> Result<TokenRecord, (StatusCode, String)> {
    let ip = peer_ip(parts);
    let secret = bearer_secret(&parts.headers, parts.uri.query());
    authorize(
        &state.paths,
        &state.limiter,
        ip,
        secret.as_deref(),
        required,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fifth_failure_locks_the_ip() {
        let limiter = AuthLimiter::new();
        let ip = IpAddr::V4(Ipv4Addr::LOCALHOST);
        for _ in 0..FAIL_LIMIT - 1 {
            limiter.fail(ip);
            assert!(limiter.blocked(ip).is_none());
        }
        limiter.fail(ip);
        assert!(limiter.blocked(ip).is_some());
    }

    #[test]
    fn missing_bearer_is_none() {
        assert!(bearer_secret(&HeaderMap::new(), None).is_none());
        assert_eq!(
            bearer_secret(&HeaderMap::new(), Some("access_token=abc")),
            Some("abc".into())
        );
    }
}
