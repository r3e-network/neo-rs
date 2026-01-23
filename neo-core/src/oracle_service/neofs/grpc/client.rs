use super::super::proto::neofs_v2;
use std::time::Duration;
use tonic::transport::{Channel, ClientTlsConfig, Endpoint};

const NEOFS_GRPC_CONNECT_TIMEOUT: Duration = Duration::from_millis(120_000);

pub(super) async fn neofs_grpc_client(
    endpoint: &str,
) -> Result<neofs_v2::object::object_service_client::ObjectServiceClient<Channel>, String> {
    let mut builder = Endpoint::from_shared(endpoint.to_string())
        .map_err(|err| format!("invalid neofs endpoint: {err}"))?
        .connect_timeout(NEOFS_GRPC_CONNECT_TIMEOUT);
    if endpoint.to_ascii_lowercase().starts_with("https://") {
        builder = builder
            .tls_config(ClientTlsConfig::new())
            .map_err(|err| format!("invalid neofs tls config: {err}"))?;
    }
    let channel = builder
        .connect()
        .await
        .map_err(|err| format!("neofs grpc connect failed: {err}"))?;
    Ok(neofs_v2::object::object_service_client::ObjectServiceClient::new(channel))
}
