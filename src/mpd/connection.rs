//! Blocking TCP connection with MPD line + binary framing
//! (mirror of MeloCore's `MPDConnection`).

use std::fmt;
use std::io::{self, BufRead, BufReader, Read, Write};
use std::net::{Shutdown, TcpStream, ToSocketAddrs};
use std::time::Duration;

#[derive(Debug)]
#[allow(dead_code)]
pub enum MpdError {
    Io(io::Error),
    /// `ACK [code@index] {command} message`
    Server {
        code: i32,
        command: String,
        message: String,
    },
    Protocol(String),
    Disconnected,
    /// The server has no picture for the requested URI.
    NoArt,
}

impl MpdError {
    /// Errors that mean the socket is dead and a reconnect is warranted.
    pub fn is_connection_error(&self) -> bool {
        matches!(self, MpdError::Io(_) | MpdError::Disconnected)
    }
}

impl fmt::Display for MpdError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            MpdError::Io(e) => write!(f, "{e}"),
            MpdError::Server {
                message, command, ..
            } => {
                if command.is_empty() {
                    write!(f, "{message}")
                } else {
                    write!(f, "{command}: {message}")
                }
            }
            MpdError::Protocol(m) => write!(f, "protocol error: {m}"),
            MpdError::Disconnected => write!(f, "disconnected"),
            MpdError::NoArt => write!(f, "no cover art"),
        }
    }
}

impl From<io::Error> for MpdError {
    fn from(e: io::Error) -> Self {
        if e.kind() == io::ErrorKind::UnexpectedEof {
            MpdError::Disconnected
        } else {
            MpdError::Io(e)
        }
    }
}

pub struct Connection {
    reader: BufReader<TcpStream>,
    writer: TcpStream,
    pub server_version: String,
}

/// A cloneable handle that lets another thread tear the socket down, which
/// unblocks any read in progress on the owning thread.
pub struct Killer(TcpStream);

impl Killer {
    pub fn kill(&self) {
        let _ = self.0.shutdown(Shutdown::Both);
    }
}

impl Connection {
    /// Opens the socket, consumes the `OK MPD x.y.z` greeting and authenticates
    /// when a password is given.
    pub fn open(
        host: &str,
        port: u16,
        password: Option<&str>,
        read_timeout: Option<Duration>,
    ) -> Result<(Self, Killer), MpdError> {
        let addrs: Vec<_> = (host, port).to_socket_addrs()?.collect();
        let mut last_err = io::Error::new(io::ErrorKind::NotFound, "no addresses resolved");
        let mut stream = None;
        for addr in addrs {
            match TcpStream::connect_timeout(&addr, Duration::from_secs(5)) {
                Ok(s) => {
                    stream = Some(s);
                    break;
                }
                Err(e) => last_err = e,
            }
        }
        let stream = stream.ok_or(MpdError::Io(last_err))?;
        stream.set_nodelay(true)?;
        stream.set_read_timeout(read_timeout)?;
        let killer = Killer(stream.try_clone()?);
        let writer = stream.try_clone()?;
        let mut conn = Connection {
            reader: BufReader::new(stream),
            writer,
            server_version: String::new(),
        };
        let greeting = conn.read_line()?;
        if let Some(v) = greeting.strip_prefix("OK MPD ") {
            conn.server_version = v.trim().to_owned();
        } else {
            return Err(MpdError::Protocol(format!(
                "unexpected greeting: {greeting}"
            )));
        }
        if let Some(pw) = password.filter(|p| !p.is_empty()) {
            conn.command(&super::protocol::password(pw))?;
        }
        Ok((conn, killer))
    }

    pub fn set_read_timeout(&self, timeout: Option<Duration>) -> io::Result<()> {
        self.writer.set_read_timeout(timeout)
    }

    /// Sends a text command and returns the response lines (without the
    /// terminating `OK`). `ACK` becomes `MpdError::Server`.
    pub fn command(&mut self, cmd: &str) -> Result<Vec<String>, MpdError> {
        self.send(cmd)?;
        self.read_text_response()
    }

    /// Sends a binary command (`albumart` / `readpicture`) and returns
    /// `(total_size, chunk)`. Returns `MpdError::Server` when MPD reports no art.
    pub fn binary_command(&mut self, cmd: &str) -> Result<(usize, Vec<u8>), MpdError> {
        self.send(cmd)?;
        let mut size: Option<usize> = None;
        let binary: Option<usize>;
        loop {
            let line = self.read_line()?;
            if line == "OK" {
                // Some servers answer `size: N` + OK with no binary section
                // when the offset is at/after the end.
                return Ok((size.unwrap_or(0), Vec::new()));
            }
            if line.starts_with("ACK ") {
                return Err(parse_ack(&line));
            }
            if let Some((k, v)) = line.split_once(':') {
                match k.trim() {
                    "size" => size = v.trim().parse().ok(),
                    "binary" => {
                        binary = v.trim().parse().ok();
                        break;
                    }
                    _ => {}
                }
            }
        }
        let count = binary.ok_or_else(|| MpdError::Protocol("missing binary header".into()))?;
        let mut chunk = vec![0u8; count];
        self.reader.read_exact(&mut chunk)?;
        // Newline after the binary blob, then the OK line.
        let _ = self.read_line()?;
        let tail = self.read_line()?;
        if tail != "OK" {
            if tail.starts_with("ACK ") {
                return Err(parse_ack(&tail));
            }
            return Err(MpdError::Protocol(format!(
                "expected OK after binary, got {tail}"
            )));
        }
        Ok((size.unwrap_or(count), chunk))
    }

    fn send(&mut self, cmd: &str) -> Result<(), MpdError> {
        self.writer.write_all(cmd.as_bytes())?;
        self.writer.flush()?;
        Ok(())
    }

    fn read_text_response(&mut self) -> Result<Vec<String>, MpdError> {
        let mut lines = Vec::new();
        loop {
            let line = self.read_line()?;
            if line == "OK" {
                return Ok(lines);
            }
            if line.starts_with("ACK ") {
                return Err(parse_ack(&line));
            }
            lines.push(line);
        }
    }

    fn read_line(&mut self) -> Result<String, MpdError> {
        let mut buf = Vec::new();
        let n = self.reader.read_until(b'\n', &mut buf)?;
        if n == 0 {
            return Err(MpdError::Disconnected);
        }
        if buf.last() == Some(&b'\n') {
            buf.pop();
        }
        Ok(String::from_utf8_lossy(&buf).into_owned())
    }
}

/// `ACK [50@0] {play} No such song`
pub fn parse_ack(line: &str) -> MpdError {
    let rest = line.trim_start_matches("ACK ").trim();
    let mut code = 0;
    let mut command = String::new();
    let mut message = rest.to_owned();
    if let Some(after_bracket) = rest.strip_prefix('[') {
        if let Some((codes, tail)) = after_bracket.split_once(']') {
            code = codes
                .split('@')
                .next()
                .and_then(|c| c.parse().ok())
                .unwrap_or(0);
            let tail = tail.trim();
            if let Some(after_brace) = tail.strip_prefix('{') {
                if let Some((cmd, msg)) = after_brace.split_once('}') {
                    command = cmd.to_owned();
                    message = msg.trim().to_owned();
                }
            } else {
                message = tail.to_owned();
            }
        }
    }
    MpdError::Server {
        code,
        command,
        message,
    }
}

/// Accumulates chunked binary responses until `total_size` bytes arrived.
/// Pure with respect to the transport (mirror of `MPDClient.assembleBinary`).
pub fn assemble_binary(
    mut fetch_chunk: impl FnMut(usize) -> Result<(usize, Vec<u8>), MpdError>,
) -> Result<Option<Vec<u8>>, MpdError> {
    let mut all = Vec::new();
    let mut offset = 0;
    let mut total: Option<usize> = None;
    loop {
        let (size, chunk) = fetch_chunk(offset)?;
        if total.is_none() {
            total = Some(size);
        }
        if size == 0 {
            return Ok(None);
        }
        // A non-zero size with an empty chunk would never advance the offset.
        if chunk.is_empty() {
            break;
        }
        offset += chunk.len();
        all.extend_from_slice(&chunk);
        if offset >= total.unwrap_or(0) {
            break;
        }
    }
    Ok(if all.is_empty() { None } else { Some(all) })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_ack() {
        match parse_ack("ACK [50@0] {play} No such song") {
            MpdError::Server {
                code,
                command,
                message,
            } => {
                assert_eq!(code, 50);
                assert_eq!(command, "play");
                assert_eq!(message, "No such song");
            }
            other => panic!("unexpected {other:?}"),
        }
    }

    #[test]
    fn assembles_chunks() {
        let data: Vec<u8> = (0..25u8).collect();
        let out = assemble_binary(|off| {
            let end = (off + 10).min(data.len());
            Ok((data.len(), data[off..end].to_vec()))
        })
        .unwrap()
        .unwrap();
        assert_eq!(out, data);
    }

    #[test]
    fn zero_size_means_no_art() {
        assert!(assemble_binary(|_| Ok((0, Vec::new()))).unwrap().is_none());
    }

    #[test]
    fn empty_chunk_with_size_terminates() {
        let mut calls = 0;
        let out = assemble_binary(|_| {
            calls += 1;
            Ok((100, Vec::new()))
        })
        .unwrap();
        assert!(out.is_none());
        assert_eq!(calls, 1);
    }
}
