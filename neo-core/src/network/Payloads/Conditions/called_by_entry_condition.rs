use neo_json::jtoken::JToken;
use crate::io::binary_writer::BinaryWriter;
use crate::io::memory_reader::MemoryReader;
use crate::neo_contract::application_engine::ApplicationEngine;
use crate::neo_contract::execution_context_state::ExecutionContextState;
use crate::network::Payloads::Conditions::{WitnessCondition, WitnessConditionType};

#[derive(Debug)]
pub struct CalledByEntryCondition;

impl WitnessCondition for CalledByEntryCondition {
    fn condition_type(&self) -> WitnessConditionType {
        WitnessConditionType::CalledByEntry
    }

    fn deserialize_without_type(&mut self, _reader: &mut MemoryReader, _max_nest_depth: i32) {}

    fn match_condition(&self, engine: &ApplicationEngine) -> bool {
        let state = engine.current_context().get_state::<ExecutionContextState>();
        if state.calling_context.is_none() {
            return true;
        }
        let state = state.calling_context.unwrap().get_state::<ExecutionContextState>();
        state.calling_context.is_none()
    }

    fn serialize_without_type(&self, _writer: &mut BinaryWriter) {}

    fn parse_json(&mut self, _json: &JToken, _max_nest_depth: i32) {}
}
