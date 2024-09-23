use std::io::{self, Read, Write};
use std::error::Error;
use std::fmt;

use assert2::{assert, check};
use bytes::{Buf, BufMut, BytesMut};

#[derive(Debug)]
struct BadRW;

impl Write for BadRW {
    fn write(&mut self, _: &[u8]) -> io::Result<usize> {
        Err(io::Error::new(io::ErrorKind::Other, "it always fails"))
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl Read for BadRW {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.write(buf)
    }
}

#[test]
fn test_write_u64_le() {
    let val: u64 = 0xbadc0de15a11dead;
    let bin = vec![0xad, 0xde, 0x11, 0x5a, 0xe1, 0x0d, 0xdc, 0xba];
    let mut bw = BufBinWriter::new();
    bw.write_u64_le(val);
    assert!(bw.err.is_none());
    let wrotebin = bw.bytes();
    assert!(wrotebin == bin);
    let mut br = BinReader::from_buf(&bin);
    let readval = br.read_u64_le();
    assert!(br.err.is_none());
    assert!(val == readval);
}

#[test]
fn test_write_u32_le() {
    let val: u32 = 0xdeadbeef;
    let bin = vec![0xef, 0xbe, 0xad, 0xde];
    let mut bw = BufBinWriter::new();
    bw.write_u32_le(val);
    assert!(bw.err.is_none());
    let wrotebin = bw.bytes();
    assert!(wrotebin == bin);
    let mut br = BinReader::from_buf(&bin);
    let readval = br.read_u32_le();
    assert!(br.err.is_none());
    assert!(val == readval);
}

#[test]
fn test_write_u16_le() {
    let val: u16 = 0xbabe;
    let bin = vec![0xbe, 0xba];
    let mut bw = BufBinWriter::new();
    bw.write_u16_le(val);
    assert!(bw.err.is_none());
    let wrotebin = bw.bytes();
    assert!(wrotebin == bin);
    let mut br = BinReader::from_buf(&bin);
    let readval = br.read_u16_le();
    assert!(br.err.is_none());
    assert!(val == readval);
}

#[test]
fn test_write_u16_be() {
    let val: u16 = 0xbabe;
    let bin = vec![0xba, 0xbe];
    let mut bw = BufBinWriter::new();
    bw.write_u16_be(val);
    assert!(bw.err.is_none());
    let wrotebin = bw.bytes();
    assert!(wrotebin == bin);
    let mut br = BinReader::from_buf(&bin);
    let readval = br.read_u16_be();
    assert!(br.err.is_none());
    assert!(val == readval);
}

#[test]
fn test_write_byte() {
    let val: u8 = 0xa5;
    let bin = vec![0xa5];
    let mut bw = BufBinWriter::new();
    bw.write_u8(val);
    assert!(bw.err.is_none());
    let wrotebin = bw.bytes();
    assert!(wrotebin == bin);
    let mut br = BinReader::from_buf(&bin);
    let readval = br.read_u8();
    assert!(br.err.is_none());
    assert!(val == readval);
}

#[test]
fn test_write_bool() {
    let bin = vec![0x01, 0x00];
    let mut bw = BufBinWriter::new();
    bw.write_bool(true);
    bw.write_bool(false);
    assert!(bw.err.is_none());
    let wrotebin = bw.bytes();
    assert!(wrotebin == bin);
    let mut br = BinReader::from_buf(&bin);
    assert!(br.read_bool());
    assert!(!br.read_bool());
    assert!(br.err.is_none());
}

#[test]
fn test_read_le_errors() {
    let bin = vec![0xad, 0xde, 0x11, 0x5a, 0xe1, 0x0d, 0xdc, 0xba];
    let mut br = BinReader::from_buf(&bin);
    // Prime the buffers with something.
    let _ = br.read_u64_le();
    assert!(br.err.is_none());

    assert!(br.read_u64_le() == 0);
    assert!(br.read_u32_le() == 0);
    assert!(br.read_u16_le() == 0);
    assert!(br.read_u16_be() == 0);
    assert!(br.read_u8() == 0);
    assert!(!br.read_bool());
    assert!(br.err.is_some());
}

#[test]
fn test_buf_bin_writer_len() {
    let val = vec![0xde];
    let mut bw = BufBinWriter::new();
    bw.write_bytes(&val);
    assert!(bw.len() == 1);
}

#[test]
fn test_bin_reader_read_var_bytes() {
    let mut buf = vec![0; 11];
    for (i, b) in buf.iter_mut().enumerate() {
        *b = i as u8;
    }
    let mut w = BufBinWriter::new();
    w.write_var_bytes(&buf);
    assert!(w.err.is_none());
    let data = w.bytes();

    {
        let mut r = BinReader::from_buf(&data);
        let actual = r.read_var_bytes(None);
        assert!(r.err.is_none());
        assert!(buf == actual);
    }
    {
        let mut r = BinReader::from_buf(&data);
        let actual = r.read_var_bytes(Some(11));
        assert!(r.err.is_none());
        assert!(buf == actual);
    }
    {
        let mut r = BinReader::from_buf(&data);
        r.read_var_bytes(Some(10));
        assert!(r.err.is_some());
    }
}

#[test]
fn test_writer_err_handling() {
    let mut badio = BadRW;
    let mut bw = BinWriter::from_io(&mut badio);
    bw.write_u32_le(0);
    assert!(bw.err.is_some());
    // these should work (without panic), preserving the Err
    bw.write_u32_le(0);
    bw.write_u16_be(0);
    bw.write_var_uint(0);
    bw.write_var_bytes(&[0x55, 0xaa]);
    bw.write_string("neo");
    assert!(bw.err.is_some());
}

#[test]
fn test_reader_err_handling() {
    let mut badio = BadRW;
    let mut br = BinReader::from_io(&mut badio);
    br.read_u32_le();
    assert!(br.err.is_some());
    // these should work (without panic), preserving the Err
    br.read_u32_le();
    br.read_u16_be();
    let val = br.read_var_uint();
    assert!(val == 0);
    let b = br.read_var_bytes(None);
    assert!(b.is_empty());
    let s = br.read_string();
    assert!(s.is_empty());
    assert!(br.err.is_some());
}

#[test]
fn test_buf_bin_writer_err() {
    let mut bw = BufBinWriter::new();
    bw.write_u32_le(0);
    assert!(bw.err.is_none());
    // inject error
    bw.err = Some(Box::new(io::Error::new(io::ErrorKind::Other, "oopsie")));
    let res = bw.bytes();
    assert!(bw.err.is_some());
    assert!(res.is_empty());
}

#[test]
fn test_buf_bin_writer_reset() {
    let mut bw = BufBinWriter::new();
    for i in 0..3 {
        bw.write_u32_le(i);
        assert!(bw.err.is_none());
        let _ = bw.bytes();
        assert!(bw.err.is_some());
        bw.reset();
        assert!(bw.err.is_none());
    }
}

#[test]
fn test_write_string() {
    let str = "teststring";
    let mut bw = BufBinWriter::new();
    bw.write_string(str);
    assert!(bw.err.is_none());
    let wrotebin = bw.bytes();
    // +1 byte for length
    assert!(wrotebin.len() == str.len() + 1);
    let mut br = BinReader::from_buf(&wrotebin);
    let readstr = br.read_string();
    assert!(br.err.is_none());
    assert!(str == readstr);
}

#[test]
fn test_write_var_uint1() {
    let val = 1u64;
    let mut bw = BufBinWriter::new();
    bw.write_var_uint(val);
    assert!(bw.err.is_none());
    let buf = bw.bytes();
    assert!(buf.len() == 1);
    let mut br = BinReader::from_buf(&buf);
    let res = br.read_var_uint();
    assert!(br.err.is_none());
    assert!(val == res);
}

#[test]
fn test_write_var_uint1000() {
    let val = 1000u64;
    let mut bw = BufBinWriter::new();
    bw.write_var_uint(val);
    assert!(bw.err.is_none());
    let buf = bw.bytes();
    assert!(buf.len() == 3);
    assert!(buf[0] == 0xfd);
    let mut br = BinReader::from_buf(&buf);
    let res = br.read_var_uint();
    assert!(br.err.is_none());
    assert!(val == res);
}

#[test]
fn test_write_var_uint100000() {
    let val = 100000u64;
    let mut bw = BufBinWriter::new();
    bw.write_var_uint(val);
    assert!(bw.err.is_none());
    let buf = bw.bytes();
    assert!(buf.len() == 5);
    assert!(buf[0] == 0xfe);
    let mut br = BinReader::from_buf(&buf);
    let res = br.read_var_uint();
    assert!(br.err.is_none());
    assert!(val == res);
}

#[test]
fn test_write_var_uint100000000000() {
    let val = 1000000000000u64;
    let mut bw = BufBinWriter::new();
    bw.write_var_uint(val);
    assert!(bw.err.is_none());
    let buf = bw.bytes();
    assert!(buf.len() == 9);
    assert!(buf[0] == 0xff);
    let mut br = BinReader::from_buf(&buf);
    let res = br.read_var_uint();
    assert!(br.err.is_none());
    assert!(val == res);
}

#[test]
fn test_write_bytes() {
    let bin = vec![0xde, 0xad, 0xbe, 0xef];
    let mut bw = BufBinWriter::new();
    bw.write_bytes(&bin);
    assert!(bw.err.is_none());
    let buf = bw.bytes();
    assert!(buf.len() == 4);
    assert!(buf[0] == 0xde);

    let mut bw = BufBinWriter::new();
    bw.err = Some(Box::new(io::Error::new(io::ErrorKind::Other, "smth bad")));
    bw.write_bytes(&bin);
    assert!(bw.len() == 0);
}

#[derive(Debug, PartialEq)]
struct TestSerializable(u16);

impl Serializable for TestSerializable {
    fn encode_binary(&self, w: &mut BinWriter) {
        w.write_u16_le(self.0);
    }

    fn decode_binary(&mut self, r: &mut BinReader) {
        self.0 = r.read_u16_le();
    }
}

#[derive(Debug, PartialEq)]
struct TestPtrSerializable(u16);

impl Serializable for TestPtrSerializable {
    fn encode_binary(&self, w: &mut BinWriter) {
        w.write_u16_le(self.0);
    }

    fn decode_binary(&mut self, r: &mut BinReader) {
        self.0 = r.read_u16_le();
    }
}

#[test]
fn test_bin_writer_write_array() {
    let mut arr = [TestSerializable(0), TestSerializable(1), TestSerializable(2)];

    let expected = vec![3, 0, 0, 1, 0, 2, 0];

    let mut w = BufBinWriter::new();
    w.write_array(&arr);
    assert!(w.err.is_none());
    assert!(w.bytes() == expected);

    w.reset();
    w.write_array(&arr[..]);
    assert!(w.err.is_none());
    assert!(w.bytes() == expected);

    let mut arr_s: Vec<Box<dyn Serializable>> = arr.iter().map(|&x| Box::new(x) as Box<dyn Serializable>).collect();

    w.reset();
    w.write_array(&arr_s);
    assert!(w.err.is_none());
    assert!(w.bytes() == expected);

    w.reset();
    assert!(std::panic::catch_unwind(|| w.write_array(&[1])).is_err());

    w.reset();
    w.err = Some(Box::new(io::Error::new(io::ErrorKind::Other, "error")));
    w.write_array(&arr[..]);
    assert!(w.err.is_some());
    assert!(w.bytes().is_empty());

    w.reset();
    assert!(std::panic::catch_unwind(|| w.write_array(&[1])).is_err());

    w.reset();
    w.err = Some(Box::new(io::Error::new(io::ErrorKind::Other, "error")));
    assert!(std::panic::catch_unwind(|| w.write_array(&[1])).is_err());

    // Ptr receiver test
    let mut arr_ptr = [TestPtrSerializable(0), TestPtrSerializable(1), TestPtrSerializable(2)];
    w.reset();
    w.write_array(&arr_ptr[..]);
    assert!(w.err.is_none());
    assert!(w.bytes() == expected);
}

#[test]
fn test_bin_reader_read_array() {
    let data = vec![3, 0, 0, 1, 0, 2, 0];
    let elems = vec![TestSerializable(0), TestSerializable(1), TestSerializable(2)];

    let mut r = BinReader::from_buf(&data);
    let mut arr_ptr: Vec<TestSerializable> = Vec::new();
    r.read_array(&mut arr_ptr);
    assert!(arr_ptr == elems);

    let mut r = BinReader::from_buf(&data);
    let mut arr_val: Vec<TestSerializable> = Vec::new();
    r.read_array(&mut arr_val);
    assert!(r.err.is_none());
    assert!(arr_val == elems);

    let mut r = BinReader::from_buf(&data);
    let mut arr_val: Vec<TestSerializable> = Vec::new();
    r.read_array(&mut arr_val, Some(3));
    assert!(r.err.is_none());
    assert!(arr_val == elems);

    let mut r = BinReader::from_buf(&data);
    let mut arr_val: Vec<TestSerializable> = Vec::new();
    r.read_array(&mut arr_val, Some(2));
    assert!(r.err.is_some());

    let mut r = BinReader::from_buf(&[0]);
    let mut arr_val: Vec<TestSerializable> = Vec::new();
    r.read_array(&mut arr_val);
    assert!(r.err.is_none());
    assert!(arr_val.is_empty());

    let mut r = BinReader::from_buf(&[0]);
    r.err = Some(Box::new(io::Error::new(io::ErrorKind::Other, "error")));
    let mut arr_val: Vec<TestSerializable> = Vec::new();
    r.read_array(&mut arr_val);
    assert!(r.err.is_some());
    assert!(arr_val.is_empty());

    let mut r = BinReader::from_buf(&[0]);
    r.err = Some(Box::new(io::Error::new(io::ErrorKind::Other, "error")));
    let mut arr_ptr: Vec<TestSerializable> = Vec::new();
    r.read_array(&mut arr_ptr);
    assert!(r.err.is_some());
    assert!(arr_ptr.is_empty());

    let mut r = BinReader::from_buf(&[0]);
    let mut arr_val = vec![TestSerializable(1), TestSerializable(2)];
    r.read_array(&mut arr_val);
    assert!(r.err.is_none());
    assert!(arr_val.is_empty());

    let mut r = BinReader::from_buf(&[1]);
    assert!(std::panic::catch_unwind(|| r.read_array(&mut vec![1])).is_err());

    let mut r = BinReader::from_buf(&[0]);
    r.err = Some(Box::new(io::Error::new(io::ErrorKind::Other, "error")));
    assert!(std::panic::catch_unwind(|| r.read_array(&mut vec![1])).is_err());
}

#[test]
fn test_bin_reader_read_bytes() {
    let data = vec![0, 1, 2, 3, 4, 5, 6, 7];
    let mut r = BinReader::from_buf(&data);

    let mut buf = vec![0; 4];
    r.read_bytes(&mut buf);
    assert!(r.err.is_none());
    assert!(buf == data[..4]);

    r.read_bytes(&mut vec![]);
    assert!(r.err.is_none());

    let mut buf = vec![0; 3];
    r.read_bytes(&mut buf);
    assert!(r.err.is_none());
    assert!(buf == data[4..7]);

    let mut buf = vec![0; 2];
    r.read_bytes(&mut buf);
    assert!(r.err.is_some());

    r.read_bytes(&mut vec![]);
    assert!(r.err.is_some());
}
