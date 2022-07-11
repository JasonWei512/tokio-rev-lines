# tokio-rev-lines

[![Crate](https://img.shields.io/crates/v/tokio-rev-lines.svg)](https://crates.io/crates/tokio-rev-lines)
[![MIT licensed](https://img.shields.io/badge/license-MIT-blue.svg)](./LICENSE)

This library provides an async stream for reading files or any `BufReader` line by line with buffering in reverse.

It's an async tokio version of [rev_lines](https://github.com/mjc-gh/rev_lines).

### Documentation

Documentation is available on [Docs.rs](https://docs.rs/tokio-rev-lines).

### Example

```rust
use futures_util::{pin_mut, StreamExt};
use tokio::{fs::File, io::BufReader};
use tokio_rev_lines::RevLines;

#[tokio::main]
async fn main() {
    let file = File::open("tests/multi_line_file").await.unwrap();
    let rev_lines = RevLines::new(BufReader::new(file)).await.unwrap();
    pin_mut!(rev_lines);
    
    while let Some(line) = rev_lines.next().await {
        println!("{}", line);
    }
}
```