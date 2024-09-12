pub trait IJsonConvertible {
    fn to_json(&self) -> Json;
    fn from_json(json: &Json) -> Result<Self, JsonError>
    where
        Self: Sized;
}