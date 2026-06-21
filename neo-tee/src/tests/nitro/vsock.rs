use super::*;

#[test]
fn frame_round_trip_request() {
    let req = EnclaveRequest::SignBlock {
        sign_data: vec![1, 2, 3, 4, 5],
        script_hash: [0xAB; 20],
    };
    let frame = encode_frame(&req).unwrap();
    // 4-byte prefix + non-empty body.
    assert!(frame.len() > 4);
    let advertised = u32::from_be_bytes([frame[0], frame[1], frame[2], frame[3]]) as usize;
    assert_eq!(advertised, frame.len() - 4);

    let (decoded, consumed): (EnclaveRequest, usize) = decode_frame(&frame).unwrap();
    assert_eq!(decoded, req);
    assert_eq!(consumed, frame.len());
}

#[test]
fn frame_round_trip_response() {
    let resp = EnclaveResponse::Signature(vec![9u8; 64]);
    let frame = encode_frame(&resp).unwrap();
    let (decoded, consumed): (EnclaveResponse, usize) = decode_frame(&frame).unwrap();
    assert_eq!(decoded, resp);
    assert_eq!(consumed, frame.len());
}

#[test]
fn decode_rejects_short_buffer() {
    assert!(decode_frame::<EnclaveRequest>(&[0, 0]).is_err());
}

#[test]
fn decode_rejects_truncated_body() {
    // Advertise 100 bytes but only supply 4.
    let mut buf = (100u32).to_be_bytes().to_vec();
    buf.extend_from_slice(&[0u8; 4]);
    assert!(decode_frame::<EnclaveRequest>(&buf).is_err());
}

#[test]
fn decode_rejects_oversize_length() {
    let buf = ((MAX_FRAME_LEN as u32) + 1).to_be_bytes().to_vec();
    assert!(decode_frame::<EnclaveRequest>(&buf).is_err());
}

#[test]
fn mock_transport_returns_handler_response() {
    let transport = MockTransport::new(|req| match req {
        EnclaveRequest::GetPublicKey => Ok(EnclaveResponse::PublicKey {
            public_key: vec![0x02; 33],
            script_hash: [0x11; 20],
        }),
        _ => Ok(EnclaveResponse::Error {
            message: "unexpected".to_string(),
        }),
    });

    let resp = transport.request(&EnclaveRequest::GetPublicKey).unwrap();
    match resp {
        EnclaveResponse::PublicKey {
            public_key,
            script_hash,
        } => {
            assert_eq!(public_key, vec![0x02; 33]);
            assert_eq!(script_hash, [0x11; 20]);
        }
        other => panic!("unexpected response: {other:?}"),
    }
}

#[test]
fn mock_transport_with_framing_round_trips() {
    let transport = MockTransport::with_framing(|req| {
        assert!(matches!(req, EnclaveRequest::GetPublicKey));
        Ok(EnclaveResponse::Signature(vec![7u8; 64]))
    });
    let resp = transport.request(&EnclaveRequest::GetPublicKey).unwrap();
    assert_eq!(resp, EnclaveResponse::Signature(vec![7u8; 64]));
}

#[test]
fn real_vsock_transport_is_experimental_stub() {
    let transport = RealVsockTransport::new(VsockAddr {
        cid: 16,
        port: 5005,
    });
    let err = transport
        .request(&EnclaveRequest::GetPublicKey)
        .unwrap_err();
    assert!(matches!(err, TeeError::FeatureNotEnabled(_)));
}
