mod dispatch;

use std::os::unix::fs::PermissionsExt;
use std::os::unix::io::AsRawFd;

use common::client::DEFAULT_SOCKET;
use common::protocol::{Request, Response};
use serde::Deserialize;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixListener;

// ---------------------------------------------------------------------------
// Daemon config (optional [daemon] section)
// ---------------------------------------------------------------------------

#[derive(Deserialize, Default)]
struct DaemonConfig {
    socket_path: Option<String>,
    /// UIDs allowed to connect. If absent or empty, all local users may connect.
    allowed_uids: Option<Vec<u32>>,
}

// ---------------------------------------------------------------------------
// Peer UID
// ---------------------------------------------------------------------------

#[cfg(target_os = "macos")]
fn peer_uid(fd: std::os::unix::io::RawFd) -> Option<u32> {
    let mut uid: libc::uid_t = 0;
    let mut gid: libc::gid_t = 0;
    let ret = unsafe { libc::getpeereid(fd, &mut uid, &mut gid) };
    if ret == 0 { Some(uid) } else { None }
}

#[cfg(target_os = "linux")]
fn peer_uid(fd: std::os::unix::io::RawFd) -> Option<u32> {
    let mut ucred = libc::ucred { pid: 0, uid: 0, gid: 0 };
    let mut len = std::mem::size_of::<libc::ucred>() as libc::socklen_t;
    let ret = unsafe {
        libc::getsockopt(
            fd,
            libc::SOL_SOCKET,
            libc::SO_PEERCRED,
            &mut ucred as *mut _ as *mut libc::c_void,
            &mut len,
        )
    };
    if ret == 0 { Some(ucred.uid) } else { None }
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
fn peer_uid(_fd: std::os::unix::io::RawFd) -> Option<u32> {
    None
}

// ---------------------------------------------------------------------------
// Connection handler
// ---------------------------------------------------------------------------

async fn handle_connection(
    mut stream: tokio::net::UnixStream,
    allowed_uids: Option<Vec<u32>>,
) {
    // Peer UID check — fail closed if UID unavailable or not in allowlist.
    let fd = stream.as_raw_fd();
    match peer_uid(fd) {
        Some(uid) => {
            if let Some(ref list) = allowed_uids {
                if !list.is_empty() && !list.contains(&uid) {
                    let resp = Response::err(format!("UID {uid} not permitted"));
                    let _ = write_response(&mut stream, &resp).await;
                    return;
                }
            }
        }
        None => {
            let resp = Response::err("could not determine peer UID");
            let _ = write_response(&mut stream, &resp).await;
            return;
        }
    }

    let (reader_half, mut writer_half) = stream.into_split();
    let mut reader = BufReader::new(reader_half);
    let mut line = String::new();

    match reader.read_line(&mut line).await {
        Ok(0) => return, // EOF
        Ok(n) if n > 1 << 20 => {
            let resp = Response::err("request too large (> 1 MiB)");
            let _ = write_response_half(&mut writer_half, &resp).await;
            return;
        }
        Ok(_) => {}
        Err(e) => {
            eprintln!("toolkit-daemon: read error: {e}");
            return;
        }
    }

    let resp = match serde_json::from_str::<Request>(line.trim()) {
        Ok(req) => dispatch::dispatch(req).await,
        Err(e) => Response::err(format!("invalid request: {e}")),
    };

    let _ = write_response_half(&mut writer_half, &resp).await;
}

async fn write_response(
    stream: &mut tokio::net::UnixStream,
    resp: &Response,
) -> std::io::Result<()> {
    let mut line = serde_json::to_string(resp).unwrap();
    line.push('\n');
    stream.write_all(line.as_bytes()).await
}

async fn write_response_half(
    writer: &mut tokio::net::unix::OwnedWriteHalf,
    resp: &Response,
) -> std::io::Result<()> {
    let mut line = serde_json::to_string(resp).unwrap();
    line.push('\n');
    writer.write_all(line.as_bytes()).await
}

// ---------------------------------------------------------------------------
// Socket lifecycle
// ---------------------------------------------------------------------------

fn cleanup_stale_socket(path: &str) {
    // Try connecting; if we succeed the daemon is already running.
    if std::os::unix::net::UnixStream::connect(path).is_ok() {
        eprintln!("toolkit-daemon: another daemon is already listening on {path}; exiting.");
        std::process::exit(1);
    }
    // Verify it is actually a socket before removing — never unlink other file types.
    if let Ok(meta) = std::fs::symlink_metadata(path) {
        use std::os::unix::fs::FileTypeExt;
        if meta.file_type().is_socket() {
            let _ = std::fs::remove_file(path);
        } else {
            eprintln!("toolkit-daemon: {path} exists but is not a socket; refusing to remove it.");
            std::process::exit(1);
        }
    }
}

// ---------------------------------------------------------------------------
// Main
// ---------------------------------------------------------------------------

#[tokio::main]
async fn main() {
    let daemon_config: DaemonConfig =
        common::load_section("daemon").unwrap_or_default();

    let socket_path = daemon_config
        .socket_path
        .unwrap_or_else(|| {
            std::env::var("TOOLKIT_SOCKET").unwrap_or_else(|_| DEFAULT_SOCKET.to_owned())
        });

    cleanup_stale_socket(&socket_path);

    let listener = UnixListener::bind(&socket_path).unwrap_or_else(|e| {
        eprintln!("toolkit-daemon: failed to bind {socket_path}: {e}");
        std::process::exit(1);
    });

    // Socket readable by all — peer UID check is the access control boundary.
    let _ = std::fs::set_permissions(&socket_path, std::fs::Permissions::from_mode(0o666));

    eprintln!("toolkit-daemon: listening on {socket_path}");

    let allowed_uids = daemon_config.allowed_uids;

    loop {
        match listener.accept().await {
            Ok((stream, _)) => {
                let uids = allowed_uids.clone();
                tokio::spawn(async move {
                    handle_connection(stream, uids).await;
                });
            }
            Err(e) => {
                eprintln!("toolkit-daemon: accept error: {e}");
            }
        }
    }
}
