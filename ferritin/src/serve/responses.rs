//! Rendered documentation responses, cached under the identity
//! [`caching`](super::caching) names them by.
//!
//! [`caching`](super::caching) already makes a *revalidating* client cheap: the etag is derived
//! before any rendering, so a matching `If-None-Match` costs a version lookup
//! and a hash. Clients that never revalidate get none of that. A crawler
//! fetching every page cold from a fresh IP renders the same crate root again
//! on every request, and at one crate-root render per request that saturated
//! the pool. Because the identity is computed before rendering, the body can be
//! cached by the server under that same identity. A repeat request from anyone
//! then costs what a 304 does.
//!
//! The cache relies on exactly the property the etags do: a body we cache is a
//! pure function of the inputs its identity hashes. So it caches exactly what we
//! are willing to validate: callers say whether a response qualifies, and one
//! that doesn't (a crate that failed to load, perhaps transiently) gets neither
//! an entry nor an etag.

use super::{Rendered, caching::Validators};
use moka::sync::Cache;
use std::{sync::Arc, time::Duration};
use trillium::Status;

/// How long a cached response is served before it is rendered again.
///
/// This bounds the one input an identity does not name: links into crates the
/// entrypoint was not built against, which resolve to whatever version is
/// latest (see [`caching`](super::caching)'s module docs). It matches how long the Store
/// remembers what a floating version requirement resolved to, so a cached body
/// is never staler than a fresh render could be.
const RESPONSE_TTL: Duration = Duration::from_secs(15 * 60);

/// A response body worth caching, and whether it may be cached.
pub(super) struct Fresh {
    pub(super) status: Status,
    pub(super) body: String,
    /// Whether the body is a pure function of the validators' inputs. False
    /// turns off both the cache entry and the etag.
    pub(super) cacheable: bool,
}

/// A cached response.
struct Cached {
    status: Status,
    body: String,
}

/// Rendered responses keyed by [`Validators::identity`], bounded by total
/// body bytes.
pub(super) struct ResponseCache(Cache<u64, Arc<Cached>>);

impl ResponseCache {
    /// A cache holding at most `max_bytes` of response bodies.
    pub(super) fn new(max_bytes: u64) -> Self {
        Self(
            Cache::builder()
                .max_capacity(max_bytes)
                .weigher(|_, cached: &Arc<Cached>| {
                    u32::try_from(cached.body.len()).unwrap_or(u32::MAX)
                })
                .time_to_live(RESPONSE_TTL)
                .build(),
        )
    }

    /// The response for a request named by `validators`: the cached body when
    /// there is one, otherwise `render`'s, caching it if it qualifies.
    ///
    /// `validators` of `None` means the request has no cheap identity, so it is
    /// rendered and neither cached nor validated.
    ///
    /// Concurrent misses on one identity each render, rather than waiting on one
    /// another: the worker pool's per-lane permits bound that to a handful, and
    /// a redundant render is cheaper than parking a pool thread.
    pub(super) fn respond(
        &self,
        validators: Option<Validators>,
        render: impl FnOnce() -> sonic_rs::Result<Fresh>,
    ) -> sonic_rs::Result<Rendered> {
        if let Some(validators) = &validators
            && let Some(cached) = self.0.get(&validators.identity())
        {
            return Ok(Rendered::Json {
                status: cached.status,
                body: cached.body.clone(),
                validators: Some(validators.clone()),
            });
        }

        let Fresh {
            status,
            body,
            cacheable,
        } = render()?;

        let validators = validators.filter(|_| cacheable);
        if let Some(validators) = &validators {
            self.0.insert(
                validators.identity(),
                Arc::new(Cached {
                    status,
                    body: body.clone(),
                }),
            );
        }

        Ok(Rendered::Json {
            status,
            body,
            validators,
        })
    }
}
