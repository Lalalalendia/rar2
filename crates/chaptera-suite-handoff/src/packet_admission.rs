//! Finite admission of the small V1 control packet before source-file access.
use crate::HandoffPacket;
use std::fmt;
use std::fs::File;
use std::io::Read;
use std::path::Path;

/// Code-owned V1 limits. Changing the schema requires an explicit new version.
pub struct HandoffPacketAdmissionV1;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HandoffPacketAdmissionError {
    Open,
    Metadata,
    NotRegularFile,
    Read,
    PacketSizeLimit,
    InvalidJson,
    FieldLengthLimit(&'static str),
}

impl HandoffPacketAdmissionError {
    pub fn code(self) -> &'static str {
        match self {
            Self::Open => "handoff_packet_open_failed",
            Self::Metadata => "handoff_packet_metadata_failed",
            Self::NotRegularFile => "handoff_packet_not_regular_file",
            Self::Read => "handoff_packet_read_failed",
            Self::PacketSizeLimit => "handoff_packet_size_limit",
            Self::InvalidJson => "handoff_packet_json_invalid",
            Self::FieldLengthLimit(_) => "handoff_packet_field_length_limit",
        }
    }
}

impl fmt::Display for HandoffPacketAdmissionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.code())?;
        if let Self::FieldLengthLimit(field) = self {
            write!(formatter, ": {field}")?;
        }
        Ok(())
    }
}

impl std::error::Error for HandoffPacketAdmissionError {}

impl HandoffPacketAdmissionV1 {
    // Representative V1 packets are <1 KiB. 32767 fully escaped Windows
    // UTF-16 path units plus 2 KiB of fixed fields need at most 198650 bytes.
    pub const MAX_PACKET_BYTES: usize = 256 * 1024;
    pub const MAX_SOURCE_PATH_BYTES: usize = 128 * 1024;
    pub const MAX_FIELD_BYTES: usize = 128;

    pub(crate) fn check_byte_length(length: usize) -> Result<(), HandoffPacketAdmissionError> {
        if length > Self::MAX_PACKET_BYTES {
            return Err(HandoffPacketAdmissionError::PacketSizeLimit);
        }
        Ok(())
    }

    pub(crate) fn check_lengths(packet: &HandoffPacket) -> Result<(), HandoffPacketAdmissionError> {
        let fields = [
            (
                "protocol_version",
                packet.protocol_version.as_str(),
                Self::MAX_FIELD_BYTES,
            ),
            (
                "sender_product_id",
                packet.sender_product_id.as_str(),
                Self::MAX_FIELD_BYTES,
            ),
            (
                "target_product_id",
                packet.target_product_id.as_str(),
                Self::MAX_FIELD_BYTES,
            ),
            (
                "requested_job",
                packet.requested_job.as_str(),
                Self::MAX_FIELD_BYTES,
            ),
            (
                "source.path",
                packet.source.path.as_str(),
                Self::MAX_SOURCE_PATH_BYTES,
            ),
            ("source.sha256", packet.source.sha256.as_str(), 64),
            (
                "source.kind",
                packet.source.kind.as_str(),
                Self::MAX_FIELD_BYTES,
            ),
            (
                "context.capability",
                packet.context.capability.as_str(),
                Self::MAX_FIELD_BYTES,
            ),
            (
                "context.loss_state",
                packet.context.loss_state.as_str(),
                Self::MAX_FIELD_BYTES,
            ),
        ];
        for (field, value, maximum) in fields {
            if value.len() > maximum {
                return Err(HandoffPacketAdmissionError::FieldLengthLimit(field));
            }
        }
        Ok(())
    }

    /// Decode bounded exact-schema bytes. Existing route/consent validation
    /// remains required after this structural admission.
    pub fn decode(bytes: &[u8]) -> Result<HandoffPacket, HandoffPacketAdmissionError> {
        Self::check_byte_length(bytes.len())?;
        // Keep serde_json's recursion guard enabled. The exact typed V1 shape
        // rejects unknown/duplicate fields and nested values in scalar fields.
        let packet =
            serde_json::from_slice(bytes).map_err(|_| HandoffPacketAdmissionError::InvalidJson)?;
        Self::check_lengths(&packet)?;
        Ok(packet)
    }

    fn read_bounded(
        reader: impl Read,
        declared_length: u64,
    ) -> Result<Vec<u8>, HandoffPacketAdmissionError> {
        if declared_length > Self::MAX_PACKET_BYTES as u64 {
            return Err(HandoffPacketAdmissionError::PacketSizeLimit);
        }
        let mut bytes = Vec::new();
        reader
            .take(Self::MAX_PACKET_BYTES as u64 + 1)
            .read_to_end(&mut bytes)
            .map_err(|_| HandoffPacketAdmissionError::Read)?;
        // Metadata is only an early rejection optimization. A growing file
        // or stale/lying length still cannot read beyond the sentinel byte.
        Self::check_byte_length(bytes.len())?;
        Ok(bytes)
    }

    pub(crate) fn read_path(path: &Path) -> Result<Vec<u8>, HandoffPacketAdmissionError> {
        let file = File::open(path).map_err(|_| HandoffPacketAdmissionError::Open)?;
        let metadata = file
            .metadata()
            .map_err(|_| HandoffPacketAdmissionError::Metadata)?;
        if !metadata.is_file() {
            return Err(HandoffPacketAdmissionError::NotRegularFile);
        }
        Self::read_bounded(file, metadata.len())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{self, Cursor};

    #[test]
    fn exact_byte_limit_and_one_sentinel_byte_are_distinguished() {
        let bytes = vec![b' '; HandoffPacketAdmissionV1::MAX_PACKET_BYTES];
        assert_eq!(
            HandoffPacketAdmissionV1::read_bounded(Cursor::new(&bytes), bytes.len() as u64)
                .unwrap(),
            bytes
        );
        let larger = vec![b' '; HandoffPacketAdmissionV1::MAX_PACKET_BYTES + 1];
        assert_eq!(
            HandoffPacketAdmissionV1::read_bounded(Cursor::new(&larger), larger.len() as u64)
                .unwrap_err(),
            HandoffPacketAdmissionError::PacketSizeLimit
        );
    }

    #[test]
    fn stale_metadata_cannot_hide_growth_and_read_is_capped() {
        let bytes = vec![b' '; 2 * HandoffPacketAdmissionV1::MAX_PACKET_BYTES];
        let mut cursor = Cursor::new(bytes);
        assert_eq!(
            HandoffPacketAdmissionV1::read_bounded(&mut cursor, 0).unwrap_err(),
            HandoffPacketAdmissionError::PacketSizeLimit
        );
        assert_eq!(
            cursor.position(),
            HandoffPacketAdmissionV1::MAX_PACKET_BYTES as u64 + 1
        );
    }

    #[test]
    fn oversized_metadata_does_not_read_and_truncation_uses_actual_bytes() {
        let mut cursor = Cursor::new(b"actual shorter bytes");
        assert_eq!(
            HandoffPacketAdmissionV1::read_bounded(
                &mut cursor,
                HandoffPacketAdmissionV1::MAX_PACKET_BYTES as u64 + 1
            )
            .unwrap_err(),
            HandoffPacketAdmissionError::PacketSizeLimit
        );
        assert_eq!(cursor.position(), 0);
        assert_eq!(
            HandoffPacketAdmissionV1::read_bounded(&mut cursor, 100).unwrap(),
            b"actual shorter bytes"
        );
    }

    #[test]
    fn read_failure_has_a_source_free_stable_code() {
        struct FailingReader;
        impl Read for FailingReader {
            fn read(&mut self, _: &mut [u8]) -> io::Result<usize> {
                Err(io::Error::other("synthetic private path must not appear"))
            }
        }
        let error = HandoffPacketAdmissionV1::read_bounded(FailingReader, 0).unwrap_err();
        assert_eq!(error.code(), "handoff_packet_read_failed");
        assert!(!error.to_string().contains("private"));
    }
}
