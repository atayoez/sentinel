// SPDX-FileCopyrightText: 2026 Atay Özcan <atay@oezcan.me>
// SPDX-License-Identifier: GPL-3.0-or-later
//! In-memory "remember" cache for the polkit/GUI auth path.
//!
//! The on-disk root timestamp store (`pam-sentinel::timestamp`) backs the
//! sudo/su paths, but the polkit dialog is shown by *this* user-space
//! agent, which can't read a root-only file. So the agent keeps its own
//! short-lived, in-**memory** cache: after an Allow, repeat requests for
//! the same `(action_id, full command)` auto-allow within the window
//! without a dialog. The key includes the **whole** elevated command
//! (args and all), so a remembered `pkexec true` can never auto-allow
//! `pkexec rm -rf /` — the bug that made one tick blanket all pkexec.
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

    /// Keyed by `(action_id, full command)`. NUL-separated so a command
    /// can't be crafted to collide across the field boundary.
    fn key(action_id: &str, command: &str) -> String {
        format!("{action_id}\0{command}")
    }

    /// True iff a non-expired grant exists for `(action_id, command)`
    /// within `ttl_secs` (capped at [`MAX_REMEMBER`]). `command` is the
    /// **full** elevated command, so a grant for one invocation never
    /// matches a different one of the same program.
    pub async fn is_fresh(&self, action_id: &str, command: &str, ttl_secs: u32) -> bool {
        if ttl_secs == 0 {
            return false;
        }
        let ttl = Duration::from_secs(ttl_secs as u64).min(MAX_REMEMBER);
        let key = Self::key(action_id, command);
        let map = self.inner.lock().await;
        map.get(&key).is_some_and(|t| t.elapsed() < ttl)
    }

    /// Record/refresh a grant for `(action_id, command)`. Opportunistically
    /// prunes entries past the cap so the map can't grow unbounded.
    pub async fn remember(&self, action_id: &str, command: &str) {
        let key = Self::key(action_id, command);
        let mut map = self.inner.lock().await;
        map.retain(|_, t| t.elapsed() < MAX_REMEMBER);
        map.insert(key, Instant::now());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn remembers_exact_command_and_isolates_others() {
        let c = RememberCache::new();
        assert!(!c.is_fresh("act", "pacman -Syu", 60).await);
        c.remember("act", "pacman -Syu").await;
        assert!(c.is_fresh("act", "pacman -Syu", 60).await);
        // SAME program, DIFFERENT args must NOT match — the core fix.
        assert!(!c.is_fresh("act", "pacman -U /tmp/evil", 60).await);
        // different command / different action id must not match
        assert!(!c.is_fresh("act", "id", 60).await);
        assert!(!c.is_fresh("other", "pacman -Syu", 60).await);
        // ttl=0 disables
        assert!(!c.is_fresh("act", "pacman -Syu", 0).await);
    }

    #[tokio::test]
    async fn pkexec_grant_does_not_blanket_other_commands() {
        // The exact incident: one remembered pkexec must not silently
        // authorize a different pkexec command.
        let c = RememberCache::new();
        let exec = "org.freedesktop.policykit.exec";
        c.remember(exec, "true").await;
        assert!(c.is_fresh(exec, "true", 300).await);
        assert!(!c.is_fresh(exec, "rm -rf /", 300).await);
        assert!(!c.is_fresh(exec, "id", 300).await);
    }
}
