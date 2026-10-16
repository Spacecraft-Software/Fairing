// SPDX-FileCopyrightText: 2026 Mohamed Hammad <Mohamed.Hammad@SpacecraftSoftware.org>
// SPDX-License-Identifier: GPL-3.0-or-later
// Rust guideline compliant 2026-05-18

//! The `ask.*` file systemd writes for each password question.
//!
//! The file is INI-shaped: an `[Ask]` section of `Key=Value` lines. It is
//! checked whole before any field is read: more than [`MAX_FILE`] bytes, or a
//! single byte that is not UTF-8, and it is refused (FRN-SRS-102). Every field
//! is then copied into a buffer of fixed capacity, so nothing the parser keeps
//! has a size that depends on the file (FRN-SRS-104).
//!
//! What a field over its capacity means depends on the field. `Message=` is
//! only shown, so it is cut at a character boundary. `Socket=` and `Id=` decide
//! where the secret goes and which question it answers, so they are refused:
//! a cut path would name another socket.
//!
//! A key that appears twice in `[Ask]` refuses the whole file. systemd 260,
//! observed on the reference machine, writes `PID=`, `Socket=`, `AcceptCached=`,
//! `Echo=`, `NotAfter=`, `Silent=`, `Message=`, `Icon=` and `Id=` in that order,
//! and writes `Message=` raw: a newline in the message starts a new line of the
//! file. Under "last value wins" a device label holding `\nSocket=/elsewhere`
//! would redirect the passphrase, so a repeat is never resolved, only refused.
//! Lines without `=` are skipped, so a multi-line message shows its first line.

use core::fmt;
use core::str;

/// Largest `ask.*` file the parser reads (FRN-SRS-102).
///
/// systemd's own files are a few hundred bytes; 64 KiB leaves room for any
/// message while keeping the read buffer a fixed, small allocation.
pub const MAX_FILE: usize = 64 * 1024;

/// Bytes kept of `Message=`. Longer messages are cut at a character boundary.
///
/// Enough for systemd-cryptsetup's "Please enter passphrase for disk … on …"
/// with long device names; the prompt region could not show more legibly.
pub const MESSAGE_CAPACITY: usize = 512;

/// Bytes allowed in `Id=`; a longer one is refused.
///
/// systemd-cryptsetup writes `cryptsetup:` and the device path; 256 bytes
/// covers by-id and by-uuid paths with room to spare.
pub const ID_CAPACITY: usize = 256;

/// Bytes allowed in `Socket=`; a longer one is refused.
///
/// `sun_path` in `struct sockaddr_un` is 108 bytes including its terminating
/// NUL, so no usable socket path is longer than 107.
pub const SOCKET_CAPACITY: usize = 107;

/// Why an `ask.*` file was refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AskError {
    /// More than [`MAX_FILE`] bytes.
    TooLarge,
    /// A byte that is not part of valid UTF-8.
    NotUtf8,
    /// No `Socket=` line, so there is nowhere to reply.
    NoSocket,
    /// `Socket=` is not an absolute path or is longer than [`SOCKET_CAPACITY`].
    BadSocket,
    /// `Id=` is longer than [`ID_CAPACITY`].
    IdTooLong,
    /// `NotAfter=`, `PID=` or `Echo=` does not parse.
    BadNumber,
    /// A key the parser reads appears twice in `[Ask]`.
    RepeatedKey,
}

impl fmt::Display for AskError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::TooLarge => "the ask file is larger than 64 KiB",
            Self::NotUtf8 => "the ask file is not UTF-8",
            Self::NoSocket => "the ask file names no reply socket",
            Self::BadSocket => "the ask file's reply socket is not a usable absolute path",
            Self::IdTooLong => "the ask file's Id= is too long",
            Self::BadNumber => "the ask file has a malformed number",
            Self::RepeatedKey => "the ask file repeats a key",
        })
    }
}

impl std::error::Error for AskError {}

/// UTF-8 text in a buffer of fixed capacity `N`.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Text<const N: usize> {
    bytes: [u8; N],
    len: usize,
}

impl<const N: usize> Text<N> {
    const fn empty() -> Self {
        Self {
            bytes: [0; N],
            len: 0,
        }
    }

    /// Copies `value` whole; `None` if it does not fit.
    pub(crate) fn exact(value: &str) -> Option<Self> {
        let mut text = Self::empty();
        let dst = text.bytes.get_mut(..value.len())?;
        dst.copy_from_slice(value.as_bytes());
        text.len = value.len();
        Some(text)
    }

    /// Copies as much of `value` as fits, cut at a character boundary.
    fn cut(value: &str) -> Self {
        let mut end = value.len().min(N);
        while !value.is_char_boundary(end) {
            end -= 1;
        }
        let mut text = Self::empty();
        text.bytes[..end].copy_from_slice(&value.as_bytes()[..end]);
        text.len = end;
        text
    }

    /// The text.
    #[must_use]
    pub fn as_str(&self) -> &str {
        // Only whole UTF-8 strings, or prefixes cut at a character boundary,
        // are ever copied in, so this never fails; an empty string is the
        // harmless answer if it somehow did.
        str::from_utf8(&self.bytes[..self.len]).unwrap_or_default()
    }

    /// Whether the text is empty.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.len == 0
    }
}

impl<const N: usize> fmt::Debug for Text<N> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(self.as_str(), f)
    }
}

/// One password question, as systemd asked it.
///
/// Implements: FRN-SRS-102, FRN-SRS-104
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Ask {
    /// `Id=`: which question this is, so a repeat can be recognised (FRN-SRS-029).
    pub id: Text<ID_CAPACITY>,
    /// `Message=`: what to show, possibly cut (FRN-SRS-021).
    pub message: Text<MESSAGE_CAPACITY>,
    /// `Socket=`: the datagram socket the answer goes to (FRN-SRS-022).
    pub socket: Text<SOCKET_CAPACITY>,
    /// `NotAfter=`: `CLOCK_MONOTONIC` microseconds after which the question
    /// expires, `None` when it does not (FRN-SRS-023).
    pub not_after_us: Option<u64>,
    /// `Echo=1`: the input may be shown (FRN-SRS-026).
    pub echo: bool,
    /// `Silent=1`: show nothing of the input, not even how much was typed,
    /// as systemd's own agents do for crypttab `password-echo=no`.
    pub silent: bool,
    /// `PID=`: the process that asked, `None` when absent.
    pub pid: Option<u32>,
}

impl Ask {
    /// Parses one `ask.*` file.
    ///
    /// Size and encoding are checked first, over the whole file, before any
    /// field is looked at. Keys outside `[Ask]`, unknown keys and comment
    /// lines are ignored. A key this parser reads that appears twice is
    /// refused rather than resolved, unlike systemd's own parser: see the
    /// module documentation.
    ///
    /// Implements: FRN-SRS-102
    ///
    /// # Errors
    ///
    /// [`AskError`] when the file is too large, not UTF-8, has no usable
    /// `Socket=`, an over-long `Id=`, a malformed number, or a repeated key.
    pub fn parse(file: &[u8]) -> Result<Self, AskError> {
        if file.len() > MAX_FILE {
            return Err(AskError::TooLarge);
        }
        let text = str::from_utf8(file).map_err(|_e| AskError::NotUtf8)?;

        let mut ask = Self {
            id: Text::empty(),
            message: Text::empty(),
            socket: Text::empty(),
            not_after_us: None,
            echo: false,
            silent: false,
            pid: None,
        };
        let mut socket = None;
        let mut seen = Seen::default();
        let mut in_ask = false;
        for line in text.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') || line.starts_with(';') {
                continue;
            }
            if line.starts_with('[') {
                in_ask = line == "[Ask]";
                continue;
            }
            if !in_ask {
                continue;
            }
            let Some((key, value)) = line.split_once('=') else {
                continue;
            };
            let value = value.trim();
            let key = key.trim();
            seen.first(key)?;
            match key {
                "Id" => ask.id = Text::exact(value).ok_or(AskError::IdTooLong)?,
                "Message" => ask.message = Text::cut(value),
                "Socket" => socket = Some(value),
                "NotAfter" => {
                    let us: u64 = value.parse().map_err(|_e| AskError::BadNumber)?;
                    // systemd writes 0 for a question without a deadline.
                    ask.not_after_us = (us != 0).then_some(us);
                }
                "Echo" => ask.echo = parse_bool(value)?,
                "Silent" => ask.silent = parse_bool(value)?,
                "PID" => ask.pid = Some(value.parse().map_err(|_e| AskError::BadNumber)?),
                _ => {}
            }
        }
        let socket = socket.ok_or(AskError::NoSocket)?;
        if !socket.starts_with('/') || socket.contains('\0') {
            return Err(AskError::BadSocket);
        }
        ask.socket = Text::exact(socket).ok_or(AskError::BadSocket)?;
        Ok(ask)
    }

    /// Whether the question has expired at `now_us` on `CLOCK_MONOTONIC`.
    ///
    /// Implements: FRN-SRS-023
    #[must_use]
    pub fn expired(&self, now_us: u64) -> bool {
        self.not_after_us.is_some_and(|deadline| now_us >= deadline)
    }
}

/// The keys [`Ask::parse`] reads, each allowed once.
#[derive(Default)]
struct Seen(u8);

impl Seen {
    const KEYS: [&'static str; 7] = [
        "Id", "Message", "Socket", "NotAfter", "Echo", "Silent", "PID",
    ];

    /// Records `key`; a key read before is refused. Keys the parser does not
    /// read are not tracked.
    fn first(&mut self, key: &str) -> Result<(), AskError> {
        let Some(index) = Self::KEYS.iter().position(|k| *k == key) else {
            return Ok(());
        };
        let bit = 1_u8 << index;
        if self.0 & bit != 0 {
            return Err(AskError::RepeatedKey);
        }
        self.0 |= bit;
        Ok(())
    }
}

/// systemd's boolean spellings, as `parse_boolean()` accepts them.
fn parse_bool(value: &str) -> Result<bool, AskError> {
    match value {
        "1" | "yes" | "y" | "true" | "t" | "on" => Ok(true),
        "0" | "no" | "n" | "false" | "f" | "off" => Ok(false),
        _ => Err(AskError::BadNumber),
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

    #[test]
    fn silent_is_read() {
        let ask =
            Ask::parse(b"[Ask]\nSocket=/run/s\nSilent=yes\n").unwrap_or_else(|e| panic!("{e}"));
        assert!(ask.silent);
        let ask = Ask::parse(b"[Ask]\nSocket=/run/s\n").unwrap_or_else(|e| panic!("{e}"));
        assert!(!ask.silent);
    }

    const CRYPTSETUP: &str = "[Ask]\n\
        PID=412\n\
        Socket=/run/systemd/ask-password/sck.6cd1f81a3b6e1d2c\n\
        AcceptCached=1\n\
        Echo=0\n\
        NotAfter=31415926\n\
        Id=cryptsetup:/dev/disk/by-uuid/0b6f4c8e-1d2a-4e57-9f0a-6a1b2c3d4e5f\n\
        Message=Please enter passphrase for disk root (cryptroot):\n\
        Icon=drive-harddisk\n";

    #[test]
    fn reads_a_cryptsetup_question() {
        // Verifies: FRN-SRS-021, FRN-SRS-023
        let ask = Ask::parse(CRYPTSETUP.as_bytes()).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(ask.pid, Some(412));
        assert_eq!(
            ask.socket.as_str(),
            "/run/systemd/ask-password/sck.6cd1f81a3b6e1d2c"
        );
        assert!(!ask.echo);
        assert_eq!(ask.not_after_us, Some(31_415_926));
        assert!(ask.id.as_str().starts_with("cryptsetup:/dev/disk/by-uuid/"));
        assert_eq!(
            ask.message.as_str(),
            "Please enter passphrase for disk root (cryptroot):"
        );
    }

    #[test]
    fn a_file_over_64_kib_is_refused_before_any_field() {
        // Verifies: FRN-SRS-102
        let mut file = CRYPTSETUP.as_bytes().to_vec();
        file.resize(MAX_FILE + 1, b'#');
        assert_eq!(Ask::parse(&file), Err(AskError::TooLarge));
        file.truncate(MAX_FILE);
        assert!(Ask::parse(&file).is_ok());
    }

    #[test]
    fn a_non_utf8_byte_anywhere_is_refused() {
        // Verifies: FRN-SRS-102
        let mut file = CRYPTSETUP.as_bytes().to_vec();
        // In a comment the parser would otherwise skip: the check is over the
        // whole file, not the fields it reads.
        file.extend_from_slice(b"# \xff\n");
        assert_eq!(Ask::parse(&file), Err(AskError::NotUtf8));
    }

    #[test]
    fn echo_and_the_absent_deadline() {
        // Verifies: FRN-SRS-026
        let ask = Ask::parse(b"[Ask]\nSocket=/run/s\nEcho=1\nNotAfter=0\n")
            .unwrap_or_else(|e| panic!("{e}"));
        assert!(ask.echo);
        assert_eq!(ask.not_after_us, None);
        assert!(!ask.expired(u64::MAX));
    }

    #[test]
    fn expiry_is_at_the_deadline() {
        // Verifies: FRN-SRS-023
        let ask =
            Ask::parse(b"[Ask]\nSocket=/run/s\nNotAfter=100\n").unwrap_or_else(|e| panic!("{e}"));
        assert!(!ask.expired(99));
        assert!(ask.expired(100));
    }

    #[test]
    fn a_long_message_is_cut_at_a_character_boundary() {
        let long = "é".repeat(MESSAGE_CAPACITY);
        let file = format!("[Ask]\nSocket=/run/s\nMessage={long}\n");
        let ask = Ask::parse(file.as_bytes()).unwrap_or_else(|e| panic!("{e}"));
        assert!(ask.message.as_str().len() <= MESSAGE_CAPACITY);
        assert!(long.starts_with(ask.message.as_str()));
        assert_eq!(ask.message.as_str().chars().count(), MESSAGE_CAPACITY / 2);
    }

    #[test]
    fn the_reply_socket_must_be_whole_and_absolute() {
        assert_eq!(Ask::parse(b"[Ask]\nMessage=x\n"), Err(AskError::NoSocket));
        assert_eq!(
            Ask::parse(b"[Ask]\nSocket=relative/sck\n"),
            Err(AskError::BadSocket)
        );
        let long = format!("[Ask]\nSocket=/{}\n", "s".repeat(SOCKET_CAPACITY));
        assert_eq!(Ask::parse(long.as_bytes()), Err(AskError::BadSocket));
        let fits = format!("[Ask]\nSocket=/{}\n", "s".repeat(SOCKET_CAPACITY - 1));
        assert!(Ask::parse(fits.as_bytes()).is_ok());
    }

    #[test]
    fn an_id_too_long_to_compare_is_refused() {
        let file = format!("[Ask]\nSocket=/run/s\nId={}\n", "i".repeat(ID_CAPACITY + 1));
        assert_eq!(Ask::parse(file.as_bytes()), Err(AskError::IdTooLong));
    }

    #[test]
    fn keys_outside_ask_and_unknown_keys_are_ignored() {
        let ask = Ask::parse(
            b"Socket=/outside\n[Other]\nSocket=/other\n[Ask]\n; comment\nColour=red\nSocket=/run/s\n",
        ).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(ask.socket.as_str(), "/run/s");
    }

    /// The file systemd 260 wrote on the reference machine for
    /// `systemd-ask-password --user --id=fairing:probe --icon=drive-harddisk`,
    /// with a message holding a non-ASCII letter, quotes, a tab and a backslash.
    const OBSERVED: &str = "[Ask]\nPID=529298\nSocket=/run/user/1000/systemd/ask-password/sck.5eff5758fdc10699\nAcceptCached=0\nEcho=0\nNotAfter=41203969003\nSilent=0\nMessage=Unlock \u{e9} \"disk\"\ttab\\back\nIcon=drive-harddisk\nId=fairing:probe\n";

    #[test]
    fn reads_the_file_systemd_260_wrote() {
        let ask = Ask::parse(OBSERVED.as_bytes()).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(ask.pid, Some(529_298));
        assert_eq!(
            ask.socket.as_str(),
            "/run/user/1000/systemd/ask-password/sck.5eff5758fdc10699"
        );
        assert_eq!(ask.message.as_str(), "Unlock \u{e9} \"disk\"\ttab\\back");
        assert_eq!(ask.id.as_str(), "fairing:probe");
        assert_eq!(ask.not_after_us, Some(41_203_969_003));
        assert!(!ask.echo && !ask.silent);
    }

    #[test]
    fn a_multi_line_message_keeps_its_first_line() {
        // systemd 260 writes a newline in the message raw.
        let ask = Ask::parse(b"[Ask]\nSocket=/run/s\nEcho=1\nMessage=line one\nline two\n")
            .unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(ask.message.as_str(), "line one");
    }

    #[test]
    fn a_socket_smuggled_in_through_the_message_is_refused() {
        // A device label "x\nSocket=/tmp/evil" would otherwise redirect the reply.
        let file =
            b"[Ask]\nSocket=/run/systemd/ask-password/sck.1\nEcho=0\nMessage=x\nSocket=/tmp/evil\n";
        assert_eq!(Ask::parse(file), Err(AskError::RepeatedKey));
    }

    #[test]
    fn every_key_read_is_refused_when_repeated() {
        for (key, value) in [
            ("Id", "a"),
            ("Message", "a"),
            ("Socket", "/run/t"),
            ("NotAfter", "1"),
            ("Echo", "1"),
            ("Silent", "1"),
            ("PID", "1"),
        ] {
            let file = format!("[Ask]\nSocket=/run/s\n{key}={value}\n{key}={value}\n");
            let once = format!("[Ask]\nSocket=/run/s\n{key}={value}\n");
            assert_eq!(
                Ask::parse(file.as_bytes()),
                Err(AskError::RepeatedKey),
                "{key}"
            );
            if key != "Socket" {
                assert!(Ask::parse(once.as_bytes()).is_ok(), "{key}");
            }
        }
        // Keys the parser does not read may repeat.
        assert!(
            Ask::parse(b"[Ask]\nSocket=/run/s\nIcon=a\nIcon=b\nAcceptCached=0\nAcceptCached=1\n")
                .is_ok()
        );
    }

    #[test]
    fn the_limit_is_64_kib() {
        let mut file = b"[Ask]\nSocket=/run/s\n".to_vec();
        file.resize(65_536, b'#');
        assert!(Ask::parse(&file).is_ok());
        file.push(b'#');
        assert_eq!(Ask::parse(&file), Err(AskError::TooLarge));
    }

    #[test]
    fn every_refusal_says_why() {
        for (error, text) in [
            (AskError::TooLarge, "the ask file is larger than 64 KiB"),
            (AskError::NotUtf8, "the ask file is not UTF-8"),
            (AskError::NoSocket, "the ask file names no reply socket"),
            (
                AskError::BadSocket,
                "the ask file's reply socket is not a usable absolute path",
            ),
            (AskError::IdTooLong, "the ask file's Id= is too long"),
            (AskError::BadNumber, "the ask file has a malformed number"),
            (AskError::RepeatedKey, "the ask file repeats a key"),
        ] {
            assert_eq!(error.to_string(), text);
        }
    }

    #[test]
    fn a_cut_inside_a_character_steps_back_to_its_start() {
        // 'é' is two bytes: a cut at 4 bytes falls inside the second one.
        let text = Text::<4>::cut("aéé");
        assert_eq!(text.as_str(), "aé");
        let text = Text::<2>::cut("é");
        assert_eq!(text.as_str(), "é");
        let text = Text::<1>::cut("é");
        assert_eq!(text.as_str(), "");
        assert!(text.is_empty());
    }

    #[test]
    fn text_is_empty_only_without_bytes() {
        let empty = Text::<4>::exact("").unwrap_or_else(|| panic!("no text"));
        assert!(empty.is_empty());
        let one = Text::<4>::exact("a").unwrap_or_else(|| panic!("no text"));
        assert!(!one.is_empty());
    }

    #[test]
    fn text_debug_is_the_quoted_string() {
        let text = Text::<8>::exact("ask.1").unwrap_or_else(|| panic!("no text"));
        assert_eq!(format!("{text:?}"), "\"ask.1\"");
    }

    #[test]
    fn malformed_numbers_are_refused() {
        for file in [
            &b"[Ask]\nSocket=/run/s\nNotAfter=soon\n"[..],
            b"[Ask]\nSocket=/run/s\nPID=-1\n",
            b"[Ask]\nSocket=/run/s\nEcho=maybe\n",
        ] {
            assert_eq!(Ask::parse(file), Err(AskError::BadNumber));
        }
    }
}
