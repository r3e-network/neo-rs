use bytes::BytesMut;
use std::collections::{HashMap, VecDeque};
use std::net::SocketAddr;
use std::time::{Duration, Instant};
use tokio::net::TcpStream;
use tokio::time::interval;

use crate::neo_system::NeoSystem;
use crate::payload::*;