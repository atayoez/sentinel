// SPDX-FileCopyrightText: 2026 Atay Özcan <atay@oezcan.me>
// SPDX-License-Identifier: GPL-3.0-or-later
//! In-memory remember store.
//!
//! Because the broker is a single long-lived process, grants live entirely
//! in its address space — there is **no on-disk artifact** to forge,
//! tamper with, or roll back. That is what subsumes the on-disk timestamp
//! store's HMAC-integrity work (T3b): the attack surface is removed rather
//! than cryptographically patched. Freshness uses the monotonic
//! [`Instant`] clock, so it is immune to wall-clock manipulation, and the
//! whole store evaporates when the broker stops (fail-closed: clients
//! re-prompt).

use sentinel_broker_proto::{RememberKey, RememberQuery};
use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// Hard ceiling on any remember window, regardless of the ttl a client
/// requests — bounds the blast radius of an over-generous config (mirrors
/// the former on-disk store's 900 s cap).
pub const MAX_REMEMBER: Duration = Duration::from_secs(900);

/// Process-local remember grants, keyed by the full [`RememberKey`].
#[derive(Default)]
pub struct RememberStore {
    inner: Mutex<HashMap<String, Instant>>,
}

impl RememberStore {
    pub fn new() -> Self {
        Self::default()
    }

    /// NUL-separated so no field boundary can be spoofed by embedding a
    /// separator in (say) the command.
    fn keystr(k: &RememberKey) -> String {
        format!(
            "{}\0{}\0{}\0{}",
            k.loginuid, k.sessionid, k.service, k.command
        )
    }

    /// True iff a non-expired grant exists for `q.key` within its (capped)
    /// ttl. Unbindable keys and a zero ttl never match.
    pub fn is_fresh(&self, q: &RememberQuery) -> bool {
        if q.ttl_secs == 0 || !q.key.is_bindable() {
            return false;
        }
        let ttl = Duration::from_secs(q.ttl_secs as u64).min(MAX_REMEMBER);
        let map = self.inner.lock().expect("store mutex poisoned");
        map.get(&Self::keystr(&q.key))
            .is_some_and(|t| t.elapsed() < ttl)
    }

    /// Record/refresh a grant. Rejects unbindable keys (returns `false`).
    /// Opportunistically prunes entries past the hard cap so the map can't
    /// grow unbounded.
    pub fn record(&self, key: &RememberKey) -> bool {
        if !key.is_bindable() {
            return false;
        }
        let mut map = self.inner.lock().expect("store mutex poisoned");
        map.retain(|_, t| t.elapsed() < MAX_REMEMBER);
        map.insert(Self::keystr(key), Instant::now());
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(cmd: &str) -> RememberKey {
        RememberKey {
            loginuid: 1000,
            sessionid: 3,
            service: "sudo".into(),
            command: cmd.into(),
        }
    }

    fn query(cmd: &str, ttl: u32) -> RememberQuery {
        RememberQuery {
            key: key(cmd),
            ttl_secs: ttl,
        }
    }

    #[test]
    fn record_then_fresh() {
        let s = RememberStore::new();
        assert!(
            !s.is_fresh(&query("pacman -Syu", 60)),
            "nothing recorded yet"
        );
        assert!(s.record(&key("pacman -Syu")));
        assert!(s.is_fresh(&query("pacman -Syu", 60)));
    }

    #[test]
    fn full_command_is_the_key() {
        // The argv-binding guarantee at the broker layer: a different
        // command (same program) must not match.
        let s = RememberStore::new();
        s.record(&key("pacman -Syu"));
        assert!(!s.is_fresh(&query("pacman -U /tmp/evil", 60)));
    }

    #[test]
    fn distinct_service_user_session_dont_match() {
        let s = RememberStore::new();
        s.record(&key("pacman -Syu"));
        let mut q = query("pacman -Syu", 60);
        q.key.service = "su".into();
        assert!(!s.is_fresh(&q));
        let mut q = query("pacman -Syu", 60);
        q.key.loginuid = 1001;
        assert!(!s.is_fresh(&q));
        let mut q = query("pacman -Syu", 60);
        q.key.sessionid = 99;
        assert!(!s.is_fresh(&q));
    }

    #[test]
    fn zero_ttl_never_fresh() {
        let s = RememberStore::new();
        s.record(&key("pacman -Syu"));
        assert!(!s.is_fresh(&query("pacman -Syu", 0)));
    }

    #[test]
    fn unbindable_is_rejected() {
        let s = RememberStore::new();
        let mut k = key("pacman -Syu");
        k.loginuid = u32::MAX;
        assert!(!s.record(&k), "unbindable key must not record");
        assert!(!s.is_fresh(&RememberQuery {
            key: k,
            ttl_secs: 60
        }));
    }
}
