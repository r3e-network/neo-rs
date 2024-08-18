// Copyright @ 2023 - 2024, R3E Network
// All Rights Reserved


use std::io::Error as IoError;
use std::net::SocketAddr;
use std::sync::Arc;
use std::sync::atomic::{AtomicI32, AtomicU32, Ordering::Relaxed};
use std::time::Duration;

use dashmap::DashMap;
use tokio::{io::AsyncWriteExt, runtime::Handle, sync::mpsc};
use tokio::net::{TcpListener, TcpStream};
use tokio::net::tcp::{OwnedReadHalf, OwnedWriteHalf};
use tokio::sync::mpsc::error::TrySendError;
use tokio::time::timeout;
use tokio_stream::StreamExt;
use tokio_util::bytes::BytesMut;
use tokio_util::codec::{Encoder, FramedRead};
use tokio_util::sync::{CancellationToken, DropGuard};

use neo_core::types::Bytes;
use crate::{*, NetEvent::*};


const SEND_TIMEOUT: Duration = Duration::from_secs(5);
const DIAL_TIMEOUT: Duration = Duration::from_secs(10);

const CLOSE_CHAN_SIZE: usize = 128;
pub(crate) const MESSAGE_CHAN_SIZE: usize = 128;
pub(crate) const CONNECT_CHAN_SIZE: usize = 128;


pub type NetHandles = Arc<DashMap<SocketAddr, NetHandle>>;


// #[derive(Debug,Clone)]
// pub struct NetConfig {
//     pub max_peers: u32,
//     pub listen: SocketAddr,
// }


#[derive(Clone)]
pub struct NetDriver {
    max_peers: usize,
    runtime: Handle,
    listen: SocketAddr,
    handles: NetHandles,
    close_tx: mpsc::Sender<SocketAddr>,
    net_tx: mpsc::Sender<NetMessage>,
}

impl NetDriver {
    pub fn new(runtime: Handle, max_peers: usize, listen: SocketAddr, net_tx: mpsc::Sender<NetMessage>) -> Self {
        let (close_tx, close_rx) = mpsc::channel(CLOSE_CHAN_SIZE);
        let handles = Arc::new(DashMap::with_capacity(max_peers));

        let driver = Self { max_peers, runtime, listen, handles, close_tx, net_tx };
        driver.on_closing(close_rx);

        driver
    }

    #[inline]
    pub fn net_handles(&self) -> NetHandles { self.handles.clone() }

    #[inline]
    fn remove_net_handle(&self, peer: &SocketAddr) -> Option<NetHandle> {
        self.handles.remove(peer).map(|(_, v)| v)
    }

    fn on_closing(&self, mut close_rx: mpsc::Receiver<SocketAddr>) {
        let handles = self.net_handles();
        let net_tx = self.net_tx.clone();
        let _close = self.runtime.spawn(async move {
            while let Some(addr) = close_rx.recv().await {
                let handle = handles.remove(&addr);
                // let is_some = handle.is_some();
                drop(handle);

                // if is_some {
                let _ = net_tx.send(Disconnected.with_peer(addr)).await;
                //}
            }
        });
    }

    async fn do_accepting(&self, listener: TcpListener) {
        loop {
            match listener.accept().await {
                Ok((stream, peer)) => {
                    self.on_established(peer, Accepted, stream).await;
                }
                Err(err) => { // TODO: log error
                    if !is_acceptable(&err) { break; }
                }
            }
        }
    }

    pub fn on_accepting(&self, cancel: CancellationToken) {
        let listen = self.listen.clone();
        let driver = self.clone();
        self.runtime.spawn(async move {
            let listener = TcpListener::bind(listen).await
                .expect(&format!("`TcpListener::bind({})` is not ok", &listen));
            tokio::select! {
                _ = driver.do_accepting(listener) => {
                    // println!("accept existed");
                },
                _ = cancel.cancelled() => {
                    // println!("accept exit-signal from exit_rx");
                },
            }
            // println!("NetDriver::run exited!");
        });
    }

    pub fn on_connecting(&self, mut connect_rx: mpsc::Receiver<SocketAddr>) {
        let driver = self.clone();
        let _connect = self.runtime.spawn(async move {
            while let Some(peer) = connect_rx.recv().await {
                let other = driver.clone();
                let task = async move {
                    match TcpStream::connect(peer).await {
                        Ok(stream) => {
                            other.on_established(peer, Connected, stream).await;
                        }
                        Err(_err) => { // TODO: log error
                            let _ = other.net_tx.send(NotConnected.with_peer(peer)).await;
                        }
                    }
                };

                if let Err(_err) = timeout(DIAL_TIMEOUT, task).await {
                    let _ = driver.net_tx.send(NotConnected.with_peer(peer)).await;
                    // TODO: log error
                }
            }
        });
    }

    async fn on_established(&self, peer: SocketAddr, event: NetEvent, stream: TcpStream) {
        let canceler = CancellationToken::new();
        let cancelee = canceler.clone();
        let (data_tx, data_rx) = mpsc::channel(MESSAGE_CHAN_SIZE);
        {
            let handle = NetHandle::new(data_tx, canceler);
            if self.handles.len() >= self.max_peers {
                return;
            }
            self.handles.insert(peer, handle);
        }

        if let Err(_err) = self.net_tx.send_timeout(event.with_peer(peer), SEND_TIMEOUT).await {
            // println!("send_timeout to {} err {:?}", &peer, _err);
            self.remove_net_handle(&peer);
            return;
        }

        let (reader, writer) = stream.into_split();
        self.on_writing(writer, peer, data_rx);
        self.on_reading(reader, peer, cancelee);
    }

    fn on_writing(&self, mut writer: OwnedWriteHalf, peer: SocketAddr, mut data_rx: mpsc::Receiver<Bytes>) {
        let close_tx = self.close_tx.clone();
        let _write = self.runtime.spawn(async move {
            let mut encoder = MessageEncoder;
            while let Some(message) = data_rx.recv().await {
                let mut buf = BytesMut::new();
                if let Err(_err) = encoder.encode(message, &mut buf) {
                    continue; // TODO: log error
                }

                if let Err(_err) = writer.write_all(buf.as_ref()).await { // TODO: timeout
                    // println!("write to {} err: {}", &peer, _err);
                    break; // TODO: log error
                }
            }

            let _ = close_tx.send(peer).await;
        });
    }

    fn on_reading(&self, reader: OwnedReadHalf, peer: SocketAddr, cancelee: CancellationToken) {
        let net_tx = self.net_tx.clone();
        let close_tx = self.close_tx.clone();
        let _read = self.runtime.spawn(async move {
            tokio::select! {
                _ = Self::do_reading(reader, peer, net_tx) => {
                    // println!("reading exited");
                },
                _ = cancelee.cancelled() => {
                    // println!("read exit-signal from exit_rx");
                },
            }

            let _ = close_tx.send(peer).await;
        });
    }

    async fn do_reading(reader: OwnedReadHalf, peer: SocketAddr, net_tx: mpsc::Sender<NetMessage>) {
        let mut frames = FramedRead::new(reader, MessageDecoder);
        while let Some(frame) = frames.next().await {
            match frame {
                Ok(message) => {
                    let message = Message(message).with_peer(peer);
                    if let Err(_err) = net_tx.send_timeout(message, SEND_TIMEOUT).await {
                        // TODO: log error
                    }
                }
                Err(_err) => {}
            }
        }
    }
}


#[allow(dead_code)]
pub(crate) struct NetHandleStates {
    sent_get_addrs: AtomicI32,
    last_block_index: AtomicU32,
    cancel: DropGuard,
}


impl NetHandleStates {
    #[inline]
    pub fn last_block_index(&self) -> u32 {
        self.last_block_index.load(Relaxed)
    }

    #[inline]
    pub fn set_last_block_index(&self, new: u32) {
        let old = self.last_block_index();
        if new > old {
            let _ = self.last_block_index.compare_exchange(old, new, Relaxed, Relaxed);
        }
    }

    #[inline]
    pub fn on_sent_get_address(&self) {
        self.sent_get_addrs.fetch_add(1, Relaxed);
    }

    pub fn on_recv_address(&self) -> bool {
        loop {
            let old = self.sent_get_addrs.load(Relaxed);
            if old <= 0 {
                return false;
            }

            if self.sent_get_addrs.compare_exchange(old, old - 1, Relaxed, Relaxed).is_ok() {
                return true;
            }
        }
    }
}


#[derive(Clone)]
pub struct NetHandle {
    data_tx: mpsc::Sender<Bytes>,
    pub(crate) states: Arc<NetHandleStates>,
}

impl NetHandle {
    #[inline]
    pub fn new(data_tx: mpsc::Sender<Bytes>, cancel: CancellationToken) -> Self {
        Self {
            data_tx,
            states: Arc::new(NetHandleStates {
                sent_get_addrs: AtomicI32::new(0),
                last_block_index: AtomicU32::new(0),
                cancel: cancel.drop_guard(),
            }),
        }
    }

    pub fn try_seed(&self, message: Bytes) -> Result<(), SendError> {
        self.data_tx.try_send(message)
            .map_err(|err| match err {
                TrySendError::Full(_) => SendError::Fulled,
                TrySendError::Closed(_) => SendError::Closed,
            })
    }
}


#[inline]
fn is_acceptable(err: &IoError) -> bool {
    let Some(errno) = err.raw_os_error() else { return false; };

    use libc::*;
    let _ = ECONNRESET;
    matches!(errno,  ECONNRESET | ECONNABORTED | EINTR | EMFILE | ENFILE | ETIMEDOUT | EAGAIN | EBUSY)
}


#[cfg(test)]
mod test {
    use std::{io::Write, net::TcpStream};
    use tokio::runtime::Runtime;
    use neo_base::encoding::bin::*;
    use neo_core::payload::{P2pMessage, Ping};
    use crate::{driver_v2::*, ToMessageEncoded};

    #[test]
    fn test_listen() {
        let addr = "127.0.0.1:10123".parse()
            .expect("parse should be ok");

        let cancel = CancellationToken::new();
        let (net_tx, mut net_rx) = mpsc::channel(MESSAGE_CHAN_SIZE);

        let runtime = Runtime::new().expect("Runtime::new() should be ok");
        let driver = NetDriver::new(runtime.handle().clone(), 128, addr, net_tx);

        driver.on_accepting(cancel.clone());
        std::thread::sleep(Duration::from_secs(1));

        let mut stream = TcpStream::connect(addr)
            .expect("`connect` should be ok");

        let ping = P2pMessage::Ping(Ping { last_block_index: 2, unix_seconds: 3, nonce: 4 });
        let buf = ping.to_message_encoded()
            .expect("`to_message_encoded` should be ok");

        stream.write_all(buf.as_ref()).expect("`write_all` should be ok");
        // stream.write_all(buf.as_ref()).expect("`write_all` should be ok");

        let recv = net_rx.blocking_recv()
            .expect("`blocking_recv` should be Some");
        assert_eq!(recv.event, Accepted);

        let recv = net_rx.blocking_recv()
            .expect("`blocking_recv` should be Some");
        assert!(matches!(recv.event, Message(_)));

        let Message(event) = recv.event else { return; };
        let mut buf = RefBuffer::from(event.as_bytes());
        let recv: P2pMessage = BinDecoder::decode_bin(&mut buf)
            .expect("`decode_bin` should be ok");
        assert!(matches!(recv, P2pMessage::Ping(_)));

        let P2pMessage::Ping(ping) = recv else { return; };
        assert_eq!(ping.last_block_index, 2);
        assert_eq!(ping.unix_seconds, 3);
        assert_eq!(ping.nonce, 4);

        let local = stream.local_addr()
            .expect("`local_addr` should be ok");
        driver.remove_net_handle(&local);

        cancel.cancel();
        std::thread::sleep(Duration::from_millis(1000));

        let recv = net_rx.blocking_recv()
            .expect("`blocking_recv` should be Some");
        assert!(matches!(recv.event, Disconnected));
    }
}