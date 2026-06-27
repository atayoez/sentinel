// SPDX-FileCopyrightText: 2026 Atay Özcan <atay@oezcan.me>
// SPDX-License-Identifier: GPL-3.0-or-later
//! Connection handling: peer-credential authentication + request dispatch.
//!
//! Only **root** may talk to the broker — the PAM shim runs inside a
//! privileged binary (`sudo`, `polkit-agent-helper-1`, `su`), so its peer
//! uid is 0, vouched by the kernel via `SO_PEERCRED` (snapshotted at
//! `connect()`, unspoofable). Everything is fail-closed: a non-root peer,
//! a bad credential lookup, or any framing error drops the connection
//! without touching the store.

use crate::store::RememberStore;
use nix::sys::socket::{getsockopt, sockopt::PeerCredentials};
use sentinel_broker_proto::{PROTOCOL_VERSION, Request, Response, read_frame, write_frame};
use std::os::fd::AsFd;
use std::os::unix::net::UnixStream;
use std::time::Duration;

/// Bound how long a slow or hostile client can tie up a handler thread.
const IO_TIMEOUT: Duration = Duration::from_secs(5);

/// Handle one client connection: authenticate the peer, read one request,
/// dispatch it, write one response, close. `enforce_peer_root` is always
/// `true` in production; tests set it `false` (the test process isn't root).
pub fn handle(mut stream: UnixStream, store: &RememberStore, enforce_peer_root: bool) {
    let _ = stream.set_read_timeout(Some(IO_TIMEOUT));
    let _ = stream.set_write_timeout(Some(IO_TIMEOUT));

    if enforce_peer_root && !peer_is_root(&stream) {
        return;
    }

    let req: Request = match read_frame(&mut stream) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("sentinel-broker: read: {e}");
            return;
        }
    };
    let resp = dispatch(req, store);
    if let Err(e) = write_frame(&mut stream, &resp) {
        eprintln!("sentinel-broker: write: {e}");
    }
}

/// Kernel-vouched peer-uid check via `SO_PEERCRED`. Fail-closed on any
/// error.
fn peer_is_root(stream: &UnixStream) -> bool {
    match getsockopt(&stream.as_fd(), PeerCredentials) {
        Ok(cred) if cred.uid() == 0 => true,
        Ok(cred) => {
            eprintln!(
                "sentinel-broker: rejecting non-root peer (uid={})",
                cred.uid()
            );
            false
        }
        Err(e) => {
            eprintln!("sentinel-broker: SO_PEERCRED lookup failed: {e}");
            false
        }
    }
}

/// Pure request → response. Separated from I/O so it is trivially testable.
pub fn dispatch(req: Request, store: &RememberStore) -> Response {
    match req {
        Request::Ping => Response::Pong {
            protocol: PROTOCOL_VERSION,
        },
        Request::CheckRemember(q) => Response::Remember {
            fresh: store.is_fresh(&q),
        },
        Request::RecordRemember(key) => {
            if store.record(&key) {
                Response::Recorded
            } else {
                Response::Error("unbindable key".into())
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sentinel_broker_proto::{RememberKey, RememberQuery};
    use std::os::unix::net::UnixListener;
    use std::sync::Arc;
    use std::thread;

    fn key() -> RememberKey {
        RememberKey {
            loginuid: 1000,
            sessionid: 3,
            service: "sudo".into(),
            command: "pacman -Syu".into(),
        }
    }

    #[test]
    fn dispatch_ping_record_check() {
        let store = RememberStore::new();
        assert!(matches!(
            dispatch(Request::Ping, &store),
            Response::Pong { protocol } if protocol == PROTOCOL_VERSION
        ));
        // not fresh before recording
        assert!(matches!(
            dispatch(
                Request::CheckRemember(RememberQuery {
                    key: key(),
                    ttl_secs: 60
                }),
                &store
            ),
            Response::Remember { fresh: false }
        ));
        // record, then fresh
        assert!(matches!(
            dispatch(Request::RecordRemember(key()), &store),
            Response::Recorded
        ));
        assert!(matches!(
            dispatch(
                Request::CheckRemember(RememberQuery {
                    key: key(),
                    ttl_secs: 60
                }),
                &store
            ),
            Response::Remember { fresh: true }
        ));
    }

    #[test]
    fn unbindable_record_errors() {
        let store = RememberStore::new();
        let mut k = key();
        k.command.clear();
        assert!(matches!(
            dispatch(Request::RecordRemember(k), &store),
            Response::Error(_)
        ));
    }

    #[test]
    fn socket_round_trip() {
        // End-to-end over a real Unix socket (peer-root check disabled
        // since the test process isn't root). Exercises framing + dispatch.
        let dir = std::env::temp_dir().join(format!("sentinel-broker-test-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let sock = dir.join("b.sock");
        let _ = std::fs::remove_file(&sock);
        let listener = UnixListener::bind(&sock).unwrap();
        let store = Arc::new(RememberStore::new());

        let srv_store = Arc::clone(&store);
        let srv = thread::spawn(move || {
            // Serve exactly two connections: a record then a check.
            for _ in 0..2 {
                let (stream, _) = listener.accept().unwrap();
                handle(stream, &srv_store, false);
            }
        });

        // 1) record
        let mut c = UnixStream::connect(&sock).unwrap();
        write_frame(&mut c, &Request::RecordRemember(key())).unwrap();
        assert!(matches!(
            read_frame::<_, Response>(&mut c).unwrap(),
            Response::Recorded
        ));
        drop(c);

        // 2) check → fresh
        let mut c = UnixStream::connect(&sock).unwrap();
        write_frame(
            &mut c,
            &Request::CheckRemember(RememberQuery {
                key: key(),
                ttl_secs: 60,
            }),
        )
        .unwrap();
        assert!(matches!(
            read_frame::<_, Response>(&mut c).unwrap(),
            Response::Remember { fresh: true }
        ));
        drop(c);

        srv.join().unwrap();
        let _ = std::fs::remove_dir_all(&dir);
    }
}
