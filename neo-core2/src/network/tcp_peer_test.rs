use std::net::{TcpStream, TcpListener};
use std::thread;
use std::io::Read;
use std::sync::{Arc, Mutex};
use crate::network::payload;
use crate::network::tcp_peer::{TCPPeer, ServerConfig, new_test_server};
use crate::network::message::Message;
use crate::network::require;

fn conn_read_stub(conn: Arc<Mutex<TcpStream>>) {
    let mut conn = conn.lock().unwrap();
    let mut buffer = [0; 1024];
    while let Ok(_) = conn.read(&mut buffer) {}
}

#[test]
fn test_peer_handshake() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let server = TcpStream::connect(addr).unwrap();
    let client = listener.accept().unwrap().0;

    let tcp_s = TCPPeer::new(server.try_clone().unwrap(), "", new_test_server(ServerConfig::default()));
    tcp_s.server.transports[0].accept(); // properly initialize the address list
    let tcp_c = TCPPeer::new(client.try_clone().unwrap(), "", new_test_server(ServerConfig::default()));
    tcp_c.server.transports[0].accept();

    // Something should read things written into the pipe.
    let tcp_s_conn = Arc::new(Mutex::new(tcp_s.conn.try_clone().unwrap()));
    let tcp_c_conn = Arc::new(Mutex::new(tcp_c.conn.try_clone().unwrap()));
    thread::spawn(move || conn_read_stub(tcp_s_conn));
    thread::spawn(move || conn_read_stub(tcp_c_conn));

    // No handshake yet.
    require::equal(false, tcp_s.handshaked());
    require::equal(false, tcp_c.handshaked());

    // No ordinary messages can be written.
    require::error(tcp_s.enqueue_p2p_message(&Message::default()));
    require::error(tcp_c.enqueue_p2p_message(&Message::default()));

    // Try to mess with VersionAck on both client and server, it should fail.
    require::error(tcp_s.send_version_ack(&Message::default()));
    require::error(tcp_s.handle_version_ack());
    require::error(tcp_c.send_version_ack(&Message::default()));
    require::error(tcp_c.handle_version_ack());

    // No handshake yet.
    require::equal(false, tcp_s.handshaked());
    require::equal(false, tcp_c.handshaked());

    // Now send and handle versions, but in a different order on client and
    // server.
    require::no_error(tcp_c.send_version());
    require::error(tcp_c.handle_version_ack()); // Didn't receive version yet.
    require::no_error(tcp_s.handle_version(&payload::Version::default()));
    require::error(tcp_s.send_version_ack(&Message::default())); // Didn't send version yet.
    require::no_error(tcp_c.handle_version(&payload::Version::default()));
    require::no_error(tcp_s.send_version());

    // No handshake yet.
    require::equal(false, tcp_s.handshaked());
    require::equal(false, tcp_c.handshaked());

    // These are sent/received and should fail now.
    require::error(tcp_c.send_version());
    require::error(tcp_s.handle_version(&payload::Version::default()));
    require::error(tcp_c.handle_version(&payload::Version::default()));
    require::error(tcp_s.send_version());

    // Now send and handle ACK, again in a different order on client and
    // server.
    require::no_error(tcp_c.send_version_ack(&Message::default()));
    require::no_error(tcp_s.handle_version_ack());
    require::no_error(tcp_c.handle_version_ack());
    require::no_error(tcp_s.send_version_ack(&Message::default()));

    // Handshaked now.
    require::equal(true, tcp_s.handshaked());
    require::equal(true, tcp_c.handshaked());

    // Subsequent ACKing should fail.
    require::error(tcp_c.send_version_ack(&Message::default()));
    require::error(tcp_s.handle_version_ack());
    require::error(tcp_c.handle_version_ack());
    require::error(tcp_s.send_version_ack(&Message::default()));

    // Now regular messaging can proceed.
    require::no_error(tcp_s.enqueue_p2p_message(&Message::default()));
    require::no_error(tcp_c.enqueue_p2p_message(&Message::default()));
}
