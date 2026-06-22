#[path = "neofs/auth.rs"]
mod auth;
#[path = "neofs/http.rs"]
mod http;
#[cfg(feature = "neofs-grpc")]
#[path = "neofs/json.rs"]
mod json;
#[path = "neofs/parse.rs"]
mod parse;
