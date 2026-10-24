//! The exit kit's data: the locked backup plus a plain copy of the wallet
//! setup, as armoured text the restore tool can find anywhere.
//!
//! The kit itself is a PDF the app makes (decision D2 in
//! docs/restore-tool.md). The same armoured text is embedded in it as an
//! uncompressed attachment, so its bytes appear as they are in the PDF file,
//! and printed on its last page, so it survives printing. [`read`] takes the
//! PDF, the armoured text pasted or typed back in, or the raw backup.
//!
//! ```text
//! -----BEGIN SATKEEP EXIT KIT-----
//! 0001 534b4b54 01000001 …(8 groups)… <check>
//! 0002 …
//! -----END SATKEEP EXIT KIT-----
//! ```
//!
//! Each line is its number, up to 32 bytes of hex in groups of 8 digits, and
//! a check of 8 hex digits (the first 4 bytes of a SHA-256 of the line's
//! number and bytes). The check finds typos and missing lines when the text
//! is typed back in; it's not a signature. What protects the kit is the
//! backup's own authentication, and [`restore_kit`] refusing a plain setup
//! that differs from the locked one (docs/restore-tool.md, threat 7.2).

use sha2::{Digest, Sha256};

use crate::setup::{MAX_SETUP, WalletSetup};
use crate::{BackupError, Restored, SecurityDevice, format, hex, unhex};

pub const BEGIN: &str = "-----BEGIN SATKEEP EXIT KIT-----";
pub const END: &str = "-----END SATKEEP EXIT KIT-----";
/// The kit data's own marker and version, inside the armour.
pub const MAGIC: &[u8; 4] = b"SKKT";
pub const VERSION: u8 = 1;
pub const BYTES_PER_LINE: usize = 32;
/// The longest line a reader looks at. A written line is 89 bytes; this
/// leaves room for extra spaces when it's typed back in, and stops a hostile
/// file from making the reader split one huge line.
pub const MAX_LINE: usize = 256;
/// The largest input [`read`] looks through: far above any kit PDF.
pub const MAX_INPUT: usize = 64 * 1024 * 1024;
const MAX_DATA: usize = 4 + 1 + 4 + format::MAX_FILE + 4 + MAX_SETUP;
const LINE_CHECK_LABEL: &[u8] = b"satkeep/exit-kit/v1/line";

/// What a kit holds.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Kit {
    /// The locked backup, as `make_backup` wrote it.
    pub backup: Vec<u8>,
    /// The plain copy of the wallet setup, readable without the second key.
    /// `None` when the input was the raw backup alone.
    pub plain_setup: Option<WalletSetup>,
}

/// The armoured text for `backup` and `setup`, ending with a newline.
pub fn encode(backup: &[u8], setup: &WalletSetup) -> Result<String, BackupError> {
    format::parse(backup)?;
    let setup_text = setup.to_text()?;
    let mut data = MAGIC.to_vec();
    data.push(VERSION);
    data.extend_from_slice(&u32::try_from(backup.len()).expect("at most MAX_FILE").to_be_bytes());
    data.extend_from_slice(backup);
    data.extend_from_slice(&u32::try_from(setup_text.len()).expect("at most MAX_SETUP").to_be_bytes());
    data.extend_from_slice(setup_text.as_bytes());

    let mut out = format!("{BEGIN}\n");
    for (i, chunk) in data.chunks(BYTES_PER_LINE).enumerate() {
        let number = u32::try_from(i + 1).expect("a few thousand lines at most");
        let groups: Vec<String> = chunk.chunks(4).map(hex).collect();
        out += &format!("{number:04} {} {}\n", groups.join(" "), hex(&line_check(number, chunk)));
    }
    out += END;
    out.push('\n');
    Ok(out)
}

/// Finds the kit in `input`: a PDF or any text holding the armoured block,
/// or the raw backup file.
///
/// Every BEGIN…END block in the input is decoded. Blocks that don't decode
/// are skipped if another one does (a PDF may hold page text between the
/// same markers); otherwise the first block's error is returned. Two blocks
/// that decode to different kits are refused.
pub fn read(input: &[u8]) -> Result<Kit, BackupError> {
    if input.len() > MAX_INPUT {
        return Err(BackupError::NotAKit);
    }
    if input.starts_with(format::MAGIC) {
        format::parse(input)?;
        return Ok(Kit { backup: input.to_vec(), plain_setup: None });
    }
    let mut found: Option<Kit> = None;
    let mut first_error = None;
    let mut rest = input;
    while let Some(start) = find(rest, BEGIN.as_bytes()) {
        let after = &rest[start + BEGIN.len()..];
        let Some(len) = find(after, END.as_bytes()) else {
            first_error.get_or_insert(BackupError::KitIncomplete);
            break;
        };
        match decode_block(&after[..len]) {
            Ok(kit) => match &found {
                Some(earlier) if *earlier != kit => return Err(BackupError::TwoDifferentKits),
                Some(_) => {}
                None => found = Some(kit),
            },
            Err(e) => {
                first_error.get_or_insert(e);
            }
        }
        rest = &after[len + END.len()..];
    }
    found.ok_or(first_error.unwrap_or(BackupError::NotAKit))
}

/// Opens the kit's backup with `device`, and refuses if the kit's plain
/// wallet setup differs from the one locked inside the backup.
pub fn restore_kit(kit: &Kit, device: &mut dyn SecurityDevice) -> Result<Restored, BackupError> {
    let restored = crate::restore(&kit.backup, device)?;
    match &kit.plain_setup {
        Some(plain) if *plain != restored.wallet_setup => Err(BackupError::KitSetupMismatch),
        _ => Ok(restored),
    }
}

fn decode_block(block: &[u8]) -> Result<Kit, BackupError> {
    let mut data = Vec::new();
    let mut expected: u32 = 1;
    let mut last_was_short = false;
    for raw in block.split(|b| *b == b'\n') {
        let damaged = BackupError::KitLine { line: expected };
        if raw.len() > MAX_LINE {
            return Err(damaged);
        }
        let line = std::str::from_utf8(raw).map_err(|_| damaged)?;
        let tokens: Vec<&str> = line.split_whitespace().collect();
        if tokens.is_empty() {
            continue;
        }
        let [number, groups @ .., check] = tokens.as_slice() else { return Err(damaged) };
        if groups.is_empty() || !number.bytes().all(|b| b.is_ascii_digit()) || number.parse::<u32>() != Ok(expected) {
            return Err(damaged);
        }
        let chunk = unhex(&groups.concat()).filter(|c| c.len() <= BYTES_PER_LINE).ok_or(damaged)?;
        if unhex(check).as_deref() != Some(&line_check(expected, &chunk)[..]) {
            return Err(damaged);
        }
        // Only the last line may be short.
        if last_was_short {
            return Err(BackupError::KitLine { line: expected - 1 });
        }
        last_was_short = chunk.len() < BYTES_PER_LINE;
        data.extend_from_slice(&chunk);
        if data.len() > MAX_DATA {
            return Err(BackupError::NotAKit);
        }
        expected += 1;
    }
    parse_data(&data)
}

fn parse_data(data: &[u8]) -> Result<Kit, BackupError> {
    if data.is_empty() {
        return Err(BackupError::KitIncomplete);
    }
    if data.len() < 5 {
        return Err(BackupError::KitIncomplete);
    }
    if &data[..4] != MAGIC {
        return Err(BackupError::NotAKit);
    }
    if data[4] != VERSION {
        return Err(BackupError::UnsupportedVersion);
    }
    let (backup, rest) = field(&data[5..], format::MAX_FILE)?;
    let (setup_bytes, rest) = field(rest, MAX_SETUP)?;
    if !rest.is_empty() {
        return Err(BackupError::NotAKit);
    }
    let backup = backup.to_vec();
    format::parse(&backup)?;
    let setup_text = std::str::from_utf8(setup_bytes).map_err(|_| BackupError::BadSetup)?;
    Ok(Kit { backup, plain_setup: Some(WalletSetup::from_text(setup_text)?) })
}

/// A u32 length and that many bytes, at most `max`; and what follows.
fn field(data: &[u8], max: usize) -> Result<(&[u8], &[u8]), BackupError> {
    let len_bytes: [u8; 4] = data.get(..4).ok_or(BackupError::KitIncomplete)?.try_into().expect("4 bytes");
    let len = usize::try_from(u32::from_be_bytes(len_bytes)).map_err(|_| BackupError::NotAKit)?;
    if len > max {
        return Err(BackupError::NotAKit);
    }
    let out = data.get(4..4 + len).ok_or(BackupError::KitIncomplete)?;
    Ok((out, &data[4 + len..]))
}

fn line_check(number: u32, chunk: &[u8]) -> [u8; 4] {
    let digest = Sha256::new().chain_update(LINE_CHECK_LABEL).chain_update(number.to_be_bytes()).chain_update(chunk).finalize();
    digest[..4].try_into().expect("a SHA-256 digest has 4 bytes")
}

fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|w| w == needle)
}
