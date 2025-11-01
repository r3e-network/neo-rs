// Copyright (C) 2015-2025 The Neo Project.
//
// Rust port of `Neo.Plugins.RestServer.Middleware.RestServerMiddleware`.

use hyper::header::SERVER;
use neo_extensions::assembly_extensions::AssemblyExtensions;
use warp::reply::Response;

/// Helper responsible for decorating responses with identifying headers, mirroring
/// the ASP.NET Core middleware used by the C# implementation.
pub struct RestServerMiddleware;

impl RestServerMiddleware {
    /// Adds the `Server` header containing the host and plugin versions.
    pub fn set_server_information_header(response: &mut Response) {
        let plugin_name = env!("CARGO_PKG_NAME");
        let plugin_version = env!("CARGO_PKG_VERSION");

        let (host_name, host_version) = match std::env::current_exe() {
            Ok(path) => {
                let name = path
                    .file_stem()
                    .and_then(|stem| stem.to_str())
                    .unwrap_or("neo-rs")
                    .to_string();
                let version = path.to_string_lossy().as_ref().get_version();
                (name, version)
            }
            Err(_) => ("neo-rs".to_string(), plugin_version.to_string()),
        };

        let value = format!("{host_name}/{host_version} {plugin_name}/{plugin_version}");

        if let Ok(header_value) = value.parse() {
            response.headers_mut().insert(SERVER, header_value);
        }
    }
}
