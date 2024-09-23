use super::*;
use crate::testserdes;
use crate::core::block;
use crate::core::transaction;
use anyhow::Result;

#[test]
fn test_headers_encode_decode() -> Result<()> {
    // normal case
    {
        let headers = new_test_headers(3);
        test_headers_encode_decode(&headers, 3, None)?;
    }

    // more than max
    {
        const SENT: usize = MaxHeadersAllowed + 1;
        let headers = new_test_headers(SENT);
        test_headers_encode_decode(&headers, MaxHeadersAllowed, Some(ErrTooManyHeaders))?;
    }

    // zero
    {
        let headers = new_test_headers(0);
        test_headers_encode_decode(&headers, 0, Some(ErrNoHeaders))?;
    }

    Ok(())
}

fn new_test_headers(n: usize) -> Headers {
    let mut headers = Headers { hdrs: vec![block::Header::default(); n] };

    for (i, header) in headers.hdrs.iter_mut().enumerate() {
        header.index = (i + 1) as u32;
        header.script = transaction::Witness {
            invocation_script: vec![0x0],
            verification_script: vec![0x1],
        };
    }

    headers
}

fn test_headers_encode_decode(headers: &Headers, expected: usize, ret_err: Option<anyhow::Error>) -> Result<()> {
    let data = testserdes::encode_binary(headers)?;
    let mut headers_decode = Headers::default();
    let r_err = testserdes::decode_binary(&data, &mut headers_decode);

    assert_eq!(ret_err, r_err.err());
    assert_eq!(expected, headers_decode.hdrs.len());

    for (i, header) in headers_decode.hdrs.iter().enumerate() {
        assert_eq!(headers.hdrs[i].version, header.version);
        assert_eq!(headers.hdrs[i].index, header.index);
        assert_eq!(headers.hdrs[i].script, header.script);
    }

    Ok(())
}

//TODO NEO3.0: Update binary
/*
#[test]
fn test_bin_encode_decode() -> Result<()> {
    let raw_block_headers = "010000000026b3c3df4dc1602a3b0e6989248b23275b5e4014a159af5dce69e16d4ab75f00f439321a51f425a530820cfe4d715bfd835b49687e87772f2c4737b8bc586dca7fda03580a000000bf14ff160228f0c059e75d652b5d3827bf04c165bbe9ef95cca4bf5501fd45014036fdd23248880c1c311bcd97df04fe6d740dc1bf340c26915f0466e31e81c039012eca7a760270389e04b58b99820fe49cf8c24c9afc65d696b4d3f406a1e6b5405172a9b461e68dd399c8716de11d31f7dd2ec3be327c636b024562db6ac5df1cffdbee74c994736fd49803234d2baffbc0054f28ba5ec76494a467b4106955bb4084af7746d269241628c667003e9d39288b190ad5cef218ada625cbba8be411bb153828d8d3634e8f586638e2448425bc5b671be69800392ccbdebc945a5099c7406f6a11824105ecad345e525957053e77fbc0119d6b3fa7f854527e816cfce0d95dac66888e07e8990c95103d8e46124aac16f152e088520d7ec8325e3a2456f840e5b77ef0e3c410b347ccaf8a87516d10b88d436563c80712153273993afc320ec49b638225f58de464a1345e62a564b398939f96f6f4b7cf21b583609f85495af1552102486fd15702c4490a26703112a5cc1d0923fd697a33406bd5a1c00e0013b09a7021024c7b7fb6c310fccf1ba33b082519d82964ea93868d676662d4a59ad548df0e7d2102aaec38470f6aad0042c6e877cfd8087d2676b0f516fddd362801b9bd3936399e2103b209fd4f53a7170ea4444e0cb0a6bb6a53c2bd016926989cf85f9b0fba17a70c2103b8d9d5771d8f513aa0869b9cc8d50986403b78c6da36890638c3d46a5adce04a2102ca0e27697b9c248f6f16e085fd0061e26f44da85b58ee835c110caa5ec3ba5542102df48f60e8f3e01c48ff40b9b7f1310d7a8b2a193188befe1c2e3df740e89509357ae00";

    let mut header_msg = Headers::default();

    let raw_block_bytes = hex::decode(raw_block_headers)?;

    testserdes::decode_binary(&raw_block_bytes, &mut header_msg)?;
    assert_eq!(1, header_msg.hdrs.len());

    let header = &header_msg.hdrs[0];
    let hash = header.hash();

    assert_eq!("f3c4ec44c07eccbda974f1ee34bc6654ab6d3f22cd89c2e5c593a16d6cc7e6e8", hash.to_string());

    let data = testserdes::encode_binary(&header_msg)?;
    assert_eq!(hex::encode(&raw_block_bytes), hex::encode(&data));

    Ok(())
}
*/
