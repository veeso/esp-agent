//! Line editing for a serial console.
//!
//! [`LineEditor`] turns the raw bytes received from a serial port into editing events and
//! holds the line being typed. It performs no I/O: the caller feeds it bytes and acts on
//! the returned [`Event`].

use alloc::string::String;
use alloc::vec::Vec;

/// Maximum number of bytes accepted on a single line. Further input is ignored.
pub const MAX_LINE_BYTES: usize = 256;

const BACKSPACE: u8 = 0x08;
const DELETE: u8 = 0x7f;
const LINE_FEED: u8 = b'\n';
const CARRIAGE_RETURN: u8 = b'\r';
const KILL_LINE: u8 = 0x15;

/// What the caller should do after feeding a byte to a [`LineEditor`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Event {
    /// Nothing to show.
    Ignored,
    /// The byte was added to the line and should be echoed back to the terminal.
    Echo(u8),
    /// One character was removed from the line; the terminal should erase it.
    Erase,
    /// The whole line was discarded; the terminal should erase it.
    Kill,
    /// The line is complete and can be read with [`LineEditor::take_line`].
    Submit,
}

/// A submitted line and whether the editor retained every byte.
#[derive(Debug)]
pub enum SubmittedLine {
    /// The entire line fit in the input buffer.
    Complete(String),
    /// The line exceeded the buffer. Only the retained text is available.
    Truncated(String),
}

impl SubmittedLine {
    /// Returns the text retained by the editor.
    pub fn text(&self) -> &str {
        match self {
            Self::Complete(text) | Self::Truncated(text) => text,
        }
    }
}

/// Accumulates bytes into a line, handling backspace and both `\r` and `\n` terminators.
#[derive(Debug, Default)]
pub struct LineEditor {
    buf: Vec<u8>,
    truncated: bool,
    after_carriage_return: bool,
}

impl LineEditor {
    /// Creates an editor with an empty line.
    pub fn new() -> Self {
        Self::default()
    }

    /// Feeds one received byte and reports how the terminal should react.
    ///
    /// A `\n` directly after a `\r` is ignored, so terminals that send `\r\n` produce a
    /// single [`Event::Submit`].
    pub fn push(&mut self, byte: u8) -> Event {
        let after_carriage_return = core::mem::replace(&mut self.after_carriage_return, false);
        match byte {
            CARRIAGE_RETURN => {
                self.after_carriage_return = true;
                Event::Submit
            }
            LINE_FEED if after_carriage_return => Event::Ignored,
            LINE_FEED => Event::Submit,
            BACKSPACE | DELETE => self.erase_last_char(),
            KILL_LINE => {
                self.truncated = false;
                if self.buf.is_empty() {
                    Event::Ignored
                } else {
                    self.buf.clear();
                    Event::Kill
                }
            }
            byte if byte.is_ascii_control() => Event::Ignored,
            _ if self.buf.len() >= MAX_LINE_BYTES => {
                self.truncated = true;
                Event::Ignored
            }
            byte => {
                self.buf.push(byte);
                Event::Echo(byte)
            }
        }
    }

    /// Returns the line typed so far and starts a new one.
    ///
    /// Bytes that are not valid UTF-8 are replaced with `U+FFFD`.
    pub fn take_line(&mut self) -> SubmittedLine {
        let line = String::from_utf8_lossy(&self.buf).into_owned();
        self.buf.clear();
        if core::mem::take(&mut self.truncated) {
            SubmittedLine::Truncated(line)
        } else {
            SubmittedLine::Complete(line)
        }
    }

    fn erase_last_char(&mut self) -> Event {
        let Some(&last) = self.buf.last() else {
            return Event::Ignored;
        };
        // Drop a whole UTF-8 sequence: continuation bytes first, then the lead byte.
        let mut last = last;
        while is_continuation(last) && self.buf.len() > 1 {
            self.buf.pop();
            last = self.buf[self.buf.len() - 1];
        }
        self.buf.pop();
        Event::Erase
    }
}

fn is_continuation(byte: u8) -> bool {
    byte & 0b1100_0000 == 0b1000_0000
}
