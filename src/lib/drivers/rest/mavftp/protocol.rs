use anyhow::{Result, anyhow};
use byteorder::{LittleEndian, ReadBytesExt, WriteBytesExt};
use tracing::*;

const FTP_HEADER_LEN: usize = 12;
const FTP_MESSAGE_PAYLOAD: usize = 251;
pub(super) const FTP_DATA_MAX_LEN: usize = FTP_MESSAGE_PAYLOAD - FTP_HEADER_LEN;

/// ref: https://mavlink.io/en/services/ftp.html#payload
#[derive(Debug, Clone)]
pub(super) struct FtpPayload {
    pub seq_number: u16,
    pub session: u8,
    pub opcode: FtpOpcode,
    pub size: u8,
    pub req_opcode: FtpOpcode,
    pub burst_complete: u8,
    pub offset: u32,
    pub data: Vec<u8>,
}

impl FtpPayload {
    #[instrument(level = "debug", skip(self))]
    pub(super) fn encode(&self) -> [u8; 251] {
        let mut buf = [0u8; 251];
        let mut cursor = std::io::Cursor::new(&mut buf[..]);
        let _ = cursor.write_u16::<LittleEndian>(self.seq_number);
        let _ = cursor.write_u8(self.session);
        let _ = cursor.write_u8(self.opcode as u8);
        let _ = cursor.write_u8(self.size);
        let _ = cursor.write_u8(self.req_opcode as u8);
        let _ = cursor.write_u8(self.burst_complete);
        let _ = cursor.write_u8(0); // padding
        let _ = cursor.write_u32::<LittleEndian>(self.offset);
        let copy_len = self.data.len().min(FTP_DATA_MAX_LEN);
        buf[FTP_HEADER_LEN..FTP_HEADER_LEN + copy_len].copy_from_slice(&self.data[..copy_len]);
        buf
    }

    #[instrument(level = "debug", skip(payload))]
    pub(super) fn decode(payload: &[u8; 251]) -> Result<Self> {
        let mut cursor = std::io::Cursor::new(&payload[..]);
        let seq_number = cursor.read_u16::<LittleEndian>()?;
        let session = cursor.read_u8()?;
        let opcode = FtpOpcode::try_from(cursor.read_u8()?)?;
        let size = cursor.read_u8()?;
        let req_opcode = FtpOpcode::try_from(cursor.read_u8()?)?;
        let burst_complete = cursor.read_u8()?;
        let _padding = cursor.read_u8()?;
        let offset = cursor.read_u32::<LittleEndian>()?;
        let data_len = (size as usize).min(FTP_DATA_MAX_LEN);
        let data = payload[FTP_HEADER_LEN..FTP_HEADER_LEN + data_len].to_vec();
        Ok(Self {
            seq_number,
            session,
            opcode,
            size,
            req_opcode,
            burst_complete,
            offset,
            data,
        })
    }

    #[instrument(level = "debug", fields(opcode = ?opcode, seq_number = seq_number))]
    pub(super) fn new_request(opcode: FtpOpcode, seq_number: u16) -> Self {
        Self {
            seq_number,
            session: 0,
            opcode,
            size: 0,
            req_opcode: FtpOpcode::None,
            burst_complete: 0,
            offset: 0,
            data: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, strum_macros::FromRepr)]
#[repr(u8)]
pub(super) enum FtpOpcode {
    None = 0,
    TerminateSession = 1,
    ResetSessions = 2,
    ListDirectory = 3,
    OpenFileRO = 4,
    ReadFile = 5,
    CreateFile = 6,
    WriteFile = 7,
    RemoveFile = 8,
    CreateDirectory = 9,
    RemoveDirectory = 10,
    OpenFileWO = 11,
    TruncateFile = 12,
    Rename = 13,
    CalcFileCRC32 = 14,
    BurstReadFile = 15,
    Ack = 128,
    Nak = 129,
}

impl TryFrom<u8> for FtpOpcode {
    type Error = anyhow::Error;

    fn try_from(v: u8) -> Result<Self> {
        Self::from_repr(v).ok_or_else(|| anyhow!("Unknown FTP opcode: {v}"))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, strum_macros::FromRepr)]
#[repr(u8)]
pub enum FtpError {
    None = 0,
    Fail = 1,
    FailErrno = 2,
    InvalidDataSize = 3,
    InvalidSession = 4,
    NoSessionsAvailable = 5,
    Eof = 6,
    UnknownCommand = 7,
    FileExists = 8,
    FileProtected = 9,
    FileNotFound = 10,
}

impl TryFrom<u8> for FtpError {
    type Error = anyhow::Error;

    fn try_from(v: u8) -> Result<Self> {
        Self::from_repr(v).ok_or_else(|| anyhow!("Unknown FTP error code: {v}"))
    }
}

impl std::fmt::Display for FtpError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::None => write!(f, "No error"),
            Self::Fail => write!(f, "Unknown failure"),
            Self::FailErrno => write!(f, "System error"),
            Self::InvalidDataSize => write!(f, "Invalid data size"),
            Self::InvalidSession => write!(f, "Invalid session"),
            Self::NoSessionsAvailable => write!(f, "No sessions available"),
            Self::Eof => write!(f, "End of file"),
            Self::UnknownCommand => write!(f, "Unknown command"),
            Self::FileExists => write!(f, "File already exists"),
            Self::FileProtected => write!(f, "File is write protected"),
            Self::FileNotFound => write!(f, "File not found"),
        }
    }
}

#[derive(Debug, Clone)]
pub struct FtpNakError {
    pub err_code: FtpError,
    pub errno: Option<u8>,
}

impl std::fmt::Display for FtpNakError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "FTP NAK: {}", self.err_code)?;
        if let Some(errno) = self.errno {
            write!(f, " (errno={errno})")?;
        }
        Ok(())
    }
}

impl std::error::Error for FtpNakError {}

#[derive(Debug, Clone, Copy, PartialEq, Eq, strum_macros::FromRepr)]
#[repr(u8)]
pub(super) enum ListingEntryKind {
    Directory = b'D',
    File = b'F',
    Skip = b'S',
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct ListingEntry {
    pub kind: ListingEntryKind,
    pub name: String,
    pub size: Option<u64>,
}

#[instrument(level = "debug", skip(resp))]
pub(super) fn check_nak(resp: &FtpPayload) -> Result<(), FtpNakError> {
    if resp.opcode == FtpOpcode::Nak {
        let err_code = if resp.data.is_empty() {
            FtpError::Fail
        } else {
            FtpError::try_from(resp.data[0]).unwrap_or(FtpError::Fail)
        };
        let errno = if err_code == FtpError::FailErrno && resp.data.len() > 1 {
            Some(resp.data[1])
        } else {
            None
        };
        return Err(FtpNakError { err_code, errno });
    }
    Ok(())
}

#[instrument(level = "debug", skip(data))]
pub(super) fn parse_listing_entries(data: &[u8]) -> Vec<ListingEntry> {
    let text = String::from_utf8_lossy(data);
    text.split('\0')
        .filter(|entry| !entry.is_empty())
        .filter_map(|entry| {
            let kind = ListingEntryKind::from_repr(*entry.as_bytes().first()?)?;
            let rest = &entry[1..];
            let (name, size) = match kind {
                ListingEntryKind::Directory | ListingEntryKind::Skip => (rest.to_string(), None),
                ListingEntryKind::File => {
                    if let Some((name, size_str)) = rest.split_once('\t') {
                        let size = size_str.parse::<u64>().ok();
                        (name.to_string(), size)
                    } else {
                        (rest.to_string(), None)
                    }
                }
            };
            Some(ListingEntry { kind, name, size })
        })
        .collect()
}
