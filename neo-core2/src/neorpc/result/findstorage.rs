// FindStorage represents the result of `findstorage` RPC handler.
#[derive(Serialize, Deserialize)]
pub struct FindStorage {
    #[serde(rename = "results")]
    results: Vec<KeyValue>,
    // Next contains the index of the next subsequent element of the contract storage
    // that can be retrieved during the next iteration.
    #[serde(rename = "next")]
    next: i32,
    #[serde(rename = "truncated")]
    truncated: bool,
}
