//! Discord's local IPC: a named pipe `discord-ipc-N` carrying frames of
//! `opcode: u32 LE, length: u32 LE, JSON`. Overlapped (tokio) pipe I/O, so a
//! pending read doesn't block writes on the same handle.

use std::io;

use serde_json::Value;
use tokio::io::{AsyncRead, AsyncReadExt};
use tokio::net::windows::named_pipe::{ClientOptions, NamedPipeClient};

pub const HANDSHAKE: u32 = 0;
pub const FRAME: u32 = 1;
pub const CLOSE: u32 = 2;
pub const PING: u32 = 3;
pub const PONG: u32 = 4;

/// Frames bigger than this are not something Discord sends; a bad length means a broken stream.
const MAX_FRAME: usize = 16 << 20;

/// The running client's pipe; Discord takes the first free of 0–9.
pub fn open() -> Option<NamedPipeClient> {
    (0..10).find_map(|i| {
        ClientOptions::new()
            .open(format!(r"\\.\pipe\discord-ipc-{i}"))
            .ok()
    })
}

pub fn encode(op: u32, payload: &Value) -> Vec<u8> {
    let body = payload.to_string().into_bytes();
    let mut frame = Vec::with_capacity(8 + body.len());
    frame.extend_from_slice(&op.to_le_bytes());
    frame.extend_from_slice(&(body.len() as u32).to_le_bytes());
    frame.extend_from_slice(&body);
    frame
}

pub async fn read(r: &mut (impl AsyncRead + Unpin)) -> io::Result<(u32, Value)> {
    let mut header = [0u8; 8];
    r.read_exact(&mut header).await?;
    let op = u32::from_le_bytes(header[..4].try_into().expect("4 bytes"));
    let len = u32::from_le_bytes(header[4..].try_into().expect("4 bytes")) as usize;
    if len > MAX_FRAME {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("frame of {len} bytes"),
        ));
    }
    let mut body = vec![0u8; len];
    r.read_exact(&mut body).await?;
    let v =
        serde_json::from_slice(&body).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
    Ok((op, v))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn frames_round_trip() {
        let msg = json!({ "cmd": "GET_GUILDS", "nonce": "1", "args": {} });
        let mut bytes = encode(FRAME, &msg);
        bytes.extend(encode(PING, &json!({})));
        let rt = tokio::runtime::Builder::new_current_thread()
            .build()
            .unwrap();
        rt.block_on(async {
            let mut r = bytes.as_slice();
            assert_eq!(read(&mut r).await.unwrap(), (FRAME, msg));
            assert_eq!(read(&mut r).await.unwrap().0, PING);
            assert!(read(&mut r).await.is_err());
        });
    }
}
