// SPDX-FileCopyrightText: 2026 Mohamed Hammad <Mohamed.Hammad@SpacecraftSoftware.org>
// SPDX-License-Identifier: GPL-3.0-or-later
// Rust guideline compliant 2026-05-18

//! The `sd_notify` datagram protocol, without libsystemd.
//!
//! systemd passes a `Type=notify` service the address of a datagram socket in
//! `NOTIFY_SOCKET`: a filesystem path, or an abstract-namespace name written
//! with a leading `@`. A state change is one datagram of `KEY=value` lines.
//! Fairing sends `READY=1` after its first frame (FRN-SRS-036) and
//! `STOPPING=1` when it begins to shut down. Without the variable there is no
//! one to tell, and every call is a no-op.

use std::ffi::OsStr;
use std::io;
use std::os::linux::net::SocketAddrExt as _;
use std::os::unix::ffi::OsStrExt as _;
use std::os::unix::net::{SocketAddr, UnixDatagram};

/// The service manager's notification socket, if there is one.
#[derive(Debug)]
pub struct Notifier {
    address: Option<SocketAddr>,
}

impl Notifier {
    /// A notifier for the value of `NOTIFY_SOCKET` (read once by the caller).
    ///
    /// An empty, unparsable or absent value yields a notifier that sends nothing.
    #[must_use]
    pub fn new(notify_socket: Option<&OsStr>) -> Self {
        let address = notify_socket.and_then(|value| {
            let bytes = value.as_bytes();
            match bytes.split_first() {
                None => None,
                Some((b'@', name)) => SocketAddr::from_abstract_name(name).ok(),
                Some(_) => SocketAddr::from_pathname(value).ok(),
            }
        });
        Self { address }
    }

    /// Whether a service manager is listening.
    #[cfg(test)]
    #[must_use]
    pub const fn is_active(&self) -> bool {
        self.address.is_some()
    }

    /// Tells the manager startup is complete (FRN-SRS-036).
    ///
    /// # Errors
    ///
    /// The socket error; the caller logs it and carries on, since a missed
    /// notification makes systemd time the unit out, not the boot fail.
    ///
    /// Implements: FRN-SRS-036
    pub fn ready(&self) -> io::Result<()> {
        self.send("READY=1\n")
    }

    /// Tells the manager the service is shutting down.
    ///
    /// # Errors
    ///
    /// The socket error.
    pub fn stopping(&self) -> io::Result<()> {
        self.send("STOPPING=1\n")
    }

    fn send(&self, message: &str) -> io::Result<()> {
        let Some(address) = &self.address else {
            return Ok(());
        };
        let socket = UnixDatagram::unbound()?;
        let sent = socket.send_to_addr(message.as_bytes(), address)?;
        if sent == message.len() {
            Ok(())
        } else {
            Err(io::Error::new(
                io::ErrorKind::WriteZero,
                "notification datagram truncated",
            ))
        }
    }
}

#[cfg(test)]
mod tests {
    use std::ffi::OsString;
    use std::time::Duration;

    use super::*;

    fn receive(socket: &UnixDatagram) -> String {
        socket
            .set_read_timeout(Some(Duration::from_secs(2)))
            .unwrap_or_else(|e| panic!("{e}"));
        let mut buf = [0_u8; 64];
        let n = socket.recv(&mut buf).unwrap_or_else(|e| panic!("{e}"));
        String::from_utf8_lossy(&buf[..n]).into_owned()
    }

    #[test]
    fn ready_reaches_a_path_socket() {
        // Verifies: FRN-SRS-036
        let dir = tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"));
        let path = dir.path().join("notify");
        let listener = UnixDatagram::bind(&path).unwrap_or_else(|e| panic!("{e}"));
        let notifier = Notifier::new(Some(path.as_os_str()));
        assert!(notifier.is_active());
        notifier.ready().unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(receive(&listener), "READY=1\n");
        notifier.stopping().unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(receive(&listener), "STOPPING=1\n");
    }

    #[test]
    fn ready_reaches_an_abstract_socket() {
        let name = format!("fairing-notify-test-{}", std::process::id());
        let address =
            SocketAddr::from_abstract_name(name.as_bytes()).unwrap_or_else(|e| panic!("{e}"));
        let listener = UnixDatagram::bind_addr(&address).unwrap_or_else(|e| panic!("{e}"));
        let value = OsString::from(format!("@{name}"));
        Notifier::new(Some(&value))
            .ready()
            .unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(receive(&listener), "READY=1\n");
    }

    #[test]
    fn without_a_socket_nothing_is_sent() {
        let notifier = Notifier::new(None);
        assert!(!notifier.is_active());
        assert!(notifier.ready().is_ok());
        assert!(!Notifier::new(Some(OsStr::new(""))).is_active());
    }
}
