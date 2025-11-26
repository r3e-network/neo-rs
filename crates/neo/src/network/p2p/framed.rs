//! Tokio-based framing helpers for Neo P2P streams with timeouts and size guards.
use super::{channels_config::ChannelsConfig, message::PAYLOAD_MAX_SIZE};
use crate::network::{NetworkError, NetworkResult};
use tokio::{
    io::AsyncReadExt,
    net::TcpStream,
    time::{timeout, Duration},
};

/// Minimal framed reader that wraps Neo P2P length-prefixing with timeouts and size guards.
pub struct FramedSocket<'a> {
    stream: &'a mut TcpStream,
}

/// Runtime framing configuration to keep read behaviour consistent across the stack.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FrameConfig {
    pub read_timeout_handshake: Duration,
    pub read_timeout_active: Duration,
    pub write_timeout: Duration,
    pub shutdown_timeout: Duration,
}

impl From<&ChannelsConfig> for FrameConfig {
    fn from(cfg: &ChannelsConfig) -> Self {
        Self {
            read_timeout_handshake: cfg.handshake_timeout,
            read_timeout_active: cfg.read_timeout_active,
            write_timeout: cfg.write_timeout,
            shutdown_timeout: cfg.shutdown_timeout,
        }
    }
}

impl From<ChannelsConfig> for FrameConfig {
    fn from(cfg: ChannelsConfig) -> Self {
        FrameConfig::from(&cfg)
    }
}

impl Default for FrameConfig {
    fn default() -> Self {
        FrameConfig::from(&ChannelsConfig::default())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::network::error::NetworkError;
    use tokio::net::{TcpListener, TcpStream};

    async fn silent_pair() -> (TcpStream, TcpStream) {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind listener");
        let addr = listener.local_addr().expect("listener addr");

        let client = TcpStream::connect(addr);
        let server = listener.accept();
        let (client_stream, server_stream) = tokio::join!(client, server);
        let client_stream = client_stream.expect("client connect succeeded");
        let (server_stream, _) = server_stream.expect("server accept succeeded");
        (client_stream, server_stream)
    }

    #[tokio::test]
    async fn read_frame_times_out_when_peer_silent() {
        let (mut client_stream, _server_stream) = silent_pair().await;

        let mut framed = FramedSocket::new(&mut client_stream);
        let cfg = FrameConfig {
            read_timeout_handshake: Duration::from_millis(10),
            read_timeout_active: Duration::from_millis(10),
            write_timeout: Duration::from_secs(1),
            shutdown_timeout: Duration::from_secs(1),
        };

        let result = framed.read_frame(&cfg, false).await;
        assert!(
            matches!(result, Err(NetworkError::Timeout)),
            "expected timeout error, got: {:?}",
            result
        );
    }

    #[tokio::test]
    async fn read_frame_times_out_in_active_session_when_peer_silent() {
        let (mut client_stream, _server_stream) = silent_pair().await;
        let mut framed = FramedSocket::new(&mut client_stream);
        let cfg = FrameConfig {
            read_timeout_handshake: Duration::from_secs(5),
            read_timeout_active: Duration::from_millis(10),
            write_timeout: Duration::from_secs(1),
            shutdown_timeout: Duration::from_secs(1),
        };

        let result = framed.read_frame(&cfg, true).await;
        assert!(
            matches!(result, Err(NetworkError::Timeout)),
            "expected active timeout error, got: {:?}",
            result
        );
    }

    #[test]
    fn frame_config_default_matches_channels_defaults() {
        let expected = FrameConfig::from(&ChannelsConfig::default());
        let actual = FrameConfig::default();
        assert_eq!(expected, actual);
    }
}

impl<'a> FramedSocket<'a> {
    pub fn new(stream: &'a mut TcpStream) -> Self {
        Self { stream }
    }

    /// Reads a full P2P message frame (flags + command + var-bytes payload) with a timeout applied to
    /// each underlying read.
    pub async fn read_frame(
        &mut self,
        cfg: &FrameConfig,
        handshake_complete: bool,
    ) -> NetworkResult<Vec<u8>> {
        let timeout_duration = if handshake_complete {
            cfg.read_timeout_active
        } else {
            cfg.read_timeout_handshake
        };

        let mut message_bytes = Vec::with_capacity(2);

        let flag_byte = self
            .read_exact_timeout(&mut [0u8; 1], timeout_duration)
            .await?;
        message_bytes.push(flag_byte);

        let command_byte = self
            .read_exact_timeout(&mut [0u8; 1], timeout_duration)
            .await?;
        message_bytes.push(command_byte);

        let (payload_length, mut length_bytes) = self.read_var_int(timeout_duration).await?;
        message_bytes.append(&mut length_bytes);

        let mut payload = vec![0u8; payload_length as usize];
        if payload_length > 0 {
            self.read_exact_slice(&mut payload, timeout_duration, "payload")
                .await?;
        }
        message_bytes.extend_from_slice(&payload);

        Ok(message_bytes)
    }

    async fn read_exact_slice(
        &mut self,
        buf: &mut [u8],
        timeout_duration: Duration,
        context: &str,
    ) -> NetworkResult<()> {
        timeout(timeout_duration, self.stream.read_exact(buf))
            .await
            .map_err(|_| NetworkError::Timeout)?
            .map_err(|e| NetworkError::ConnectionError(format!("Failed to read {context}: {e}")))?;
        Ok(())
    }

    async fn read_exact_timeout(
        &mut self,
        buf: &mut [u8; 1],
        timeout_duration: Duration,
    ) -> NetworkResult<u8> {
        self.read_exact_slice(buf, timeout_duration, "byte").await?;
        Ok(buf[0])
    }

    async fn read_var_int(&mut self, timeout_duration: Duration) -> NetworkResult<(u64, Vec<u8>)> {
        let mut first = [0u8; 1];
        self.read_exact_slice(&mut first, timeout_duration, "varint prefix")
            .await?;

        let mut bytes = vec![first[0]];
        let value = match first[0] {
            0xFD => {
                let mut buffer = [0u8; 2];
                self.read_exact_slice(&mut buffer, timeout_duration, "varint (u16)")
                    .await?;
                bytes.extend_from_slice(&buffer);
                u16::from_le_bytes(buffer) as u64
            }
            0xFE => {
                let mut buffer = [0u8; 4];
                self.read_exact_slice(&mut buffer, timeout_duration, "varint (u32)")
                    .await?;
                bytes.extend_from_slice(&buffer);
                u32::from_le_bytes(buffer) as u64
            }
            0xFF => {
                let mut buffer = [0u8; 8];
                self.read_exact_slice(&mut buffer, timeout_duration, "varint (u64)")
                    .await?;
                bytes.extend_from_slice(&buffer);
                u64::from_le_bytes(buffer)
            }
            value => value as u64,
        };

        if value > PAYLOAD_MAX_SIZE as u64 {
            return Err(NetworkError::InvalidMessage(format!(
                "Payload length {} exceeds maximum {}",
                value, PAYLOAD_MAX_SIZE
            )));
        }

        Ok((value, bytes))
    }
}
