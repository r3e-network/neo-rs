//! Checks applied at the HTTP boundary, ahead of the routes.
//!
//! The dispatcher in `handlers` authenticates each JSON-RPC call, but only
//! after the body has been aggregated and parsed. The gate runs first, on the
//! request head alone, so a client that cannot authenticate never makes the
//! server read or parse anything:
//!
//! 1. per-peer-IP admission limit,
//! 2. HTTP Basic authentication, when credentials are configured,
//! 3. the request body, received under a total and an idle deadline,
//! 4. the routes (parse, method-tier limit, handler).
//!
//! The dispatcher keeps its own authentication check as defence in depth.

use super::cors::{BasicAuth, CorsConfig, apply_cors, verify_basic_auth};
use super::{build_http_response, error_response, max_post_body_len};
use crate::client_identity;
use crate::middleware::{GovernorRateLimiter, RateLimitConfig};
use crate::rpc_error::RpcError;
use crate::rpc_server::{RPC_ERR_TOTAL, RPC_REQ_TOTAL};
use crate::rpc_server_settings::RpcServerConfig;
use bytes::{Bytes, BytesMut};
use hyper::body::HttpBody;
use hyper::header::{AUTHORIZATION, CONTENT_LENGTH, HeaderValue, ORIGIN};
use hyper::service::Service;
use hyper::{Body, Method, Request, Response, StatusCode};
use std::convert::Infallible;
use std::fmt;
use std::future::Future;
use std::net::{IpAddr, SocketAddr};
use std::sync::Arc;
use std::time::Duration;
use tokio::time::{Instant, timeout_at};
use tracing::debug;

/// Why a request was abandoned while its body was being received.
///
/// The gate returns it as the service error. Hyper then drops the connection
/// without writing a response, which is what releases the connection's slot:
/// a client that will not send its body is not owed a reply, and nothing is
/// left that depends on it reading one.
#[derive(Debug)]
pub(crate) enum BodyReadError {
    /// The whole body did not arrive within `request_body_timeout`.
    Deadline,
    /// No data arrived for `request_body_idle_timeout`.
    Idle,
    /// More data arrived than `max_request_body_size` allows.
    TooLarge,
    /// The connection failed, or was closed, before the body was complete.
    Transport(hyper::Error),
}

impl fmt::Display for BodyReadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Deadline => f.write_str("request body not received before the deadline"),
            Self::Idle => f.write_str("request body stalled"),
            Self::TooLarge => f.write_str("request body larger than allowed"),
            Self::Transport(error) => write!(f, "request body not received: {error}"),
        }
    }
}

impl std::error::Error for BodyReadError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Transport(error) => Some(error),
            Self::Deadline | Self::Idle | Self::TooLarge => None,
        }
    }
}

/// Limits on receiving a request body.
struct BodyLimits {
    max_len: u64,
    /// For the whole body. Progress does not extend it, which is what stops
    /// a client that dribbles a byte at a time.
    total: Option<Duration>,
    /// Between two pieces of data.
    idle: Option<Duration>,
}

impl BodyLimits {
    /// Whether the routes would read this request's body.
    ///
    /// Only the POST route reads one, and only once the declared length has
    /// passed its limit check. A POST with no `Content-Length` (which
    /// includes a plain chunked body) or with one over the limit is rejected
    /// there from the head alone, so it is passed on untouched to be answered
    /// as before. Anything else is assumed to be read.
    fn applies_to<B>(&self, request: &Request<B>) -> bool {
        if request.method() != Method::POST {
            return false;
        }
        let Some(declared) = request.headers().get(CONTENT_LENGTH) else {
            return false;
        };
        let declared = declared
            .to_str()
            .ok()
            .and_then(|value| value.trim().parse::<u64>().ok());
        declared.is_none_or(|len| len <= self.max_len)
    }

    /// Receives the whole body, or fails once a limit is passed.
    async fn read(&self, body: &mut Body) -> Result<Bytes, BodyReadError> {
        let started = Instant::now();
        let total = self.total.map(|limit| started + limit);
        let mut idle = self.idle.map(|limit| started + limit);
        let mut received = BytesMut::new();

        loop {
            let next = until(total, until(idle, body.data()))
                .await
                .ok_or(BodyReadError::Deadline)?
                .ok_or(BodyReadError::Idle)?;
            let chunk = match next {
                None => return Ok(received.freeze()),
                Some(Err(error)) => return Err(BodyReadError::Transport(error)),
                Some(Ok(chunk)) => chunk,
            };
            // Only data counts as progress.
            if chunk.is_empty() {
                continue;
            }
            // The declared length was within the limit, but it does not bound
            // a body sent with chunked transfer encoding as well.
            if (received.len() + chunk.len()) as u64 > self.max_len {
                return Err(BodyReadError::TooLarge);
            }
            received.extend_from_slice(&chunk);
            idle = self.idle.map(|limit| Instant::now() + limit);
        }
    }
}

/// Awaits `future`, giving up at `deadline` if there is one.
async fn until<F: Future>(deadline: Option<Instant>, future: F) -> Option<F::Output> {
    match deadline {
        Some(deadline) => timeout_at(deadline, future).await.ok(),
        None => Some(future.await),
    }
}

pub(crate) struct HttpGate {
    auth: Arc<Option<BasicAuth>>,
    /// Budget of its own, never the per-method limiter: an admitted call is
    /// still charged exactly once, at its method tier.
    admission: Option<GovernorRateLimiter>,
    cors: Option<CorsConfig>,
    body: BodyLimits,
}

impl HttpGate {
    pub(crate) fn new(auth: Arc<Option<BasicAuth>>, settings: &RpcServerConfig) -> Self {
        let admission = settings
            .admission_rate_limit()
            .map(|(max_rps, burst)| GovernorRateLimiter::new(RateLimitConfig { max_rps, burst }));
        Self {
            auth,
            admission,
            cors: CorsConfig::from_settings_quiet(settings),
            body: BodyLimits {
                max_len: max_post_body_len(settings),
                total: settings.request_body_timeout_duration(),
                idle: settings.request_body_idle_timeout_duration(),
            },
        }
    }

    /// Serves one request from `remote` with that peer bound as the client
    /// identity, so admission, method-tier limiting and session quotas are
    /// all attributed to the same address: the TCP peer, never a
    /// client-supplied forwarding header.
    pub(crate) async fn serve_scoped<S>(
        self: Arc<Self>,
        remote: SocketAddr,
        request: Request<Body>,
        service: S,
    ) -> Result<Response<Body>, BodyReadError>
    where
        S: Service<Request<Body>, Response = Response<Body>, Error = Infallible>,
    {
        client_identity::scope_client_key(
            remote.ip().to_string(),
            self.serve(remote, request, service),
        )
        .await
    }

    async fn serve<S>(
        &self,
        remote: SocketAddr,
        request: Request<Body>,
        mut service: S,
    ) -> Result<Response<Body>, BodyReadError>
    where
        S: Service<Request<Body>, Response = Response<Body>, Error = Infallible>,
    {
        if let Some(refusal) = self.refuse(remote.ip(), &request) {
            // The body is dropped unread; hyper then closes the connection
            // rather than draining what the client still wants to send.
            return Ok(refusal);
        }

        let mut request = match self.receive_body(request).await {
            Ok(request) => request,
            Err(error) => {
                debug!(
                    target: "neo::rpc",
                    peer = %remote,
                    %error,
                    "closing RPC connection"
                );
                return Err(error);
            }
        };

        // The deadlines end with the body: nothing below is timed.
        request.extensions_mut().insert(remote);
        match service.call(request).await {
            Ok(response) => Ok(response),
            Err(never) => match never {},
        }
    }

    /// Replaces the body the routes would read with its buffered content,
    /// received under the body deadlines.
    async fn receive_body(&self, request: Request<Body>) -> Result<Request<Body>, BodyReadError> {
        if !self.body.applies_to(&request) {
            return Ok(request);
        }
        let (head, mut body) = request.into_parts();
        let received = self.body.read(&mut body).await?;
        Ok(Request::from_parts(head, Body::from(received)))
    }

    /// Decides from the request head alone whether to refuse the request.
    fn refuse<B>(&self, peer: IpAddr, request: &Request<B>) -> Option<Response<Body>> {
        // A CORS preflight carries neither credentials nor a body; its route
        // answers it without doing any work on behalf of the client.
        if request.method() == Method::OPTIONS {
            return None;
        }
        let origin = request.headers().get(ORIGIN);

        if let Some(limiter) = &self.admission
            && limiter.check(peer).is_blocked()
        {
            return Some(self.refusal(
                RpcError::too_many_requests(),
                StatusCode::TOO_MANY_REQUESTS,
                origin,
            ));
        }

        let auth = self.auth.as_ref().as_ref()?;
        let header = request
            .headers()
            .get(AUTHORIZATION)
            .and_then(|value| value.to_str().ok());
        if verify_basic_auth(header, auth) {
            return None;
        }
        Some(self.refusal(RpcError::access_denied(), StatusCode::UNAUTHORIZED, origin))
    }

    /// A transport-level refusal. The JSON-RPC id is unknown because the body
    /// has not been read, so the error carries a null id.
    fn refusal(
        &self,
        error: RpcError,
        status: StatusCode,
        origin: Option<&HeaderValue>,
    ) -> Response<Body> {
        RPC_REQ_TOTAL.inc();
        RPC_ERR_TOTAL.inc();
        let unauthorized = status == StatusCode::UNAUTHORIZED;
        let mut response = build_http_response(
            Some(error_response(None, error)),
            unauthorized,
            unauthorized,
        );
        *response.status_mut() = status;
        apply_cors(&mut response, self.cors.as_ref(), origin);
        response
    }
}
