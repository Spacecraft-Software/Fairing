// SPDX-FileCopyrightText: 2026 Mohamed Hammad <Mohamed.Hammad@SpacecraftSoftware.org>
// SPDX-License-Identifier: GPL-3.0-or-later
// Rust guideline compliant 2026-05-18

//! The typed secret, in one buffer of fixed capacity.
//!
//! The bytes live inline in [`Secret`], never on the heap, so no allocator
//! ever holds a copy. Nothing about a secret is printable: `Debug` shows only
//! its length, there is no `Display`, `Clone` or `Copy`, and the only way out
//! is [`Secret::with_bytes`], which the reply uses to fill its datagram.
//!
//! Release builds abort on panic, which skips `Drop`. So the agent clears a
//! secret eagerly on every path that ends a question (reply, cancel, expiry,
//! error) with [`Secret::clear`], and `Drop` zeroises as a second line, not
//! the first (FRN-SRS-024).

use core::fmt;

use zeroize::Zeroize;

/// Bytes a secret may hold.
///
/// systemd's own console agent caps a password at `LINE_MAX`; 512 bytes holds
/// any passphrase typed at a boot prompt with room for multi-byte characters,
/// and keeps the buffer, and the datagram built from it, small and fixed.
pub const CAPACITY: usize = 512;

/// A secret being typed, at most [`CAPACITY`] bytes of UTF-8.
///
/// Implements: FRN-SRS-024, FRN-SRS-104
pub struct Secret {
    bytes: [u8; CAPACITY],
    len: usize,
    chars: usize,
}

impl Secret {
    /// An empty secret.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            bytes: [0; CAPACITY],
            len: 0,
            chars: 0,
        }
    }

    /// Appends `c`; `false` when it would not fit, and nothing changes.
    pub fn push(&mut self, c: char) -> bool {
        let width = c.len_utf8();
        let Some(dst) = self.bytes.get_mut(self.len..self.len + width) else {
            return false;
        };
        c.encode_utf8(dst);
        self.len += width;
        self.chars += 1;
        true
    }

    /// Removes the last character, zeroising its bytes.
    pub fn pop(&mut self) {
        let Some(last) = self.as_str().chars().next_back() else {
            return;
        };
        let start = self.len - last.len_utf8();
        self.bytes[start..self.len].zeroize();
        self.len = start;
        self.chars -= 1;
    }

    /// Zeroises every byte and empties the secret.
    pub fn clear(&mut self) {
        self.bytes.zeroize();
        self.len = 0;
        self.chars = 0;
    }

    /// Characters typed, which is what a masked field shows (FRN-SRS-021).
    #[must_use]
    pub const fn chars(&self) -> usize {
        self.chars
    }

    /// Whether nothing has been typed.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Lends the secret's bytes to `f`, the only way to read them.
    pub fn with_bytes<R>(&self, f: impl FnOnce(&[u8]) -> R) -> R {
        f(&self.bytes[..self.len])
    }

    /// The secret as text, for an `Echo=1` field (FRN-SRS-026).
    #[must_use]
    pub fn as_str(&self) -> &str {
        // Only whole characters are ever pushed, so this never fails.
        core::str::from_utf8(&self.bytes[..self.len]).unwrap_or_default()
    }
}

impl Default for Secret {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for Secret {
    fn drop(&mut self) {
        self.clear();
    }
}

impl fmt::Debug for Secret {
    /// Shows the length only: a secret never reaches a log (FRN-SRS-025).
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Secret")
            .field("chars", &self.chars)
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn typing_and_backspace() {
        let mut secret = Secret::new();
        for c in "pässwörd".chars() {
            assert!(secret.push(c));
        }
        assert_eq!(secret.chars(), 8);
        secret.pop();
        assert_eq!(secret.as_str(), "pässwör");
        secret.pop();
        // The popped bytes are zero, not left behind the end.
        assert!(secret.bytes[secret.len..].iter().all(|&b| b == 0));
    }

    #[test]
    fn clear_zeroises_every_byte() {
        // Verifies: FRN-SRS-024
        let mut secret = Secret::new();
        for c in "correct horse battery staple".chars() {
            secret.push(c);
        }
        secret.clear();
        assert!(secret.is_empty());
        assert_eq!(secret.chars(), 0);
        assert!(secret.bytes.iter().all(|&b| b == 0));
    }

    #[test]
    fn a_full_secret_refuses_more_and_keeps_whole_characters() {
        let mut secret = Secret::new();
        while secret.push('é') {}
        assert_eq!(secret.as_str().len(), CAPACITY);
        assert!(!secret.push('x'));
        assert_eq!(secret.chars(), CAPACITY / 2);
    }

    #[test]
    fn debug_shows_no_secret() {
        // Verifies: FRN-SRS-025
        let mut secret = Secret::new();
        for c in "hunter2".chars() {
            secret.push(c);
        }
        let shown = format!("{secret:?}");
        assert!(!shown.contains("hunter2"), "{shown}");
        assert!(shown.contains("chars: 7"), "{shown}");
    }
}
