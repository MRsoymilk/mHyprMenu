use std::{
    env, fs,
    io::{Read, Write},
    os::unix::{
        fs::PermissionsExt,
        net::{UnixListener, UnixStream},
    },
    path::PathBuf,
};

use anyhow::{Context, Result, bail};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Request {
    Popup,
    Reload,
    Status,
    Quit,
}

impl Request {
    pub fn as_bytes(self) -> &'static [u8] {
        match self {
            Self::Popup => b"popup\n",
            Self::Reload => b"reload\n",
            Self::Status => b"status\n",
            Self::Quit => b"quit\n",
        }
    }

    pub fn parse(bytes: &[u8]) -> Option<Self> {
        match std::str::from_utf8(bytes).ok()?.trim() {
            "popup" => Some(Self::Popup),
            "reload" => Some(Self::Reload),
            "status" => Some(Self::Status),
            "quit" => Some(Self::Quit),
            _ => None,
        }
    }
}

pub fn socket_path() -> Result<PathBuf> {
    let runtime_dir = env::var_os("XDG_RUNTIME_DIR").context("XDG_RUNTIME_DIR is not set")?;
    Ok(PathBuf::from(runtime_dir).join("mhyprmenu.sock"))
}

pub fn send(request: Request) -> Result<()> {
    let path = socket_path()?;
    let mut stream = UnixStream::connect(&path)
        .with_context(|| format!("failed to connect to {}", path.display()))?;
    stream
        .write_all(request.as_bytes())
        .with_context(|| format!("failed to write to {}", path.display()))
}

pub fn request_status() -> Result<String> {
    let path = socket_path()?;
    let mut stream = UnixStream::connect(&path)
        .with_context(|| format!("failed to connect to {}", path.display()))?;
    stream
        .write_all(Request::Status.as_bytes())
        .with_context(|| format!("failed to write to {}", path.display()))?;

    let mut response = String::new();
    stream
        .read_to_string(&mut response)
        .with_context(|| format!("failed to read from {}", path.display()))?;
    Ok(response)
}

pub fn write_response(stream: &mut UnixStream, response: &str) -> Result<()> {
    stream
        .write_all(response.as_bytes())
        .context("failed to write daemon response")
}

pub fn bind_listener() -> Result<(UnixListener, SocketGuard)> {
    let path = socket_path()?;

    if path.exists() {
        if UnixStream::connect(&path).is_ok() {
            bail!("mhyprmenu daemon is already running");
        }

        fs::remove_file(&path)
            .with_context(|| format!("failed to remove stale {}", path.display()))?;
    }

    let listener =
        UnixListener::bind(&path).with_context(|| format!("failed to bind {}", path.display()))?;
    listener
        .set_nonblocking(true)
        .context("failed to make daemon socket nonblocking")?;
    fs::set_permissions(&path, fs::Permissions::from_mode(0o600))
        .with_context(|| format!("failed to set permissions on {}", path.display()))?;

    Ok((listener, SocketGuard(path)))
}

pub fn read_request(stream: &mut UnixStream) -> Result<Option<Request>> {
    let mut buffer = [0_u8; 64];
    let size = stream
        .read(&mut buffer)
        .context("failed to read daemon request")?;
    Ok(Request::parse(&buffer[..size]))
}

pub struct SocketGuard(PathBuf);

impl Drop for SocketGuard {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}
