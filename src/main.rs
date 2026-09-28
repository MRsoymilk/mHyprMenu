mod config;
mod hyprland;
mod ipc;
mod menu;
mod render;
mod style;

use std::{env, io, os::unix::net::UnixListener, process::Command, time::Instant};

use anyhow::{Context, Result, bail};
use smithay_client_toolkit::{
    compositor::{CompositorHandler, CompositorState},
    delegate_registry,
    output::{OutputHandler, OutputState},
    reexports::{
        calloop::{EventLoop, Interest, Mode, PostAction, generic::Generic},
        calloop_wayland_source::WaylandSource,
    },
    registry::{ProvidesRegistryState, RegistryState},
    registry_handlers,
    seat::{
        Capability, SeatHandler, SeatState,
        pointer::{BTN_LEFT, BTN_RIGHT, PointerEvent, PointerEventKind, PointerHandler},
    },
    shell::{
        WaylandSurface,
        wlr_layer::{
            Anchor, KeyboardInteractivity, Layer, LayerShell, LayerShellHandler, LayerSurface,
            LayerSurfaceConfigure,
        },
    },
    shm::{Shm, ShmHandler, slot::SlotPool},
};
use wayland_client::{
    Connection, Dispatch, QueueHandle, WEnum,
    globals::registry_queue_init,
    protocol::{wl_keyboard, wl_output, wl_pointer, wl_seat, wl_shm, wl_surface},
};

use crate::{
    config::Config,
    ipc::Request,
    menu::{ClickOutcome, MenuState},
    render::Renderer,
    style::Style,
};

fn main() -> Result<()> {
    match env::args().nth(1).as_deref() {
        Some("--daemon") => run_daemon(),
        Some("--oneshot") => run_wayland(None, false),
        Some("--reload") => ipc::send(Request::Reload),
        Some("--status") => {
            print!("{}", ipc::request_status()?);
            Ok(())
        }
        Some("--quit") => ipc::send(Request::Quit),
        Some("-h" | "--help") => {
            print_help();
            Ok(())
        }
        Some(argument) => bail!("unknown argument: {argument}"),
        None => {
            if ipc::send(Request::Popup).is_ok() {
                Ok(())
            } else {
                run_wayland(None, false)
            }
        }
    }
}

fn format_timing(value: Option<u128>) -> String {
    value
        .map(|value| value.to_string())
        .unwrap_or_else(|| "-".to_string())
}

fn print_help() {
    println!(
        "mhyprmenu\n\
         \n\
         Usage:\n\
           mhyprmenu           show the menu via daemon, or fall back to one-shot mode\n\
           mhyprmenu --oneshot force one-shot mode (uses MHYPRMENU_CONFIG_DIR when set)\n\
           mhyprmenu --daemon  run the persistent Wayland daemon\n\
           mhyprmenu --reload  reload daemon configuration\n\
           mhyprmenu --status  show last popup timing\n\
           mhyprmenu --quit    stop the daemon"
    );
}

fn run_daemon() -> Result<()> {
    let (listener, _socket_guard) = ipc::bind_listener()?;
    run_wayland(Some(listener), true)
}

fn run_wayland(listener: Option<UnixListener>, daemon_mode: bool) -> Result<()> {
    let config = Config::load()?;
    let style = Style::load()?;

    let conn = Connection::connect_to_env().context("failed to connect to Wayland compositor")?;
    let (globals, event_queue) =
        registry_queue_init(&conn).context("failed to enumerate Wayland globals")?;
    let qh = event_queue.handle();

    let mut event_loop: EventLoop<App> =
        EventLoop::try_new().context("failed to create event loop")?;
    WaylandSource::new(conn.clone(), event_queue)
        .insert(event_loop.handle())
        .context("failed to register Wayland event source")?;

    let compositor =
        CompositorState::bind(&globals, &qh).context("wl_compositor is unavailable")?;
    let layer_shell = LayerShell::bind(&globals, &qh).context("wlr-layer-shell is unavailable")?;
    let shm = Shm::bind(&globals, &qh).context("wl_shm is unavailable")?;
    let pool = SlotPool::new(4, &shm).context("failed to create Wayland SHM pool")?;

    let mut app = App {
        compositor,
        layer_shell,
        registry_state: RegistryState::new(&globals),
        seat_state: SeatState::new(&globals, &qh),
        output_state: OutputState::new(&globals, &qh),
        shm,
        pool,
        layer: None,
        pointer: None,
        keyboard: None,
        width: 1,
        height: 1,
        configured: false,
        exit: false,
        daemon_mode,
        popup_started: None,
        configure_ms: None,
        pointer_ms: None,
        visible_ms: None,
        menu: MenuState::new(config, style),
        renderer: Renderer::new(),
    };

    if let Some(listener) = listener {
        let popup_qh = qh.clone();
        event_loop
            .handle()
            .insert_source(
                Generic::new(listener, Interest::READ, Mode::Level),
                move |_, listener, app| {
                    loop {
                        match listener.as_ref().accept() {
                            Ok((mut stream, _)) => match ipc::read_request(&mut stream) {
                                Ok(Some(Request::Popup)) => {
                                    if let Err(error) = app.show(&popup_qh) {
                                        eprintln!("mhyprmenu: failed to show menu: {error:#}");
                                    }
                                }
                                Ok(Some(Request::Reload)) => {
                                    if let Err(error) = app.reload_config() {
                                        eprintln!("mhyprmenu: failed to reload config: {error:#}");
                                    }
                                }
                                Ok(Some(Request::Status)) => {
                                    if let Err(error) =
                                        ipc::write_response(&mut stream, &app.status_text())
                                    {
                                        eprintln!("mhyprmenu: failed to send status: {error:#}");
                                    }
                                }
                                Ok(Some(Request::Quit)) => {
                                    app.exit = true;
                                }
                                Ok(None) => {}
                                Err(error) => {
                                    eprintln!("mhyprmenu: daemon request failed: {error:#}");
                                }
                            },
                            Err(error) if error.kind() == io::ErrorKind::WouldBlock => break,
                            Err(error) => {
                                eprintln!("mhyprmenu: daemon accept failed: {error}");
                                break;
                            }
                        }
                    }

                    Ok(PostAction::Continue)
                },
            )
            .context("failed to register daemon socket")?;
    } else {
        app.show(&qh)?;
    }

    while !app.exit {
        event_loop
            .dispatch(None, &mut app)
            .context("event loop dispatch failed")?;
    }

    Ok(())
}

struct App {
    compositor: CompositorState,
    layer_shell: LayerShell,
    registry_state: RegistryState,
    seat_state: SeatState,
    output_state: OutputState,
    shm: Shm,
    pool: SlotPool,
    layer: Option<LayerSurface>,
    pointer: Option<wl_pointer::WlPointer>,
    keyboard: Option<wl_keyboard::WlKeyboard>,
    width: u32,
    height: u32,
    configured: bool,
    exit: bool,
    daemon_mode: bool,
    popup_started: Option<Instant>,
    configure_ms: Option<u128>,
    pointer_ms: Option<u128>,
    visible_ms: Option<u128>,
    menu: MenuState,
    renderer: Renderer,
}

impl App {
    fn show(&mut self, qh: &QueueHandle<Self>) -> Result<()> {
        if self.layer.is_some() {
            self.hide();
        }

        self.menu.reset();
        self.width = 1;
        self.height = 1;
        self.configured = false;
        self.popup_started = Some(Instant::now());
        self.configure_ms = None;
        self.pointer_ms = None;
        self.visible_ms = None;

        match hyprland::cursor_position_local() {
            Ok((x, y)) => {
                self.menu.set_origin(x, y);
                self.pointer_ms = self.elapsed_ms();
            }
            Err(error) => {
                eprintln!(
                    "mhyprmenu: failed to read cursor position, waiting for pointer event: {error:#}"
                );
            }
        }

        let surface = self.compositor.create_surface(qh);
        let layer = self.layer_shell.create_layer_surface(
            qh,
            surface,
            Layer::Overlay,
            Some("mhyprmenu"),
            None,
        );

        layer.set_anchor(Anchor::TOP | Anchor::BOTTOM | Anchor::LEFT | Anchor::RIGHT);
        layer.set_keyboard_interactivity(KeyboardInteractivity::Exclusive);
        layer.set_exclusive_zone(-1);
        layer.set_size(0, 0);
        layer.commit();

        self.layer = Some(layer);
        Ok(())
    }

    fn hide(&mut self) {
        self.layer.take();
        self.configured = false;
        self.width = 1;
        self.height = 1;
        self.menu.reset();
    }

    fn close_menu(&mut self) {
        if self.daemon_mode {
            self.hide();
        } else {
            self.exit = true;
        }
    }

    fn reload_config(&mut self) -> Result<()> {
        let config = Config::load()?;
        let style = Style::load()?;
        self.menu.replace_config(config, style);
        if self.layer.is_some() {
            self.draw();
        }
        Ok(())
    }

    fn elapsed_ms(&self) -> Option<u128> {
        self.popup_started
            .map(|started| started.elapsed().as_millis())
    }

    fn status_text(&self) -> String {
        format!(
            "daemon=1 visible={} configure_ms={} origin_ms={} visible_ms={}\n",
            u8::from(self.layer.is_some()),
            format_timing(self.configure_ms),
            format_timing(self.pointer_ms),
            format_timing(self.visible_ms),
        )
    }

    fn draw(&mut self) {
        if !self.configured || self.width == 0 || self.height == 0 {
            return;
        }

        let Some(surface) = self.layer.as_ref().map(|layer| layer.wl_surface().clone()) else {
            return;
        };

        let width = self.width;
        let height = self.height;
        let stride = width as i32 * 4;

        let (buffer, canvas) = match self.pool.create_buffer(
            width as i32,
            height as i32,
            stride,
            wl_shm::Format::Argb8888,
        ) {
            Ok(pair) => pair,
            Err(error) => {
                eprintln!("mhyprmenu: failed to create SHM buffer: {error}");
                self.close_menu();
                return;
            }
        };

        self.renderer.draw(canvas, width, height, &self.menu);
        surface.damage_buffer(0, 0, width as i32, height as i32);

        if let Err(error) = buffer.attach_to(&surface) {
            eprintln!("mhyprmenu: failed to attach SHM buffer: {error}");
            self.close_menu();
            return;
        }

        if let Some(layer) = &self.layer {
            layer.commit();
        }

        if self.menu.has_origin() && self.visible_ms.is_none() {
            self.visible_ms = self.elapsed_ms();
        }
    }

    fn activate(&mut self, x: f64, y: f64) {
        let outcome = self.menu.click(x, y, self.width as f64, self.height as f64);
        self.apply_outcome(outcome, false);
    }

    fn activate_selected(&mut self) {
        let outcome = self.menu.activate_selected();
        self.apply_outcome(outcome, true);
    }

    fn apply_outcome(&mut self, outcome: ClickOutcome, redraw_on_keep: bool) {
        match outcome {
            ClickOutcome::Keep => {
                if redraw_on_keep {
                    self.draw();
                }
            }
            ClickOutcome::Close => self.close_menu(),
            ClickOutcome::Command(command) => {
                if let Err(error) = Command::new("sh").arg("-lc").arg(&command).spawn() {
                    eprintln!("mhyprmenu: failed to execute {command:?}: {error}");
                }
                self.close_menu();
            }
        }
    }
}

impl CompositorHandler for App {
    fn scale_factor_changed(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _surface: &wl_surface::WlSurface,
        _new_factor: i32,
    ) {
    }

    fn transform_changed(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _surface: &wl_surface::WlSurface,
        _new_transform: wl_output::Transform,
    ) {
    }

    fn frame(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _surface: &wl_surface::WlSurface,
        _time: u32,
    ) {
    }

    fn surface_enter(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _surface: &wl_surface::WlSurface,
        _output: &wl_output::WlOutput,
    ) {
    }

    fn surface_leave(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _surface: &wl_surface::WlSurface,
        _output: &wl_output::WlOutput,
    ) {
    }
}

impl LayerShellHandler for App {
    fn closed(&mut self, _conn: &Connection, _qh: &QueueHandle<Self>, layer: &LayerSurface) {
        let is_current = self
            .layer
            .as_ref()
            .is_some_and(|current| current.wl_surface() == layer.wl_surface());

        if is_current {
            self.close_menu();
        }
    }

    fn configure(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        layer: &LayerSurface,
        configure: LayerSurfaceConfigure,
        _serial: u32,
    ) {
        let is_current = self
            .layer
            .as_ref()
            .is_some_and(|current| current.wl_surface() == layer.wl_surface());

        if !is_current {
            return;
        }

        self.width = configure.new_size.0.max(1);
        self.height = configure.new_size.1.max(1);
        self.configured = true;
        if self.configure_ms.is_none() {
            self.configure_ms = self.elapsed_ms();
        }
        self.draw();
    }
}

impl SeatHandler for App {
    fn seat_state(&mut self) -> &mut SeatState {
        &mut self.seat_state
    }

    fn new_seat(&mut self, _conn: &Connection, _qh: &QueueHandle<Self>, _seat: wl_seat::WlSeat) {}

    fn new_capability(
        &mut self,
        _conn: &Connection,
        qh: &QueueHandle<Self>,
        seat: wl_seat::WlSeat,
        capability: Capability,
    ) {
        if capability == Capability::Pointer && self.pointer.is_none() {
            match self.seat_state.get_pointer(qh, &seat) {
                Ok(pointer) => self.pointer = Some(pointer),
                Err(error) => eprintln!("mhyprmenu: failed to create pointer: {error}"),
            }
        }

        if capability == Capability::Keyboard && self.keyboard.is_none() {
            self.keyboard = Some(seat.get_keyboard(qh, ()));
        }
    }

    fn remove_capability(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _seat: wl_seat::WlSeat,
        capability: Capability,
    ) {
        if capability == Capability::Pointer
            && let Some(pointer) = self.pointer.take()
        {
            pointer.release();
        }

        if capability == Capability::Keyboard {
            self.keyboard.take();
        }
    }

    fn remove_seat(&mut self, _conn: &Connection, _qh: &QueueHandle<Self>, _seat: wl_seat::WlSeat) {
    }
}

impl PointerHandler for App {
    fn pointer_frame(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _pointer: &wl_pointer::WlPointer,
        events: &[PointerEvent],
    ) {
        let Some(surface) = self.layer.as_ref().map(|layer| layer.wl_surface().clone()) else {
            return;
        };

        for event in events {
            if event.surface != surface {
                continue;
            }

            match event.kind {
                PointerEventKind::Enter { .. } | PointerEventKind::Motion { .. } => {
                    if self.pointer_ms.is_none() {
                        self.pointer_ms = self.elapsed_ms();
                    }
                    if self.menu.pointer_moved(
                        event.position.0,
                        event.position.1,
                        self.width as f64,
                        self.height as f64,
                    ) {
                        self.draw();
                    }
                }
                PointerEventKind::Press { button, .. } if button == BTN_LEFT => {
                    self.activate(event.position.0, event.position.1);
                }
                PointerEventKind::Press { button, .. } if button == BTN_RIGHT => {
                    self.close_menu();
                }
                _ => {}
            }
        }
    }
}

impl OutputHandler for App {
    fn output_state(&mut self) -> &mut OutputState {
        &mut self.output_state
    }

    fn new_output(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _output: wl_output::WlOutput,
    ) {
    }

    fn update_output(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _output: wl_output::WlOutput,
    ) {
    }

    fn output_destroyed(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _output: wl_output::WlOutput,
    ) {
    }
}

impl ShmHandler for App {
    fn shm_state(&mut self) -> &mut Shm {
        &mut self.shm
    }
}

impl Dispatch<wl_keyboard::WlKeyboard, ()> for App {
    fn event(
        app: &mut Self,
        _keyboard: &wl_keyboard::WlKeyboard,
        event: wl_keyboard::Event,
        _data: &(),
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
        if let wl_keyboard::Event::Key {
            key,
            state: WEnum::Value(wl_keyboard::KeyState::Pressed),
            ..
        } = event
        {
            match key {
                1 => app.close_menu(), // Esc
                103 => {
                    if app.menu.move_vertical(-1) {
                        app.draw();
                    }
                }
                108 => {
                    if app.menu.move_vertical(1) {
                        app.draw();
                    }
                }
                105 => {
                    if app.menu.move_left() {
                        app.draw();
                    }
                }
                106 => {
                    if app.menu.move_right() {
                        app.draw();
                    }
                }
                28 | 96 => app.activate_selected(), // Enter / keypad Enter
                _ => {}
            }
        }
    }
}

delegate_registry!(App);

impl ProvidesRegistryState for App {
    fn registry(&mut self) -> &mut RegistryState {
        &mut self.registry_state
    }

    registry_handlers![OutputState, SeatState];
}

smithay_client_toolkit::delegate_dispatch2!(App);
