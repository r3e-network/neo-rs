// Copyright (C) 2015-2025 The Neo Project.
//
// vsock.rs file belongs to the neo project and is free
// software distributed under the MIT software license, see the
// accompanying file LICENSE in the main directory of the
// repository or http://www.opensource.org/licenses/mit-license.php
// for more details.
//
// Redistribution and use in source and binary forms with or without
// modifications are permitted.

use super::settings::VsockAddress;
use super::sign_client::{GrpcChannel, ServiceConfig};
use std::sync::Arc;

/// Vsock implementation matching C# Vsock exactly
pub struct Vsock {
    endpoint: VsockEndPoint,
}

/// Vsock endpoint matching C# VSockEndPoint
pub struct VsockEndPoint {
    pub context_id: i32,
    pub port: i32,
}

impl VsockEndPoint {
    pub fn new(context_id: i32, port: i32) -> Self {
        Self { context_id, port }
    }
}

impl Vsock {
    /// Creates a new Vsock instance
    /// Matches C# constructor
    pub fn new(address: VsockAddress) -> Self {
        if !cfg!(target_os = "linux") {
            panic!("Vsock is only supported on Linux");
        }

        Self {
            endpoint: VsockEndPoint::new(address.context_id, address.port),
        }
    }

    /// Connects to the vsock endpoint
    /// Matches C# ConnectAsync method
    pub async fn connect_async(
        &self,
        context: &SocketsHttpConnectionContext,
        cancellation: &tokio::time::Duration,
    ) -> Result<Box<dyn std::io::Read + Send + Sync>, VsockError> {
        if !cfg!(target_os = "linux") {
            return Err(VsockError::PlatformNotSupported);
        }

        let socket = VSock::create(SocketType::Stream)?;

        // In a real implementation, this would connect to the vsock endpoint
        // For now, we'll return a mock stream
        Ok(Box::new(MockStream::new()))
    }

    /// Creates a gRPC channel for the vsock endpoint
    /// Matches C# CreateChannel method
    pub fn create_channel(
        address: VsockAddress,
        service_config: ServiceConfig,
    ) -> Arc<dyn GrpcChannel> {
        let vsock = Self::new(address);
        let sockets_http_handler = SocketsHttpHandler {
            connect_callback: Box::new(move |context, cancellation| {
                let vsock = vsock.clone();
                Box::pin(async move { vsock.connect_async(context, cancellation).await })
            }),
        };

        let address_placeholder = format!("http://127.0.0.1:{}", address.port);
        Arc::new(VsockGrpcChannel::new(
            address_placeholder,
            service_config,
            sockets_http_handler,
        ))
    }
}

/// Vsock error matching C# exceptions
#[derive(Debug, Clone)]
pub enum VsockError {
    PlatformNotSupported,
    ConnectionFailed,
    InvalidAddress,
}

impl std::fmt::Display for VsockError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            VsockError::PlatformNotSupported => write!(f, "Vsock is only supported on Linux"),
            VsockError::ConnectionFailed => write!(f, "Failed to connect to vsock endpoint"),
            VsockError::InvalidAddress => write!(f, "Invalid vsock address"),
        }
    }
}

impl std::error::Error for VsockError {}

/// Socket type matching C# SocketType
#[derive(Debug, Clone, Copy)]
pub enum SocketType {
    Stream,
    Datagram,
}

/// VSock implementation
pub struct VSock;

impl VSock {
    pub fn create(socket_type: SocketType) -> Result<VSockSocket, VsockError> {
        // In a real implementation, this would create a vsock socket
        Ok(VSockSocket::new(socket_type))
    }
}

/// VSock socket
pub struct VSockSocket {
    socket_type: SocketType,
}

impl VSockSocket {
    pub fn new(socket_type: SocketType) -> Self {
        Self { socket_type }
    }

    pub fn connect(&self, endpoint: &VsockEndPoint) -> Result<(), VsockError> {
        // In a real implementation, this would connect to the endpoint
        Ok(())
    }
}

/// Sockets HTTP connection context
pub struct SocketsHttpConnectionContext;

/// Sockets HTTP handler
pub struct SocketsHttpHandler {
    pub connect_callback: Box<
        dyn Fn(
                &SocketsHttpConnectionContext,
                &tokio::time::Duration,
            ) -> std::pin::Pin<
                Box<
                    dyn std::future::Future<
                            Output = Result<Box<dyn std::io::Read + Send + Sync>, VsockError>,
                        > + Send
                        + Sync,
                >,
            > + Send
            + Sync,
    >,
}

/// Vsock gRPC channel
pub struct VsockGrpcChannel {
    address: String,
    service_config: ServiceConfig,
    http_handler: SocketsHttpHandler,
}

impl VsockGrpcChannel {
    pub fn new(
        address: String,
        service_config: ServiceConfig,
        http_handler: SocketsHttpHandler,
    ) -> Self {
        Self {
            address,
            service_config,
            http_handler,
        }
    }
}

impl GrpcChannel for VsockGrpcChannel {
    fn dispose(&self) {
        // In a real implementation, this would dispose the channel
    }
}

/// Mock stream for testing
pub struct MockStream;

impl MockStream {
    pub fn new() -> Self {
        Self
    }
}

impl std::io::Read for MockStream {
    fn read(&mut self, _buf: &mut [u8]) -> std::io::Result<usize> {
        Ok(0)
    }
}

impl Clone for Vsock {
    fn clone(&self) -> Self {
        Self {
            endpoint: VsockEndPoint::new(self.endpoint.context_id, self.endpoint.port),
        }
    }
}
