//! ### RevLines
//!
//! This library provides an async stream for reading files or any `BufReader` line by line with buffering in reverse.
//! 
//! It's an async tokio version of [rev_lines](https://github.com/mjc-gh/rev_lines).
//!
//! #### Example
//!
//! ```
//!  use futures_util::{pin_mut, StreamExt};
//!  use tokio::{fs::File, io::BufReader};
//!  use tokio_rev_lines::RevLines;
//!
//!  #[tokio::main]
//!  async fn main() {
//!      let file = File::open("tests/multi_line_file").await.unwrap();
//!      let rev_lines = RevLines::new(BufReader::new(file)).await.unwrap();
//!      pin_mut!(rev_lines);
//!
//!      while let Some(line) = rev_lines.next().await {
//!          println!("{}", line);
//!      }
//!  }
//! ```
//!
//! This method uses logic borrowed from [uutils/coreutils
//! tail](https://github.com/uutils/coreutils/blob/f2166fed0ad055d363aedff6223701001af090d3/src/tail/tail.rs#L399-L402)

use futures_util::{stream, Stream};
use std::cmp::min;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncSeek, AsyncSeekExt, BufReader, Result, SeekFrom};

static DEFAULT_SIZE: usize = 4096;

static LF_BYTE: u8 = '\n' as u8;
static CR_BYTE: u8 = '\r' as u8;

/// `RevLines` struct
pub struct RevLines<R> {
    reader: BufReader<R>,
    reader_pos: u64,
    buf_size: u64,
}

impl<R: AsyncSeek + AsyncRead + Unpin> RevLines<R> {
    /// Create a `Stream<Item = String>` from a `BufReader<R>`. Internal
    /// buffering for iteration will default to 4096 bytes at a time.
    pub async fn new(reader: BufReader<R>) -> Result<impl Stream<Item = String>> {
        RevLines::with_capacity(DEFAULT_SIZE, reader).await
    }

    /// Create a `Stream<Item = String>` from a `BufReader<R>`. Internal
    /// buffering for iteration will use `cap` bytes at a time.
    pub async fn with_capacity(
        cap: usize,
        mut reader: BufReader<R>,
    ) -> Result<impl Stream<Item = String>> {
        // Seek to end of reader now
        let reader_size = reader.seek(SeekFrom::End(0)).await?;

        let mut rev_lines = RevLines {
            reader: reader,
            reader_pos: reader_size,
            buf_size: cap as u64,
        };

        // Handle any trailing new line characters for the reader
        // so the first next call does not return Some("")

        // Read at most 2 bytes
        let end_size = min(reader_size, 2);
        let end_buf = rev_lines.read_to_buffer(end_size).await?;

        if end_size == 1 {
            if end_buf[0] != LF_BYTE {
                rev_lines.move_reader_position(1).await?;
            }
        } else if end_size == 2 {
            if end_buf[0] != CR_BYTE {
                rev_lines.move_reader_position(1).await?;
            }

            if end_buf[1] != LF_BYTE {
                rev_lines.move_reader_position(1).await?;
            }
        }

        let stream = stream::unfold(rev_lines, |mut rev_lines| async {
            match rev_lines.next_line().await {
                Some(line) => Some((line, rev_lines)),
                None => None,
            }
        });

        Ok(stream)
    }

    async fn read_to_buffer(&mut self, size: u64) -> Result<Vec<u8>> {
        let mut buf = vec![0; size as usize];
        let offset = -(size as i64);

        self.reader.seek(SeekFrom::Current(offset)).await?;
        self.reader.read_exact(&mut buf[0..(size as usize)]).await?;
        self.reader.seek(SeekFrom::Current(offset)).await?;

        self.reader_pos -= size;

        Ok(buf)
    }

    async fn move_reader_position(&mut self, offset: u64) -> Result<()> {
        self.reader.seek(SeekFrom::Current(offset as i64)).await?;
        self.reader_pos += offset;

        Ok(())
    }

    async fn next_line(&mut self) -> Option<String> {
        let mut result: Vec<u8> = Vec::new();

        'outer: loop {
            if self.reader_pos < 1 {
                if result.len() > 0 {
                    break;
                }

                return None;
            }

            // Read the of minimum between the desired
            // buffer size or remaining length of the reader
            let size = min(self.buf_size, self.reader_pos);

            match self.read_to_buffer(size).await {
                Ok(buf) => {
                    for (idx, ch) in (&buf).iter().enumerate().rev() {
                        // Found a new line character to break on
                        if *ch == LF_BYTE {
                            let mut offset = idx as u64;

                            // Add an extra byte cause of CR character
                            if idx > 1 && buf[idx - 1] == CR_BYTE {
                                offset -= 1;
                            }

                            match self.reader.seek(SeekFrom::Current(offset as i64)).await {
                                Ok(_) => {
                                    self.reader_pos += offset;

                                    break 'outer;
                                }

                                Err(_) => return None,
                            }
                        } else {
                            result.push(ch.clone());
                        }
                    }
                }

                Err(_) => return None,
            }
        }

        // Reverse the results since they were written backwards
        result.reverse();

        // Convert to a String
        Some(String::from_utf8(result).unwrap())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use futures_util::{pin_mut, StreamExt};
    use tokio::fs::File;

    #[tokio::test]
    async fn it_handles_empty_files() {
        let file = File::open("tests/empty_file").await.unwrap();
        let rev_lines = RevLines::new(BufReader::new(file)).await.unwrap();
        let results = vec![];

        assert_stream_eq(rev_lines, results).await;
    }

    #[tokio::test]
    async fn it_handles_file_with_one_line() {
        let file = File::open("tests/one_line_file").await.unwrap();
        let rev_lines = RevLines::new(BufReader::new(file)).await.unwrap();
        let results = vec!["ABCD"];

        assert_stream_eq(rev_lines, results).await;
    }

    #[tokio::test]
    async fn it_handles_file_with_multi_lines() {
        let file = File::open("tests/multi_line_file").await.unwrap();
        let rev_lines = RevLines::new(BufReader::new(file)).await.unwrap();
        let results = vec!["UVWXYZ", "LMNOPQRST", "GHIJK", "ABCDEF"];

        assert_stream_eq(rev_lines, results).await;
    }

    #[tokio::test]
    async fn it_handles_file_with_blank_lines() {
        let file = File::open("tests/blank_line_file").await.unwrap();
        let rev_lines = RevLines::new(BufReader::new(file)).await.unwrap();
        let results = vec!["", "", "XYZ", "", "ABCD"];

        assert_stream_eq(rev_lines, results).await;
    }

    #[tokio::test]
    async fn it_handles_file_with_multi_lines_and_with_capacity() {
        let file = File::open("tests/multi_line_file").await.unwrap();
        let rev_lines = RevLines::with_capacity(5, BufReader::new(file))
            .await
            .unwrap();
        let results = vec!["UVWXYZ", "LMNOPQRST", "GHIJK", "ABCDEF"];

        assert_stream_eq(rev_lines, results).await;
    }

    async fn assert_stream_eq(rev_lines: impl Stream<Item = String>, results: Vec<&str>) {
        pin_mut!(rev_lines);

        for result in results {
            assert_eq!(rev_lines.next().await, Some(result.to_string()));
        }
        assert_eq!(rev_lines.next().await, None);
    }
}
