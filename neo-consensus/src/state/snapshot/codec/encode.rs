use neo_base::{encoding::NeoEncode, write_varint, NeoWrite};

use super::super::model::SnapshotState;

impl NeoEncode for SnapshotState {
    fn neo_encode<W: NeoWrite>(&self, writer: &mut W) {
        self.height.neo_encode(writer);
        self.view.neo_encode(writer);
        match self.proposal {
            Some(hash) => {
                writer.write_u8(1);
                hash.neo_encode(writer);
            }
            None => writer.write_u8(0),
        }
        write_varint(writer, self.participation.len() as u64);
        for (kind, messages) in &self.participation {
            writer.write_u8(kind.as_u8());
            write_varint(writer, messages.len() as u64);
            for message in messages {
                message.neo_encode(writer);
            }
        }
        write_varint(writer, self.expected.len() as u64);
        for (kind, validators) in &self.expected {
            writer.write_u8(kind.as_u8());
            write_varint(writer, validators.len() as u64);
            for validator in validators {
                validator.neo_encode(writer);
            }
        }
        write_varint(writer, self.change_view_reasons.len() as u64);
        for (validator, reason) in &self.change_view_reasons {
            validator.neo_encode(writer);
            writer.write_u8(*reason as u8);
        }
        write_varint(writer, self.change_view_reason_counts.len() as u64);
        for (reason, count) in &self.change_view_reason_counts {
            writer.write_u8(*reason as u8);
            write_varint(writer, *count as u64);
        }
        writer.write_u64(self.change_view_total);
    }
}
