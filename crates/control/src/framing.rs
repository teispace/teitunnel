//! One JSON message per line, bounded: a line longer than the limit is never buffered
//! whole.

use std::io;

use tokio::io::{AsyncBufRead, AsyncBufReadExt, AsyncWrite, AsyncWriteExt};

/// What was read.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Frame {
    /// A line (without its newline).
    Line(String),
    /// The line was longer than allowed (the rest of the stream is unusable).
    TooLarge,
}

/// Reads the next line of at most `max` bytes. `None` at the end of the stream.
pub(crate) async fn read_frame<R: AsyncBufRead + Unpin>(
    reader: &mut R,
    max: usize,
) -> io::Result<Option<Frame>> {
    let mut line = Vec::new();
    loop {
        let available = reader.fill_buf().await?;
        if available.is_empty() {
            return Ok(if line.is_empty() {
                None
            } else {
                Some(to_frame(line))
            });
        }
        let (chunk, done) = match available.iter().position(|&b| b == b'\n') {
            Some(end) => (&available[..end], Some(end + 1)),
            None => (available, None),
        };
        if line.len() + chunk.len() > max {
            return Ok(Some(Frame::TooLarge));
        }
        line.extend_from_slice(chunk);
        let used = done.unwrap_or(available.len());
        reader.consume(used);
        if done.is_some() {
            return Ok(Some(to_frame(line)));
        }
    }
}

fn to_frame(mut line: Vec<u8>) -> Frame {
    if line.last() == Some(&b'\r') {
        line.pop();
    }
    Frame::Line(String::from_utf8_lossy(&line).into_owned())
}

/// Writes one message and its newline.
pub(crate) async fn write_frame<W: AsyncWrite + Unpin>(
    writer: &mut W,
    line: &str,
) -> io::Result<()> {
    writer.write_all(line.as_bytes()).await?;
    writer.write_all(b"\n").await?;
    writer.flush().await
}

#[cfg(test)]
mod tests {
    use tokio::io::BufReader;

    use super::*;

    #[tokio::test]
    async fn reads_lines_and_refuses_long_ones() {
        let input = b"{\"a\":1}\r\n\n{\"b\":2}".to_vec();
        let mut reader = BufReader::with_capacity(4, &input[..]);
        assert_eq!(
            read_frame(&mut reader, 64).await.unwrap(),
            Some(Frame::Line("{\"a\":1}".into()))
        );
        assert_eq!(
            read_frame(&mut reader, 64).await.unwrap(),
            Some(Frame::Line(String::new()))
        );
        assert_eq!(
            read_frame(&mut reader, 64).await.unwrap(),
            Some(Frame::Line("{\"b\":2}".into()))
        );
        assert_eq!(read_frame(&mut reader, 64).await.unwrap(), None);

        let long = [b'x'; 100];
        let mut reader = BufReader::with_capacity(8, &long[..]);
        assert_eq!(
            read_frame(&mut reader, 64).await.unwrap(),
            Some(Frame::TooLarge)
        );
    }
}
