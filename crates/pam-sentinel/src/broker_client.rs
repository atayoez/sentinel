// SPDX-FileCopyrightText: 2026 Atay Özcan <atay@oezcan.me>
// SPDX-License-Identifier: GPL-3.0-or-later
//! Thin client to the `sentinel-broker` remember daemon.
//!
//! This is the shim half of the privilege-separation split: instead of
//! owning a root timestamp store in-process, `pam_sentinel` relays the
//! remember decision to the broker over its Unix socket (see
//! `sentinel-broker-proto`).
//!
//! **Fail-closed is the whole contract.** If the broker is missing, down,
//! hung, or misbehaving, `check_remember` returns `false` (→ show the
//! dialog) and `record_remember` is a no-op. Auth never breaks because the
//! broker is unavailable — the worst case is "you get prompted", never
//! "you get let in". I/O is bounded by a short timeout so a hung broker
//! can't stall the auth.

use sentinel_broker_proto::{
    RememberKey, RememberQuery, Request, Response, read_frame, write_frame,
};
use std::os::unix::net::UnixStream;
use std::time::Duration;

/// Default broker socket (matches `sentinel-broker`'s
/// `RuntimeDirectory=sentinel-broker`). Overridable via the
/// `SENTINEL_BROKER_SOCK` env var (used by tests; harmless in production).
const DEFAULT_SOCK: &str = "/run/sentinel-broker/broker.sock";

/// Cap on how long a broker round-trip may delay the auth.
const IO_TIMEOUT: Duration = Duration::from_secs(2);

fn sock_path() -> String {
    std::env::var("SENTINEL_BROKER_SOCK").unwrap_or_else(|_| DEFAULT_SOCK.to_string())
}

/// One connect → write → read round-trip. `None` on *any* failure, so
/// callers fail closed.
fn roundtrip_at(sock: &str, req: &Request) -> Option<Response> {
    let mut s = UnixStream::connect(sock).ok()?;
    s.set_read_timeout(Some(IO_TIMEOUT)).ok()?;
    s.set_write_timeout(Some(IO_TIMEOUT)).ok()?;
    write_frame(&mut s, req).ok()?;
    read_frame::<_, Response>(&mut s).ok()
}

/// Ask the broker whether a fresh grant exists. Fail-closed: only an
/// explicit `Remember { fresh: true }` returns `true`; everything else
/// (unreachable broker, error response, timeout) is `false`.
pub fn check_remember(key: RememberKey, ttl_secs: u32) -> bool {
    let req = Request::CheckRemember(RememberQuery { key, ttl_secs });
    matches!(
        roundtrip_at(&sock_path(), &req),
        Some(Response::Remember { fresh: true })
    )
}

/// Record a grant (after an opt-in Allow). Best-effort: any failure is
/// swallowed (the user simply re-prompts next time).
pub fn record_remember(key: RememberKey) {
    if let Some(Response::Error(e)) = roundtrip_at(&sock_path(), &Request::RecordRemember(key)) {
        log::warn!("sentinel: broker rejected remember record: {e}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;
    use std::os::unix::net::UnixListener;
    use std::thread;

    fn key() -> RememberKey {
        RememberKey {
            loginuid: 1000,
            sessionid: 3,
            service: "sudo".into(),
            command: "pacman -Syu".into(),
        }
    }

    /// Spawn a one-shot mock broker on a temp socket that replies with
    /// `canned` to a single request, and returns its path.
    fn mock_broker(
        tag: &str,
        canned: Response,
    ) -> (std::path::PathBuf, thread::JoinHandle<Option<Request>>) {
        let dir = std::env::temp_dir().join(format!("sentinel-bc-{}-{}", tag, std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let sock = dir.join("b.sock");
        let _ = std::fs::remove_file(&sock);
        let listener = UnixListener::bind(&sock).unwrap();
        let h = thread::spawn(move || {
            let (mut stream, _) = listener.accept().ok()?;
            let req: Request = read_frame(&mut stream).ok()?;
            write_frame(&mut stream, &canned).ok()?;
            Some(req)
        });
        (sock, h)
    }

    #[test]
    fn fresh_true_round_trip() {
        let (sock, h) = mock_broker("fresh", Response::Remember { fresh: true });
        let resp = roundtrip_at(
            sock.to_str().unwrap(),
            &Request::CheckRemember(RememberQuery {
                key: key(),
                ttl_secs: 60,
            }),
        );
        assert!(matches!(resp, Some(Response::Remember { fresh: true })));
        let seen = h.join().unwrap();
        assert!(matches!(seen, Some(Request::CheckRemember(_))));
    }

    #[test]
    fn unreachable_broker_fails_closed() {
        // No server at this path → None → check_remember would be false.
        let resp = roundtrip_at("/nonexistent/sentinel/broker.sock", &Request::Ping);
        assert!(resp.is_none());
    }

    #[test]
    fn error_response_is_not_fresh() {
        let (sock, h) = mock_broker("err", Response::Error("nope".into()));
        let resp = roundtrip_at(
            sock.to_str().unwrap(),
            &Request::CheckRemember(RememberQuery {
                key: key(),
                ttl_secs: 60,
            }),
        );
        // An Error reply must not read as fresh.
        assert!(!matches!(resp, Some(Response::Remember { fresh: true })));
        let _ = h.join();
    }

    #[test]
    fn record_sends_record_request() {
        let (sock, h) = mock_broker("rec", Response::Recorded);
        let _ = roundtrip_at(sock.to_str().unwrap(), &Request::RecordRemember(key()));
        assert!(matches!(
            h.join().unwrap(),
            Some(Request::RecordRemember(_))
        ));
    }

    #[test]
    fn decode_does_not_panic_on_garbage_reply() {
        // Defensive: a reply that isn't a valid frame must error, not panic.
        let mut cur = Cursor::new(vec![0xFFu8; 8]);
        assert!(read_frame::<_, Response>(&mut cur).is_err());
    }
}
