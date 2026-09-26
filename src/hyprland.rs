use std::{
    env,
    io::{Read, Write},
    net::Shutdown,
    os::unix::net::UnixStream,
    path::PathBuf,
    time::Duration,
};

use anyhow::{Context, Result};
use serde::Deserialize;

#[derive(Debug, Deserialize)]
struct CursorPos {
    x: f64,
    y: f64,
}

#[derive(Debug, Deserialize)]
struct Monitor {
    x: f64,
    y: f64,
    width: f64,
    height: f64,
    scale: f64,
    #[serde(default)]
    disabled: bool,
}

pub fn cursor_position_local() -> Result<(f64, f64)> {
    let cursor: CursorPos =
        serde_json::from_str(request("j/cursorpos")?.trim()).context("invalid cursorpos JSON")?;
    let monitors: Vec<Monitor> =
        serde_json::from_str(request("j/monitors")?.trim()).context("invalid monitors JSON")?;

    for monitor in monitors.into_iter().filter(|monitor| !monitor.disabled) {
        let scale = monitor.scale.max(0.001);
        let logical_width = monitor.width / scale;
        let logical_height = monitor.height / scale;

        if cursor.x >= monitor.x
            && cursor.x < monitor.x + logical_width
            && cursor.y >= monitor.y
            && cursor.y < monitor.y + logical_height
        {
            return Ok((cursor.x - monitor.x, cursor.y - monitor.y));
        }
    }

    Ok((cursor.x, cursor.y))
}

fn request(command: &str) -> Result<String> {
    let mut stream =
        UnixStream::connect(socket_path()?).context("failed to connect Hyprland IPC")?;
    let timeout = Some(Duration::from_millis(250));
    stream
        .set_read_timeout(timeout)
        .context("failed to set Hyprland IPC read timeout")?;
    stream
        .set_write_timeout(timeout)
        .context("failed to set Hyprland IPC write timeout")?;
    stream
        .write_all(command.as_bytes())
        .context("failed to write Hyprland IPC request")?;
    stream
        .shutdown(Shutdown::Write)
        .context("failed to finish Hyprland IPC request")?;

    let mut response = String::new();
    stream
        .read_to_string(&mut response)
        .context("failed to read Hyprland IPC response")?;
    Ok(response)
}

fn socket_path() -> Result<PathBuf> {
    let runtime_dir = env::var_os("XDG_RUNTIME_DIR").context("XDG_RUNTIME_DIR is not set")?;
    let signature = env::var_os("HYPRLAND_INSTANCE_SIGNATURE")
        .context("HYPRLAND_INSTANCE_SIGNATURE is not set")?;

    Ok(PathBuf::from(runtime_dir)
        .join("hypr")
        .join(signature)
        .join(".socket.sock"))
}
