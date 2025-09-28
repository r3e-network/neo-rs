// Copyright (C) 2015-2025 The Neo Project.
//
// u_pn_p.rs file belongs to the neo project and is free
// software distributed under the MIT software license, see the
// accompanying file LICENSE in the main directory of the
// repository or http://www.opensource.org/licenses/mit-license.php
// for more details.
//
// Redistribution and use in source and binary forms with or without
// modifications are permitted.

use quick_xml::events::Event;
use quick_xml::Reader;
use reqwest::blocking::{Client, Response};
use reqwest::header::{HeaderMap, HeaderValue, CONTENT_TYPE};
use std::net::{IpAddr, Ipv4Addr, SocketAddr, UdpSocket};
use std::time::{Duration, Instant};

/// Provides methods for interacting with UPnP devices.
pub struct UPnP;

/// Static service URL for UPnP
static mut SERVICE_URL: Option<String> = None;

/// Gets or sets the timeout for discovering the UPnP device.
static mut TIMEOUT: Duration = Duration::from_secs(3);

impl UPnP {
    /// Gets the timeout for discovering the UPnP device.
    pub fn get_timeout() -> Duration {
        unsafe { TIMEOUT }
    }

    /// Sets the timeout for discovering the UPnP device.
    pub fn set_timeout(timeout: Duration) {
        unsafe {
            TIMEOUT = timeout;
        }
    }

    /// Sends an Udp broadcast message to discover the UPnP device.
    /// Returns true if the UPnP device is successfully discovered; otherwise, false.
    pub fn discover() -> bool {
        let socket = match UdpSocket::bind("0.0.0.0:0") {
            Ok(s) => s,
            Err(_) => return false,
        };

        let timeout = Self::get_timeout();
        let _ = socket.set_read_timeout(Some(timeout));
        let _ = socket.set_broadcast(true);

        let req = "M-SEARCH * HTTP/1.1\r\n\
                   HOST: 239.255.255.250:1900\r\n\
                   ST:upnp:rootdevice\r\n\
                   MAN:\"ssdp:discover\"\r\n\
                   MX:3\r\n\r\n";

        let data = req.as_bytes();
        let broadcast_addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(239, 255, 255, 250)), 1900);

        // Send three discovery messages
        for _ in 0..3 {
            if socket.send_to(data, broadcast_addr).is_err() {
                return false;
            }
        }

        let mut buffer = [0u8; 0x1000];
        let start = Instant::now();

        loop {
            if start.elapsed() >= timeout {
                break;
            }

            match socket.recv(&mut buffer) {
                Ok(length) => {
                    if let Ok(resp) = std::str::from_utf8(&buffer[..length]) {
                        let resp_lower = resp.to_lowercase();
                        if resp_lower.contains("upnp:rootdevice") {
                            if let Some(location_pos) = resp_lower.find("location:") {
                                let location_start = location_pos + 9;
                                if let Some(resp_after_location) = resp.get(location_start..) {
                                    if let Some(cr_pos) = resp_after_location.find('\r') {
                                        let location_url = resp_after_location[..cr_pos].trim();
                                        if let Some(service_url) =
                                            Self::get_service_url(location_url)
                                        {
                                            if !service_url.is_empty() {
                                                unsafe {
                                                    SERVICE_URL = Some(service_url);
                                                }
                                                return true;
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
                Err(_) => continue,
            }
        }

        false
    }

    fn get_service_url(location_url: &str) -> Option<String> {
        // Fetch and parse device description XML
        let resp = reqwest::blocking::get(location_url).ok()?;
        if !resp.status().is_success() {
            return None;
        }
        let bytes = resp.bytes().ok()?;

        let mut reader = Reader::from_reader(bytes.as_ref());
        reader.trim_text(true);

        let mut buf = Vec::new();
        let mut device_type_ok = false;
        let mut control_url: Option<String> = None;
        let mut in_service = false;
        let mut in_service_type = false;
        let mut in_control_url = false;

        while let Ok(ev) = reader.read_event_into(&mut buf) {
            match ev {
                Event::Eof => break,
                Event::Text(t) => {
                    let text = t.unescape().unwrap_or_default().to_string();
                    if in_service_type {
                        if text.contains("WANIPConnection") || text.contains("WANPPPConnection") {
                            in_service = true;
                        } else {
                            in_service = false;
                        }
                        in_service_type = false;
                    } else if in_control_url && in_service {
                        control_url = Some(text);
                        in_control_url = false;
                    } else if text.contains("InternetGatewayDevice") {
                        device_type_ok = true;
                    }
                }
                Event::Start(e) => {
                    let name = String::from_utf8_lossy(e.name().as_ref()).to_lowercase();
                    if name.ends_with("devicetype") {
                        // next Text may contain device type
                    } else if name.ends_with("service") {
                        in_service = false;
                    } else if name.ends_with("servicetype") {
                        in_service_type = true;
                    } else if name.ends_with("controlurl") {
                        in_control_url = true;
                    }
                }
                Event::End(e) => {
                    let name = String::from_utf8_lossy(e.name().as_ref()).to_lowercase();
                    if name.ends_with("service") {
                        in_service = false;
                    }
                }
                _ => {}
            }
            buf.clear();
        }

        if !device_type_ok {
            return None;
        }
        let control = control_url?;
        Some(Self::combine_urls(location_url, &control))
    }

    fn combine_urls(location: &str, control: &str) -> String {
        // Extract scheme+host from location and join with control path
        match location.find("://") {
            Some(pos) => {
                let rest = &location[pos + 3..];
                match rest.find('/') {
                    Some(slash) => format!("{}{}", &location[..pos + 3 + slash], control),
                    None => format!("{}/{}", location, control.trim_start_matches('/')),
                }
            }
            None => format!(
                "{}/{}",
                location.trim_end_matches('/'),
                control.trim_start_matches('/')
            ),
        }
    }

    fn run_command(service_url: &str, command: &str, args: &str) -> Result<String, String> {
        let envelope = format!(
            "<?xml version=\"1.0\"?>\
            <s:Envelope xmlns:s=\"http://schemas.xmlsoap.org/soap/envelope/\" s:encodingStyle=\"http://schemas.xmlsoap.org/soap/encoding/\">\
            <s:Body>\
            <u:{cmd} xmlns:u=\"urn:schemas-upnp-org:service:WANIPConnection:1\">{args}</u:{cmd}>\
            </s:Body>\
            </s:Envelope>",
            cmd = command,
            args = args
        );

        let mut headers = HeaderMap::new();
        headers.insert(
            "SOAPACTION",
            HeaderValue::from_str(&format!(
                "\"urn:schemas-upnp-org:service:WANIPConnection:1#{}\"",
                command
            ))
            .map_err(|e| e.to_string())?,
        );
        headers.insert(
            CONTENT_TYPE,
            HeaderValue::from_static("text/xml; charset=\"utf-8\""),
        );

        let client = Client::builder()
            .timeout(Self::get_timeout())
            .default_headers(headers)
            .build()
            .map_err(|e| e.to_string())?;

        let resp: Response = client
            .post(service_url)
            .body(envelope)
            .send()
            .map_err(|e| e.to_string())?;

        let status = resp.status();
        let text = resp.text().map_err(|e| e.to_string())?;
        if !status.is_success() {
            return Err(format!("UPnP SOAP error: HTTP {}: {}", status, text));
        }
        Ok(text)
    }

    /// Forwards a port on the UPnP device.
    /// Mirrors C# `UPnP.ForwardPort(int port, ProtocolType protocol, string description)`.
    /// Returns true if the port is successfully forwarded; otherwise, false.
    pub fn forward_port(port: i32, protocol: &str, description: &str) -> bool {
        let service_url = unsafe {
            match &SERVICE_URL {
                Some(url) => url.clone(),
                None => return false,
            }
        };

        let internal_ip = match Self::get_local_ip() {
            Some(ip) => ip,
            None => return false,
        };

        let args = format!(
            "<NewRemoteHost></NewRemoteHost>\
            <NewExternalPort>{}</NewExternalPort>\
            <NewProtocol>{}</NewProtocol>\
            <NewInternalPort>{}</NewInternalPort>\
            <NewInternalClient>{}</NewInternalClient>\
            <NewEnabled>1</NewEnabled>\
            <NewPortMappingDescription>{}</NewPortMappingDescription>\
            <NewLeaseDuration>0</NewLeaseDuration>",
            port, protocol, port, internal_ip, description
        );

        Self::run_command(&service_url, "AddPortMapping", &args).is_ok()
    }

    /// Deletes a forwarded port on the UPnP device.
    /// Mirrors C# `UPnP.DeleteForwardingRule(int port, ProtocolType protocol)`.
    /// Returns true if the port forwarding is successfully deleted; otherwise, false.
    pub fn delete_forwarding_rule(port: i32, protocol: &str) -> bool {
        let service_url = unsafe {
            match &SERVICE_URL {
                Some(url) => url.clone(),
                None => return false,
            }
        };

        let args = format!(
            "<NewRemoteHost></NewRemoteHost>\
            <NewExternalPort>{}</NewExternalPort>\
            <NewProtocol>{}</NewProtocol>",
            port, protocol
        );

        Self::run_command(&service_url, "DeletePortMapping", &args).is_ok()
    }

    /// Gets the external IP address from the UPnP device.
    pub fn get_external_ip() -> Option<String> {
        let service_url = unsafe {
            match &SERVICE_URL {
                Some(url) => url.clone(),
                None => return None,
            }
        };

        match Self::run_command(&service_url, "GetExternalIPAddress", "") {
            Ok(response) => {
                let mut reader = Reader::from_str(&response);
                reader.trim_text(true);
                let mut buf = Vec::new();
                let mut in_node = false;
                while let Ok(ev) = reader.read_event_into(&mut buf) {
                    match ev {
                        Event::Eof => break,
                        Event::Start(e) => {
                            let name = String::from_utf8_lossy(e.name().as_ref()).to_string();
                            if name.eq_ignore_ascii_case("NewExternalIPAddress") {
                                in_node = true;
                            }
                        }
                        Event::Text(t) if in_node => {
                            return Some(t.unescape().unwrap_or_default().to_string());
                        }
                        Event::End(e) => {
                            let name = String::from_utf8_lossy(e.name().as_ref()).to_string();
                            if name.eq_ignore_ascii_case("NewExternalIPAddress") {
                                in_node = false;
                            }
                        }
                        _ => {}
                    }
                    buf.clear();
                }
                None
            }
            Err(_) => None,
        }
    }

    fn get_local_ip() -> Option<String> {
        // Determine the primary local IPv4 by opening a UDP socket
        use std::net::UdpSocket;

        let socket = UdpSocket::bind("0.0.0.0:0").ok()?;
        socket.connect("8.8.8.8:80").ok()?;
        let addr = socket.local_addr().ok()?;
        Some(addr.ip().to_string())
    }
}
