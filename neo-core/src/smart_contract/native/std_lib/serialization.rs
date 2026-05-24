use super::StdLib;
use crate::error::CoreError as Error;
use crate::error::CoreResult as Result;
use crate::smart_contract::application_engine::ApplicationEngine;
use crate::smart_contract::{BinarySerializer, JsonSerializer};

impl StdLib {
    /// Serializes a stack item using the binary serializer.
    pub(super) fn serialize(
        &self,
        engine: &ApplicationEngine,
        args: &[Vec<u8>],
    ) -> Result<Vec<u8>> {
        if args.is_empty() {
            return Err(Error::native_contract(
                "serialize requires data argument".to_string(),
            ));
        }

        let item = self.decode_stack_item(engine, &args[0])?;
        BinarySerializer::serialize(&item, engine.execution_limits())
            .map_err(|e| Error::native_contract(format!("Serialize failed: {e}")))
    }

    /// Deserializes a binary-serialized stack item.
    pub(super) fn deserialize(
        &self,
        engine: &ApplicationEngine,
        args: &[Vec<u8>],
    ) -> Result<Vec<u8>> {
        if args.is_empty() {
            return Err(Error::native_contract(
                "deserialize requires data argument".to_string(),
            ));
        }

        let item = BinarySerializer::deserialize(&args[0], engine.execution_limits(), None)
            .map_err(|e| Error::native_contract(format!("Deserialize failed: {e}")))?;
        BinarySerializer::serialize(&item, engine.execution_limits())
            .map_err(|e| Error::native_contract(format!("Deserialize failed: {e}")))
    }

    /// Serializes a stack item to JSON.
    pub(super) fn json_serialize(
        &self,
        engine: &ApplicationEngine,
        args: &[Vec<u8>],
    ) -> Result<Vec<u8>> {
        if args.is_empty() {
            return Err(Error::native_contract(
                "jsonSerialize requires data argument".to_string(),
            ));
        }

        let item = self.decode_stack_item(engine, &args[0])?;
        JsonSerializer::serialize_to_byte_array(&item, engine.execution_limits().max_item_size)
            .map_err(|e| Error::native_contract(format!("JSON serialization error: {e}")))
    }

    /// Deserializes JSON into a stack item.
    pub(super) fn json_deserialize(
        &self,
        engine: &ApplicationEngine,
        args: &[Vec<u8>],
    ) -> Result<Vec<u8>> {
        if args.is_empty() {
            return Err(Error::native_contract(
                "jsonDeserialize requires JSON string argument".to_string(),
            ));
        }

        self.ensure_max_input_len(&args[0], "jsonDeserialize")?;
        let item = JsonSerializer::deserialize(&args[0], 10)
            .map_err(|e| Error::native_contract(format!("JSON deserialization error: {e}")))?;
        BinarySerializer::serialize(&item, engine.execution_limits())
            .map_err(|e| Error::native_contract(format!("JSON deserialization error: {e}")))
    }
}
