// SPDX-FileCopyrightText: 2026 Atay Özcan <atay@oezcan.me>
// SPDX-License-Identifier: GPL-3.0-or-later
//! In-memory "remember" cache for the polkit/GUI auth path.
//!
//! The on-disk root timestamp store (`pam-sentinel::timestamp`) backs the
//! sudo/su paths, but the polkit dialog is shown by *this* user-space
//! agent, which can't read a root-only file. So the agent keeps its own
//! short-lived, in-**memory** cache: after an Allow, repeat requests for
//! the same `(action_id, exe)` auto-allow within the window without a
//! dialog.
//!
//! Being in-memory and per-user-process, a non-root process can't forge
//! an entry (there's no shared file), and the cache evaporates on logout
//! (the agent restarts with the session). The window is hard-capped to
//! bound risk, matching the PAM store.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::Mutex;

/// Hard ceiling on the remember window, matching the PAM store's cap.
const MAX_REMEMBER: Duration = Duration::from_secs(900);

#[derive(Clone, Default)]
pub struct RememberCache {
    inner: Arc<Mutex<HashMap<String, Instant>>>,
}

impl RememberCache {
    pub fn new() -> Self {
        Self::default()
    }

    fn key(action_id: &str, exe: Option<&str>) -> String {
        format!("{action_id}\0{}", exe.unwrap_or(""))
    }

    /// True iff a non-expired grant exists for `(action_id, exe)` within
    /// `ttl_secs` (capped at [`MAX_REMEMBER`]).
    pub async fn is_fresh(&self, action_id: &str, exe: Option<&str>, ttl_secs: u32) -> bool {
        if ttl_secs == 0 {
            return false;
        }
        let ttl = Duration::from_secs(ttl_secs as u64).min(MAX_REMEMBER);
        let key = Self::key(action_id, exe);
        let map = self.inner.lock().await;
        map.get(&key).is_some_and(|t| t.elapsed() < ttl)
    }

    /// Record/refresh a grant for `(action_id, exe)`. Opportunistically
    /// prunes entries past the cap so the map can't grow unbounded.
    pub async fn remember(&self, action_id: &str, exe: Option<&str>) {
        let key = Self::key(action_id, exe);
        let mut map = self.inner.lock().await;
        map.retain(|_, t| t.elapsed() < MAX_REMEMBER);
        map.insert(key, Instant::now());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn remembers_then_distinguishes_action_and_exe() {
        let c = RememberCache::new();
        assert!(!c.is_fresh("act", Some("/usr/bin/x"), 60).await);
        c.remember("act", Some("/usr/bin/x")).await;
        assert!(c.is_fresh("act", Some("/usr/bin/x"), 60).await);
        // different action or exe must not match
        assert!(!c.is_fresh("other", Some("/usr/bin/x"), 60).await);
        assert!(!c.is_fresh("act", Some("/usr/bin/y"), 60).await);
        // ttl=0 disables
        assert!(!c.is_fresh("act", Some("/usr/bin/x"), 0).await);
    }
}
