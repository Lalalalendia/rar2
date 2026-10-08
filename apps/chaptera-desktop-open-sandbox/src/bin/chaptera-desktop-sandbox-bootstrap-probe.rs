#![forbid(unsafe_code)]

use std::io::Write;

fn main() {
    let mut stdout = std::io::stdout();
    stdout
        .write_all(b"CHAPTERA_SANDBOX_BOOTSTRAP_OK\n")
        .expect("write bootstrap marker");
    stdout.flush().expect("flush bootstrap marker");
}
