use std::io;
use std::net::SocketAddr;
use std::pin::Pin;
use std::task::{Context, Poll};
use std::time::Duration;
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
use tokio::net::TcpStream;
use tokio::sync::OwnedSemaphorePermit;
use tracing::{error, warn};

pub(crate) struct TlsConnection {
    stream: tokio_rustls::server::TlsStream<TcpStream>,
    remote_addr: SocketAddr,
    _permit: OwnedSemaphorePermit,
}

impl TlsConnection {
    pub(crate) fn new(
        stream: tokio_rustls::server::TlsStream<TcpStream>,
        remote_addr: SocketAddr,
        permit: OwnedSemaphorePermit,
    ) -> Self {
        Self {
            stream,
            remote_addr,
            _permit: permit,
        }
    }

    pub(crate) const fn remote_addr(&self) -> SocketAddr {
        self.remote_addr
    }
}

impl AsyncRead for TlsConnection {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        Pin::new(&mut self.get_mut().stream).poll_read(cx, buf)
    }
}

impl AsyncWrite for TlsConnection {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        data: &[u8],
    ) -> Poll<io::Result<usize>> {
        Pin::new(&mut self.get_mut().stream).poll_write(cx, data)
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.get_mut().stream).poll_flush(cx)
    }

    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.get_mut().stream).poll_shutdown(cx)
    }
}

pub(crate) struct PlainConnection {
    stream: TcpStream,
    remote_addr: SocketAddr,
    _permit: OwnedSemaphorePermit,
}

impl PlainConnection {
    pub(crate) fn new(
        stream: TcpStream,
        remote_addr: SocketAddr,
        permit: OwnedSemaphorePermit,
    ) -> Self {
        Self {
            stream,
            remote_addr,
            _permit: permit,
        }
    }

    pub(crate) const fn remote_addr(&self) -> SocketAddr {
        self.remote_addr
    }
}

impl AsyncRead for PlainConnection {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        Pin::new(&mut self.get_mut().stream).poll_read(cx, buf)
    }
}

impl AsyncWrite for PlainConnection {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        data: &[u8],
    ) -> Poll<io::Result<usize>> {
        Pin::new(&mut self.get_mut().stream).poll_write(cx, data)
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.get_mut().stream).poll_flush(cx)
    }

    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.get_mut().stream).poll_shutdown(cx)
    }
}

pub(crate) fn apply_tcp_keepalive(_stream: &TcpStream, _keepalive: Option<Duration>) {
    // TCP keepalive was previously applied via the `socket2` crate.
    // jsonrpsee 0.24 manages socket options internally; the keepalive
    // configuration (if needed) is handled via the server builder's
    // middleware rather than per-stream. This function is kept as a
    // no-op so the existing call sites in legacy code continue to
    // compile; it can be removed once those callers are deleted.
}

pub(crate) fn log_join_error(error: tokio::task::JoinError) {
    if error.is_cancelled() {
        warn!(target: "neo", "rpc server task cancelled before completion");
    } else {
        match error.try_into_panic() {
            Ok(payload) => {
                if let Some(message) = payload.downcast_ref::<&str>() {
                    error!(target: "neo", message = %message, "rpc server panicked");
                } else if let Some(message) = payload.downcast_ref::<String>() {
                    error!(target: "neo", message = %message, "rpc server panicked");
                } else {
                    error!(target: "neo", "rpc server panicked");
                }
            }
            Err(join_err) => {
                error!(target: "neo", error = %join_err, "rpc server task failed");
            }
        }
    }
}
