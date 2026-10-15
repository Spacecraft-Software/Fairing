// SPDX-FileCopyrightText: 2026 Mohamed Hammad <Mohamed.Hammad@SpacecraftSoftware.org>
// SPDX-License-Identifier: GPL-3.0-or-later
// Rust guideline compliant 2026-05-18

//! The answer to a question: one datagram on the question's `Socket=`.
//!
//! systemd's protocol is a single `SOCK_DGRAM` message: `+` followed by the
//! password, or `-` alone to cancel. The datagram is built in a buffer of
//! fixed size on the stack, zeroised as soon as it has been sent or has
//! failed to send, and the secret it was built from is cleared on every
//! path, success or not (FRN-SRS-022, FRN-SRS-024).

use std::io;
use std::os::unix::net::UnixDatagram;
use std::path::Path;

use zeroize::Zeroize;

use crate::secret::{CAPACITY, Secret};

/// Sends `+` and the secret to `socket`, then clears the secret.
///
/// Implements: FRN-SRS-022, FRN-SRS-024, FRN-SRS-025
///
/// # Errors
///
/// The socket error when the datagram could not be sent. The secret is
/// cleared either way.
pub fn answer(socket: &Path, secret: &mut Secret) -> io::Result<()> {
    let mut datagram = [0u8; 1 + CAPACITY];
    datagram[0] = b'+';
    let len = secret.with_bytes(|bytes| {
        datagram[1..=bytes.len()].copy_from_slice(bytes);
        1 + bytes.len()
    });
    secret.clear();
    let sent = send(socket, &datagram[..len]);
    datagram.zeroize();
    sent
}

/// Sends `-` to `socket`: the question was cancelled. Clears the secret.
///
/// Implements: FRN-SRS-022, FRN-SRS-024
///
/// # Errors
///
/// The socket error when the datagram could not be sent.
pub fn cancel(socket: &Path, secret: &mut Secret) -> io::Result<()> {
    secret.clear();
    send(socket, b"-")
}

/// One datagram from an unbound socket, the way systemd's agents reply.
///
/// The path must name a socket itself, not a link to one: `sendto(2)`
/// follows symbolic links, and the agent answers only a socket inside the
/// question directory (maintainer decision T-061).
fn send(socket: &Path, datagram: &[u8]) -> io::Result<()> {
    let stat = rustix::fs::lstat(socket)?;
    if rustix::fs::FileType::from_raw_mode(stat.st_mode) != rustix::fs::FileType::Socket {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "the reply path is not a socket",
        ));
    }
    let sender = UnixDatagram::unbound()?;
    let sent = sender.send_to(datagram, socket)?;
    if sent == datagram.len() {
        Ok(())
    } else {
        Err(io::Error::new(
            io::ErrorKind::WriteZero,
            "the reply datagram was cut short",
        ))
    }
}

#[cfg(test)]
#[allow(
    clippy::disallowed_types,
    clippy::disallowed_macros,
    clippy::disallowed_methods,
    reason = "test fixtures are built with them; the agent is not"
)]
mod tests {
    use super::*;

    fn listener() -> (tempfile::TempDir, std::path::PathBuf, UnixDatagram) {
        let dir = tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"));
        let path = dir.path().join("sck.test");
        let socket = UnixDatagram::bind(&path).unwrap_or_else(|e| panic!("{e}"));
        (dir, path, socket)
    }

    fn typed(text: &str) -> Secret {
        let mut secret = Secret::new();
        for c in text.chars() {
            secret.push(c);
        }
        secret
    }

    #[test]
    fn enter_sends_plus_and_the_secret_in_one_datagram() {
        // Verifies: FRN-SRS-022
        let (_dir, path, socket) = listener();
        let mut secret = typed("open sesame");
        answer(&path, &mut secret).unwrap_or_else(|e| panic!("{e}"));
        let mut buf = [0u8; 1024];
        let n = socket.recv(&mut buf).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(&buf[..n], b"+open sesame");
        assert!(secret.is_empty(), "the secret is cleared once sent");
    }

    #[test]
    fn escape_sends_minus() {
        // Verifies: FRN-SRS-022
        let (_dir, path, socket) = listener();
        let mut secret = typed("half typed");
        cancel(&path, &mut secret).unwrap_or_else(|e| panic!("{e}"));
        let mut buf = [0u8; 16];
        let n = socket.recv(&mut buf).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(&buf[..n], b"-");
        assert!(secret.is_empty());
    }

    #[test]
    fn a_failed_send_still_clears_the_secret() {
        // Verifies: FRN-SRS-024
        let dir = tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"));
        let mut secret = typed("lost");
        assert!(answer(&dir.path().join("nobody-listens"), &mut secret).is_err());
        assert!(secret.is_empty());
    }

    #[test]
    fn a_link_to_a_socket_gets_nothing_and_the_secret_is_cleared() {
        // Maintainer decision T-061: sendto(2) would follow the link.
        let (dir, path, socket) = listener();
        let link = dir.path().join("sck.link");
        std::os::unix::fs::symlink(&path, &link).unwrap_or_else(|e| panic!("{e}"));
        socket
            .set_nonblocking(true)
            .unwrap_or_else(|e| panic!("{e}"));
        let mut secret = typed("not for you");
        let error = answer(&link, &mut secret)
            .err()
            .unwrap_or_else(|| panic!("a link must be refused"));
        assert_eq!(error.kind(), io::ErrorKind::InvalidInput);
        assert!(secret.is_empty());
        let mut buf = [0u8; 64];
        assert!(socket.recv(&mut buf).is_err(), "nothing was sent");
    }

    #[test]
    fn a_full_secret_fits_the_datagram() {
        let (_dir, path, socket) = listener();
        let mut secret = Secret::new();
        while secret.push('x') {}
        answer(&path, &mut secret).unwrap_or_else(|e| panic!("{e}"));
        let mut buf = [0u8; 2 * CAPACITY];
        let n = socket.recv(&mut buf).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(n, 1 + CAPACITY);
    }
}
