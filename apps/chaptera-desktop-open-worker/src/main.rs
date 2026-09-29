#![forbid(unsafe_code)]

use std::io::{Read, Write};

fn main() {
    let max = chaptera_desktop_open_worker::request_wire_max_bytes_v1();
    let mut input = Vec::new();
    let read_result = std::io::stdin()
        .lock()
        .take(max.saturating_add(1))
        .read_to_end(&mut input);
    if read_result.is_err() || u64::try_from(input.len()).unwrap_or(u64::MAX) > max {
        eprintln!("chaptera desktop-open worker failed closed");
        std::process::exit(2);
    }

    let output = match chaptera_desktop_open_worker::process_wire_v1(&input) {
        Ok(output) => output,
        Err(_) => {
            eprintln!("chaptera desktop-open worker failed closed");
            std::process::exit(2);
        }
    };
    if std::io::stdout().lock().write_all(&output).is_err() {
        std::process::exit(2);
    }
}
