use std::io::{self, BufReader, BufWriter};

fn main() {
    let stdin = io::stdin();
    let stdout = io::stdout();
    let mut reader = BufReader::new(stdin.lock());
    let mut writer = BufWriter::new(stdout.lock());
    if let Err(error) = neo_probe::run_worker_once(&mut reader, &mut writer) {
        eprintln!("neo-probe rejected request: {error}");
        std::process::exit(1);
    }
}
