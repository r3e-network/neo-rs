use std::io::{self, Read};
use std::io::Cursor;
use serde_json::from_slice;
use test::Bencher;

struct ReadCloser<R: Read> {
    reader: R,
}

impl<R: Read> ReadCloser<R> {
    fn new(reader: R) -> Self {
        ReadCloser { reader }
    }
}

impl<R: Read> io::Read for ReadCloser<R> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.reader.read(buf)
    }
}

impl<R: Read> io::Write for ReadCloser<R> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.reader.read(buf)
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl<R: Read> io::Seek for ReadCloser<R> {
    fn seek(&mut self, pos: io::SeekFrom) -> io::Result<u64> {
        self.reader.seek(pos)
    }
}

impl<R: Read> io::BufRead for ReadCloser<R> {
    fn fill_buf(&mut self) -> io::Result<&[u8]> {
        self.reader.fill_buf()
    }

    fn consume(&mut self, amt: usize) {
        self.reader.consume(amt)
    }
}

impl<R: Read> io::Close for ReadCloser<R> {
    fn close(&mut self) -> io::Result<()> {
        Ok(())
    }
}

#[bench]
fn benchmark_unmarshal(b: &mut Bencher) {
    let req = br#"{"jsonrpc":"2.0", "method":"invokefunction","params":["0x50befd26fdf6e4d957c11e078b24ebce6291456f", "someMethod", [{"type": "String", "value": "50befd26fdf6e4d957c11e078b24ebce6291456f"}, {"type": "Integer", "value": "42"}, {"type": "Boolean", "value": false}]]}"#;
    b.iter(|| {
        b.iter(|| {
            let mut in_data: In = serde_json::from_slice(req).unwrap();
        });
    });

    b.iter(|| {
        let mut r = Request::new();
        r.in_data = In::default();
        let rd = Cursor::new(req);
        let mut read_closer = ReadCloser::new(rd);
        r.decode_data(&mut read_closer).unwrap();
    });
}
