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
//!
//! The API accepts only a [`BoundKey`] — a [`RememberKey`] proven bindable
//! — so "act on an unbound grant" is unrepresentable (the check happens
//! once, at the [`RememberKey::bind`] boundary in `dispatch`).

use sentinel_broker_proto::{BoundKey, RememberKey};
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

    /// True iff a non-expired grant exists for `key` within its (capped)
    /// ttl. A zero ttl never matches. Bindability is guaranteed by the
    /// [`BoundKey`] type.
    pub fn is_fresh(&self, key: &BoundKey, ttl_secs: u32) -> bool {
        if ttl_secs == 0 {
            return false;
        }
        let ttl = Duration::from_secs(ttl_secs as u64).min(MAX_REMEMBER);
        let map = self.inner.lock().expect("store mutex poisoned");
        map.get(&Self::keystr(key.key()))
            .is_some_and(|t| t.elapsed() < ttl)
    }

    /// Record/refresh a grant. Opportunistically prunes entries past the
    /// hard cap so the map can't grow unbounded.
    pub fn record(&self, key: &BoundKey) {
        let mut map = self.inner.lock().expect("store mutex poisoned");
        map.retain(|_, t| t.elapsed() < MAX_REMEMBER);
        map.insert(Self::keystr(key.key()), Instant::now());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bound(cmd: &str) -> BoundKey {
        RememberKey {
            loginuid: 1000,
            sessionid: 3,
            service: "sudo".into(),
            command: cmd.into(),
        }
        .bind()
        .expect("test key is bindable")
    }

    #[test]
    fn record_then_fresh() {
        let s = RememberStore::new();
        assert!(
            !s.is_fresh(&bound("pacman -Syu"), 60),
            "nothing recorded yet"
        );
        s.record(&bound("pacman -Syu"));
        assert!(s.is_fresh(&bound("pacman -Syu"), 60));
    }

    #[test]
    fn full_command_is_the_key() {
        // The argv-binding guarantee at the broker layer: a different
        // command (same program) must not match.
        let s = RememberStore::new();
        s.record(&bound("pacman -Syu"));
        assert!(!s.is_fresh(&bound("pacman -U /tmp/evil"), 60));
    }

    #[test]
    fn distinct_service_user_session_dont_match() {
        let s = RememberStore::new();
        s.record(&bound("pacman -Syu"));
        for mutate in [
            |k: &mut RememberKey| k.service = "su".into(),
            |k: &mut RememberKey| k.loginuid = 1001,
            |k: &mut RememberKey| k.sessionid = 99,
        ] {
            let mut k = RememberKey {
                loginuid: 1000,
                sessionid: 3,
                service: "sudo".into(),
                command: "pacman -Syu".into(),
            };
            mutate(&mut k);
            assert!(!s.is_fresh(&k.bind().unwrap(), 60));
        }
    }

    #[test]
    fn zero_ttl_never_fresh() {
        let s = RememberStore::new();
        s.record(&bound("pacman -Syu"));
        assert!(!s.is_fresh(&bound("pacman -Syu"), 0));
    }

    #[test]
    fn unbound_keys_cannot_reach_the_store() {
        // Type-state: an unbindable key yields None, so it can never be
        // passed to record()/is_fresh() — the store only sees BoundKey.
        let mut k = RememberKey {
            loginuid: u32::MAX,
            sessionid: 3,
            service: "sudo".into(),
            command: "pacman -Syu".into(),
        };
        assert!(k.clone().bind().is_none());
        k.loginuid = 1000;
        k.command.clear();
        assert!(k.bind().is_none());
    }
}
