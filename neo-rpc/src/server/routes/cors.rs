use crate::server::rpc_server_settings::RpcServerConfig;
use base64::{engine::general_purpose::STANDARD as BASE64_STANDARD, Engine as _};
use subtle::ConstantTimeEq;
use warp::http::header::{
    HeaderValue, ACCESS_CONTROL_ALLOW_CREDENTIALS, ACCESS_CONTROL_ALLOW_HEADERS,
    ACCESS_CONTROL_ALLOW_METHODS, ACCESS_CONTROL_ALLOW_ORIGIN, VARY};
use warp::reply::Response as HttpResponse;

#[derive(Clone)]
pub struct BasicAuth {
    pub(super) user: Vec<u8>,
    pub(super) pass: Vec<u8>}

impl BasicAuth {
    pub fn from_settings(settings: &RpcServerConfig) -> Option<Self> {
        if settings.rpc_user.trim().is_empty() {
            return None;
       }

        Some(Self {
            user: settings.rpc_user.as_bytes().to_vec(),
            pass: settings.rpc_pass.as_bytes().to_vec()})
   }
}

#[derive(Clone)]
pub(super) struct CorsConfig {
    allow_any: bool,
    origins: Vec<HeaderValue>}

impl CorsConfig {
    pub(super) fn from_settings(settings: &RpcServerConfig, has_auth: bool) -> Option<Self> {
        if !settings.enable_cors {
            return None;
       }

        let allow_any = settings.allow_origins.is_empty();
        let mut invalid_origins = 0usize;
        let origins = settings
            .allow_origins
            .iter()
            .filter_map(|origin| {
                if let Ok(value) = HeaderValue::from_str(origin) {
                    Some(value)
               } else {
                    invalid_origins += 1;
                    None
               }
           })
            .collect::<Vec<_>>();

        if invalid_origins > 0 {
            tracing::warn!(
                invalid_origins,
                "Ignoring invalid CORS origin entries in allow_origins"
            );
       }
        if !allow_any && origins.is_empty() {
            tracing::warn!(
                "CORS is enabled but allow_origins contains no valid entries; CORS will be \
                 effectively disabled"
            );
       }

        if allow_any && has_auth {
            tracing::warn!(
                "SECURITY WARNING: CORS is configured to allow all origins ('*') while \
                authentication is enabled. This combination is insecure and may expose \
                your RPC server to CSRF attacks. Consider specifying explicit allowed \
                origins in the 'allow_origins' configuration."
            );
       }

        Some(Self {allow_any, origins})
   }

    pub(super) fn origin_header(
        &self,
        request_origin: Option<&HeaderValue>,
    ) -> Option<HeaderValue> {
        if self.allow_any {
            Some(HeaderValue::from_static("*"))
       } else if let Some(origin) = request_origin {
            self.origins
                .iter()
                .find(|allowed| *allowed == origin)
                .cloned()
       } else {
            None
       }
   }
}

pub(super) fn apply_cors(
    response: &mut HttpResponse,
    cors: Option<&CorsConfig>,
    request_origin: Option<&HeaderValue>,
) {
    if let Some(cors) = cors {
        if let Some(origin) = cors.origin_header(request_origin) {
            response
                .headers_mut()
                .insert(ACCESS_CONTROL_ALLOW_ORIGIN, origin);
       }
        if !cors.allow_any {
            response
                .headers_mut()
                .insert(VARY, HeaderValue::from_static("origin"));
            response.headers_mut().insert(
                ACCESS_CONTROL_ALLOW_CREDENTIALS,
                HeaderValue::from_static("true"),
            );
       }
        response.headers_mut().insert(
            ACCESS_CONTROL_ALLOW_METHODS,
            HeaderValue::from_static("POST, GET, OPTIONS"),
        );
        response.headers_mut().insert(
            ACCESS_CONTROL_ALLOW_HEADERS,
            HeaderValue::from_static("content-type"),
        );
   }
}

pub fn verify_basic_auth(header: Option<&str>, auth: &BasicAuth) -> bool {
    let header = match header {
        Some(value) => value.trim(),
        None => return false};

    let mut parts = header.splitn(2, ' ');
    let scheme = parts.next().unwrap_or("");
    if !scheme.eq_ignore_ascii_case("basic") {
        return false;
   }

    let value = parts.next().unwrap_or("").trim();

    let decoded = match BASE64_STANDARD.decode(value) {
        Ok(bytes) => bytes,
        Err(_) => return false};

    let Some(index) = decoded.iter().position(|byte| *byte == b':') else {
        return false;
   };

    let (user, pass) = decoded.split_at(index);
    let pass = &pass[1..];

    constant_time_equals(user, &auth.user) && constant_time_equals(pass, &auth.pass)
}

fn constant_time_equals(left: &[u8], right: &[u8]) -> bool {
    left.len() == right.len() && left.ct_eq(right).into()
}
