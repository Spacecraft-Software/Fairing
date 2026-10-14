// SPDX-FileCopyrightText: 2026 Mohamed Hammad <Mohamed.Hammad@SpacecraftSoftware.org>
// SPDX-License-Identifier: GPL-3.0-or-later
// Rust guideline compliant 2026-05-18

//! The `ask.*` file: systemd's password request, parsed with fixed bounds.
//!
//! systemd writes one file per request into `/run/systemd/ask-password/`
//! (format: <https://systemd.io/PASSWORD_AGENTS/>). systemd 260, observed on
//! the reference machine, writes these keys in this order under `[Ask]`:
//!
//! ```text
//! [Ask]
//! PID=529298
//! Socket=/run/systemd/ask-password/sck.5eff5758fdc10699
//! AcceptCached=0
//! Echo=0
//! NotAfter=41203969003
//! Silent=0
//! Message=Please enter passphrase for disk …
//! Icon=drive-harddisk
//! Id=cryptsetup:/dev/nvme0n1p2
//! ```
//!
//! `Message=` is written raw: a newline in the message starts a new line of
//! the file. Because `Socket=` and `Echo=` precede it, a message that smuggles
//! in `\nSocket=/elsewhere` would redirect the passphrase under the usual
//! "last value wins" rule. This parser therefore refuses any key that appears
//! twice, and skips (does not join) lines that carry no `=`; the visible
//! message is its first line.
//!
//! Two known edges, both fail-safe: a message line that looks like a section
//! header (`[Foo]`) ends `[Ask]`, so the `Icon=` and `Id=` after it are lost and
//! a repeat of that request shows no rejection notice (FRN-SRS-029); and a
//! message line that is itself `Id=…` or `Icon=…` makes the real key a repeat,
//! so the whole request is refused and left to the tty agent.

use std::io::Read;
use std::num::NonZeroU32;
use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::error::{ParseError, ParseErrorKind};

/// The largest `ask.*` file the agent reads (FRN-SRS-102).
///
/// systemd's own requests are a few hundred bytes; 64 KiB leaves room for any
/// message a person could read on a boot screen. Every allocation this module
/// makes is bounded by it (FRN-SRS-104).
pub const MAX_REQUEST_BYTES: usize = 64 * 1024;

/// The longest socket path `connect(2)` accepts: `sun_path` is 108 bytes,
/// one of which is the terminating NUL.
const MAX_SOCKET_PATH_BYTES: usize = 107;

/// One password request, as systemd wrote it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Request {
    socket: PathBuf,
    message: String,
    id: Option<String>,
    icon: Option<String>,
    pid: Option<NonZeroU32>,
    not_after: Option<Duration>,
    echo: bool,
    accept_cached: bool,
    silent: bool,
}

impl Request {
    /// The `AF_UNIX` datagram socket the answer goes to.
    #[must_use]
    pub fn socket(&self) -> &Path {
        &self.socket
    }

    /// The prompt text: the first line of `Message=`, empty when absent.
    #[must_use]
    pub fn message(&self) -> &str {
        &self.message
    }

    /// The `Id=` that names what the password is for, such as
    /// `cryptsetup:/dev/nvme0n1p2`; a repeated prompt carries the same one.
    #[must_use]
    pub fn id(&self) -> Option<&str> {
        self.id.as_deref()
    }

    /// The `Icon=` name, an XDG icon name.
    #[must_use]
    pub fn icon(&self) -> Option<&str> {
        self.icon.as_deref()
    }

    /// The `PID=` of the asking process.
    #[must_use]
    pub const fn pid(&self) -> Option<NonZeroU32> {
        self.pid
    }

    /// When the request expires, as a `CLOCK_MONOTONIC` reading; `None` when
    /// it never does (`NotAfter=0` or absent).
    #[must_use]
    pub const fn not_after(&self) -> Option<Duration> {
        self.not_after
    }

    /// Whether the input is shown as typed (`Echo=1`, FRN-SRS-026).
    #[must_use]
    pub const fn echo(&self) -> bool {
        self.echo
    }

    /// Whether a cached password may answer the request (`AcceptCached=`).
    #[must_use]
    pub const fn accept_cached(&self) -> bool {
        self.accept_cached
    }

    /// Whether the input is to be hidden entirely, not even masked (`Silent=`).
    #[must_use]
    pub const fn silent(&self) -> bool {
        self.silent
    }
}

/// Reads and parses one `ask.*` file from `source`.
///
/// At most [`MAX_REQUEST_BYTES`] + 1 bytes are read, into one buffer of that
/// fixed capacity, so a larger file costs no more memory than a full one.
///
/// # Errors
///
/// [`ParseErrorKind::Io`] when reading fails, otherwise as [`parse`].
///
/// Implements: FRN-SRS-102, FRN-SRS-104
pub fn read_request(source: impl Read) -> Result<Request, ParseError> {
    let mut bytes = Vec::with_capacity(MAX_REQUEST_BYTES + 1);
    source
        .take(as_u64(MAX_REQUEST_BYTES + 1))
        .read_to_end(&mut bytes)
        .map_err(ParseError::from_io)?;
    parse(&bytes)
}

/// Parses the bytes of one `ask.*` file.
///
/// The size and UTF-8 checks run before any field is looked at.
///
/// # Errors
///
/// [`ParseErrorKind::TooLarge`] above [`MAX_REQUEST_BYTES`];
/// [`ParseErrorKind::NotUtf8`] for any invalid byte;
/// [`ParseErrorKind::DuplicateKey`] when a known key repeats in `[Ask]`;
/// [`ParseErrorKind::InvalidValue`] when a known key's value does not parse
/// or `Socket=` is not an absolute path that fits `sun_path`;
/// [`ParseErrorKind::MissingSocket`] when there is no `Socket=` in `[Ask]`.
///
/// Implements: FRN-SRS-102, FRN-SRS-104
pub fn parse(bytes: &[u8]) -> Result<Request, ParseError> {
    if bytes.len() > MAX_REQUEST_BYTES {
        return Err(ParseError::new(ParseErrorKind::TooLarge));
    }
    let text =
        std::str::from_utf8(bytes).map_err(|_utf8| ParseError::new(ParseErrorKind::NotUtf8))?;

    let mut fields = Fields::default();
    let mut in_ask = false;
    for (index, raw) in text.split('\n').enumerate() {
        let line_number = index + 1;
        let line = raw.trim_matches(is_space);
        if line.is_empty() || line.starts_with('#') || line.starts_with(';') {
            continue;
        }
        if let Some(name) = line.strip_prefix('[').and_then(|l| l.strip_suffix(']')) {
            in_ask = name == "Ask";
            continue;
        }
        if !in_ask {
            continue;
        }
        // A line without `=` is the tail of a multi-line `Message=`.
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        fields.set(
            key.trim_matches(is_space),
            value.trim_matches(is_space),
            line_number,
        )?;
    }
    fields.finish()
}

/// The `[Ask]` keys this agent understands; any other key is ignored.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Key {
    Socket,
    Message,
    Id,
    Icon,
    Pid,
    NotAfter,
    Echo,
    AcceptCached,
    Silent,
}

impl Key {
    fn from_name(name: &str) -> Option<Self> {
        Some(match name {
            "Socket" => Self::Socket,
            "Message" => Self::Message,
            "Id" => Self::Id,
            "Icon" => Self::Icon,
            "PID" => Self::Pid,
            "NotAfter" => Self::NotAfter,
            "Echo" => Self::Echo,
            "AcceptCached" => Self::AcceptCached,
            "Silent" => Self::Silent,
            _ => return None,
        })
    }

    const fn name(self) -> &'static str {
        match self {
            Self::Socket => "Socket",
            Self::Message => "Message",
            Self::Id => "Id",
            Self::Icon => "Icon",
            Self::Pid => "PID",
            Self::NotAfter => "NotAfter",
            Self::Echo => "Echo",
            Self::AcceptCached => "AcceptCached",
            Self::Silent => "Silent",
        }
    }

    const fn bit(self) -> u16 {
        1 << (self as u16)
    }
}

/// The fields seen so far, and which keys have been set.
#[derive(Debug, Default)]
struct Fields {
    seen: u16,
    socket: Option<PathBuf>,
    message: String,
    id: Option<String>,
    icon: Option<String>,
    pid: Option<NonZeroU32>,
    not_after: Option<Duration>,
    echo: bool,
    accept_cached: bool,
    silent: bool,
}

impl Fields {
    fn set(&mut self, name: &str, value: &str, line: usize) -> Result<(), ParseError> {
        let Some(key) = Key::from_name(name) else {
            return Ok(());
        };
        if self.seen & key.bit() != 0 {
            return Err(ParseError::at(
                ParseErrorKind::DuplicateKey,
                key.name(),
                line,
            ));
        }
        self.seen |= key.bit();
        let invalid = || ParseError::at(ParseErrorKind::InvalidValue, key.name(), line);
        match key {
            Key::Socket => self.socket = Some(socket_path(value).ok_or_else(invalid)?),
            Key::Message => value.clone_into(&mut self.message),
            Key::Id => self.id = non_empty(value),
            Key::Icon => self.icon = non_empty(value),
            Key::Pid => self.pid = NonZeroU32::new(value.parse().map_err(|_int| invalid())?),
            Key::NotAfter => {
                let micros: u64 = value.parse().map_err(|_int| invalid())?;
                self.not_after = (micros != 0).then(|| Duration::from_micros(micros));
            }
            Key::Echo => self.echo = boolean(value).ok_or_else(invalid)?,
            Key::AcceptCached => self.accept_cached = boolean(value).ok_or_else(invalid)?,
            Key::Silent => self.silent = boolean(value).ok_or_else(invalid)?,
        }
        Ok(())
    }

    fn finish(self) -> Result<Request, ParseError> {
        let socket = self
            .socket
            .ok_or_else(|| ParseError::new(ParseErrorKind::MissingSocket))?;
        Ok(Request {
            socket,
            message: self.message,
            id: self.id,
            icon: self.icon,
            pid: self.pid,
            not_after: self.not_after,
            echo: self.echo,
            accept_cached: self.accept_cached,
            silent: self.silent,
        })
    }
}

/// systemd's `parse_boolean` vocabulary.
fn boolean(value: &str) -> Option<bool> {
    const TRUE: [&str; 6] = ["1", "yes", "y", "true", "t", "on"];
    const FALSE: [&str; 6] = ["0", "no", "n", "false", "f", "off"];
    if TRUE.iter().any(|word| value.eq_ignore_ascii_case(word)) {
        Some(true)
    } else if FALSE.iter().any(|word| value.eq_ignore_ascii_case(word)) {
        Some(false)
    } else {
        None
    }
}

/// An absolute socket path that `connect(2)` can take.
fn socket_path(value: &str) -> Option<PathBuf> {
    let fits =
        value.starts_with('/') && value.len() <= MAX_SOCKET_PATH_BYTES && !value.contains('\0');
    fits.then(|| PathBuf::from(value))
}

fn non_empty(value: &str) -> Option<String> {
    (!value.is_empty()).then(|| value.to_owned())
}

/// The whitespace systemd's configuration parser strips around keys and values.
const fn is_space(c: char) -> bool {
    matches!(c, ' ' | '\t' | '\r')
}

fn as_u64(n: usize) -> u64 {
    u64::try_from(n).unwrap_or(u64::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The file systemd 260 wrote on the reference machine for
    /// `systemd-ask-password --user --id=fairing:probe --icon=drive-harddisk`
    /// with a message holding a non-ASCII letter, quotes, a tab and a backslash.
    const OBSERVED: &str = "[Ask]\nPID=529298\nSocket=/run/user/1000/systemd/ask-password/sck.5eff5758fdc10699\nAcceptCached=0\nEcho=0\nNotAfter=41203969003\nSilent=0\nMessage=Unlock \u{e9} \"disk\"\ttab\\back\nIcon=drive-harddisk\nId=fairing:probe\n";

    fn parsed(text: &str) -> Request {
        parse(text.as_bytes()).unwrap_or_else(|e| panic!("{e}"))
    }

    fn refused(bytes: &[u8]) -> ParseError {
        parse(bytes)
            .err()
            .unwrap_or_else(|| panic!("expected a refusal"))
    }

    #[test]
    fn parses_the_file_systemd_wrote() {
        let request = parsed(OBSERVED);
        assert_eq!(
            request.socket(),
            Path::new("/run/user/1000/systemd/ask-password/sck.5eff5758fdc10699")
        );
        assert_eq!(request.message(), "Unlock \u{e9} \"disk\"\ttab\\back");
        assert_eq!(request.id(), Some("fairing:probe"));
        assert_eq!(request.icon(), Some("drive-harddisk"));
        assert_eq!(request.pid().map(NonZeroU32::get), Some(529_298));
        assert_eq!(
            request.not_after(),
            Some(Duration::from_micros(41_203_969_003))
        );
        assert!(!request.echo());
        assert!(!request.accept_cached());
        assert!(!request.silent());
    }

    #[test]
    fn echo_one_is_read() {
        let request = parsed("[Ask]\nSocket=/run/s\nEcho=1\n");
        assert!(request.echo());
    }

    #[test]
    fn a_multi_line_message_keeps_its_first_line() {
        // systemd 260 writes a newline in the message raw.
        let request = parsed(
            "[Ask]\nSocket=/run/s\nEcho=1\nNotAfter=41208549140\nMessage=line one\nline two\n",
        );
        assert_eq!(request.message(), "line one");
    }

    #[test]
    fn a_smuggled_socket_is_refused() {
        // A device label "x\nSocket=/tmp/evil" would otherwise redirect the reply.
        let error = refused(
            b"[Ask]\nSocket=/run/systemd/ask-password/sck.1\nMessage=x\nSocket=/tmp/evil\n",
        );
        assert_eq!(error.kind(), ParseErrorKind::DuplicateKey);
        assert_eq!(error.key(), Some("Socket"));
        assert_eq!(error.line(), Some(4));
    }

    #[test]
    fn a_smuggled_echo_is_refused() {
        let error = refused(b"[Ask]\nSocket=/run/s\nEcho=0\nMessage=x\nEcho=1\n");
        assert_eq!(error.kind(), ParseErrorKind::DuplicateKey);
        assert_eq!(error.key(), Some("Echo"));
    }

    #[test]
    fn exactly_the_bound_is_accepted_and_one_more_byte_is_not() {
        // Verifies: FRN-SRS-102
        let head = "[Ask]\nSocket=/run/s\n#";
        let mut at_bound = head.as_bytes().to_vec();
        at_bound.resize(MAX_REQUEST_BYTES, b'x');
        assert!(parse(&at_bound).is_ok());
        at_bound.push(b'x');
        assert_eq!(refused(&at_bound).kind(), ParseErrorKind::TooLarge);
    }

    #[test]
    fn size_is_checked_before_utf8() {
        // Verifies: FRN-SRS-102
        let bytes = vec![0xff; MAX_REQUEST_BYTES + 1];
        assert_eq!(refused(&bytes).kind(), ParseErrorKind::TooLarge);
    }

    #[test]
    fn a_non_utf8_byte_anywhere_is_refused_before_fields() {
        // Verifies: FRN-SRS-102
        // The bad byte sits in an unknown key's value after a valid Socket=,
        // so only a whole-file check can refuse it.
        let error = refused(b"[Ask]\nSocket=/run/s\nX-Vendor=\xc3\x28\n");
        assert_eq!(error.kind(), ParseErrorKind::NotUtf8);
    }

    #[test]
    fn read_request_stops_one_byte_past_the_bound() {
        // Verifies: FRN-SRS-102
        let endless = std::io::repeat(b'#');
        let error = read_request(endless)
            .err()
            .unwrap_or_else(|| panic!("expected a refusal"));
        assert_eq!(error.kind(), ParseErrorKind::TooLarge);
    }

    #[test]
    fn read_request_parses_a_reader() {
        let request = read_request(OBSERVED.as_bytes()).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(request.id(), Some("fairing:probe"));
    }

    #[test]
    fn socket_must_be_absolute_and_fit_sun_path() {
        assert_eq!(
            refused(b"[Ask]\nSocket=relative/sck\n").kind(),
            ParseErrorKind::InvalidValue
        );
        let long = format!("[Ask]\nSocket=/{}\n", "s".repeat(MAX_SOCKET_PATH_BYTES));
        let error = refused(long.as_bytes());
        assert_eq!(
            (error.kind(), error.key()),
            (ParseErrorKind::InvalidValue, Some("Socket"))
        );
        let fits = format!("[Ask]\nSocket=/{}\n", "s".repeat(MAX_SOCKET_PATH_BYTES - 1));
        assert!(parse(fits.as_bytes()).is_ok());
        assert_eq!(
            refused(b"[Ask]\nSocket=/run/\0s\n").kind(),
            ParseErrorKind::InvalidValue
        );
    }

    #[test]
    fn socket_is_required_and_only_counts_inside_ask() {
        assert_eq!(
            refused(b"[Ask]\nMessage=hi\n").kind(),
            ParseErrorKind::MissingSocket
        );
        assert_eq!(
            refused(b"[Other]\nSocket=/run/s\n").kind(),
            ParseErrorKind::MissingSocket
        );
        assert_eq!(refused(b"").kind(), ParseErrorKind::MissingSocket);
        let request = parsed("[Other]\nSocket=/elsewhere\n[Ask]\nSocket=/run/s\n");
        assert_eq!(request.socket(), Path::new("/run/s"));
    }

    #[test]
    fn booleans_follow_systemd() {
        for word in ["1", "yes", "Y", "true", "t", "ON"] {
            assert!(
                parsed(&format!("[Ask]\nSocket=/s\nSilent={word}\n")).silent(),
                "{word}"
            );
        }
        for word in ["0", "no", "n", "FALSE", "f", "off"] {
            assert!(
                !parsed(&format!("[Ask]\nSocket=/s\nSilent={word}\n")).silent(),
                "{word}"
            );
        }
        let error = refused(b"[Ask]\nSocket=/s\nAcceptCached=maybe\n");
        assert_eq!(
            (error.kind(), error.key()),
            (ParseErrorKind::InvalidValue, Some("AcceptCached"))
        );
    }

    #[test]
    fn numbers_are_checked() {
        assert_eq!(
            parsed("[Ask]\nSocket=/s\nNotAfter=0\nPID=0\n").not_after(),
            None
        );
        assert_eq!(parsed("[Ask]\nSocket=/s\nPID=0\n").pid(), None);
        let error = refused(b"[Ask]\nSocket=/s\nNotAfter=-1\n");
        assert_eq!(error.key(), Some("NotAfter"));
        let error = refused(b"[Ask]\nSocket=/s\nPID=99999999999\n");
        assert_eq!(error.key(), Some("PID"));
    }

    #[test]
    fn whitespace_comments_and_unknown_keys_are_tolerated() {
        let request = parsed(
            "# comment\n; comment\n\n[Ask]\r\n  Socket = /run/s \r\nX-New=1\nMessage=\tpad \n",
        );
        assert_eq!(request.socket(), Path::new("/run/s"));
        assert_eq!(request.message(), "pad");
    }

    #[test]
    fn errors_name_the_key_but_never_the_value() {
        let error = refused(b"[Ask]\nSocket=/s\nEcho=hunter2\n");
        let shown = error.to_string();
        assert!(shown.contains("Echo"), "{shown}");
        assert!(!shown.contains("hunter2"), "{shown}");
        let debug = format!("{error:?}");
        assert!(!debug.contains("hunter2"), "{debug}");
    }
}
