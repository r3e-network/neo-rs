use std::any::Any;
use std::sync::Arc;
use hyper::StatusCode;
use crate::neorpc::{self, Error as NeoRpcError, Header};

// abstractResult is a trait which represents either single JSON-RPC 2.0 response
// or batch JSON-RPC 2.0 response.
trait AbstractResult {
    fn run_for_errors(&self, f: &dyn Fn(&NeoRpcError));
}

// abstract represents abstract JSON-RPC 2.0 response. It is used as a server-side response
// representation.
#[derive(Serialize, Deserialize)]
struct Abstract {
    #[serde(flatten)]
    header: Header,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<Arc<NeoRpcError>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    result: Option<Box<dyn Any>>,
}

// Implement AbstractResult for Abstract
impl AbstractResult for Abstract {
    fn run_for_errors(&self, f: &dyn Fn(&NeoRpcError)) {
        if let Some(ref err) = self.error {
            f(err);
        }
    }
}

// abstractBatch represents abstract JSON-RPC 2.0 batch-response.
type AbstractBatch = Vec<Abstract>;

// Implement AbstractResult for AbstractBatch
impl AbstractResult for AbstractBatch {
    fn run_for_errors(&self, f: &dyn Fn(&NeoRpcError)) {
        for a in self {
            a.run_for_errors(f);
        }
    }
}

fn get_http_code_for_error(resp_err: &NeoRpcError) -> StatusCode {
    match resp_err.code {
        neorpc::BAD_REQUEST_CODE => StatusCode::BAD_REQUEST,
        neorpc::METHOD_NOT_FOUND_CODE => StatusCode::METHOD_NOT_ALLOWED,
        neorpc::INTERNAL_SERVER_ERROR_CODE => StatusCode::INTERNAL_SERVER_ERROR,
        _ => StatusCode::UNPROCESSABLE_ENTITY,
    }
}
