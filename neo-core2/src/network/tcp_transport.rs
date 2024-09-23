use std::net::{TcpListener, TcpStream};
use std::sync::{Arc, RwLock};
use std::time::Duration;
use std::thread;
use std::io;
use log::{error, warn, info};
use std::sync::atomic::{AtomicBool, Ordering};

pub struct TCPTransport {
    log: Arc<log::Logger>,
    server: Arc<Server>,
    listener: Option<TcpListener>,
    bind_addr: String,
    host_port: HostPort,
    lock: RwLock<()>,
    quit: AtomicBool,
}

pub struct HostPort {
    host: String,
    port: String,
}

impl TCPTransport {
    // NewTCPTransport returns a new TCPTransport that will listen for
    // new incoming peer connections.
    pub fn new(s: Arc<Server>, bind_addr: String, log: Arc<log::Logger>) -> Self {
        let (host, port) = match bind_addr.split_once(':') {
            Some((h, p)) => (h.to_string(), p.to_string()),
            None => (bind_addr.clone(), String::new()),
        };
        TCPTransport {
            log,
            server: s,
            listener: None,
            bind_addr: bind_addr.clone(),
            host_port: HostPort { host, port },
            lock: RwLock::new(()),
            quit: AtomicBool::new(false),
        }
    }

    // Dial implements the Transporter interface.
    pub fn dial(&self, addr: &str, timeout: Duration) -> io::Result<Arc<TCPPeer>> {
        let conn = TcpStream::connect_timeout(&addr.parse().unwrap(), timeout)?;
        let peer = Arc::new(TCPPeer::new(conn, addr.to_string(), Arc::clone(&self.server)));
        let peer_clone = Arc::clone(&peer);
        thread::spawn(move || {
            peer_clone.handle_conn();
        });
        Ok(peer)
    }

    // Accept implements the Transporter interface.
    pub fn accept(&self) {
        let listener = TcpListener::bind(&self.bind_addr).unwrap_or_else(|err| {
            self.log.panic("TCP listen error", err);
            return;
        });

        {
            let _lock = self.lock.write().unwrap();
            if self.quit.load(Ordering::SeqCst) {
                return;
            }
            self.listener = Some(listener.try_clone().unwrap());
            self.bind_addr = listener.local_addr().unwrap().to_string();
            let (host, port) = self.bind_addr.split_once(':').unwrap();
            self.host_port.host = host.to_string();
            self.host_port.port = port.to_string();
        }

        for stream in listener.incoming() {
            match stream {
                Ok(conn) => {
                    let peer = Arc::new(TCPPeer::new(conn, String::new(), Arc::clone(&self.server)));
                    let peer_clone = Arc::clone(&peer);
                    thread::spawn(move || {
                        peer_clone.handle_conn();
                    });
                }
                Err(err) => {
                    let quit = {
                        let _lock = self.lock.read().unwrap();
                        self.quit.load(Ordering::SeqCst)
                    };
                    if err.kind() == io::ErrorKind::WouldBlock && quit {
                        break;
                    }
                    self.log.warn("TCP accept error", err);
                }
            }
        }
    }

    // Close implements the Transporter interface.
    pub fn close(&self) {
        let _lock = self.lock.write().unwrap();
        if let Some(listener) = &self.listener {
            listener.shutdown(std::net::Shutdown::Both).unwrap();
        }
        self.quit.store(true, Ordering::SeqCst);
    }

    // Proto implements the Transporter interface.
    pub fn proto(&self) -> &str {
        "tcp"
    }

    // HostPort implements the Transporter interface.
    pub fn host_port(&self) -> (String, String) {
        let _lock = self.lock.read().unwrap();
        (self.host_port.host.clone(), self.host_port.port.clone())
    }
}
