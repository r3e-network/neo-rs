// Type represents node capability type.
#[repr(u8)]
enum Type {
    // TCPServer represents TCP node capability type.
    TCPServer = 0x01,
    // WSServer represents WebSocket node capability type.
    WSServer = 0x02,
    // FullNode represents full node capability type.
    FullNode = 0x10,
}
