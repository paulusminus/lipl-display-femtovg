#![doc = include_str!("../README.md")]

use femtovg::{Canvas, Color, FontId, Paint, renderer::OpenGl};
use futures_util::TryStreamExt;
use glutin::surface::GlSurface;
use lipl_display_common::{Command, HandleMessage, LiplScreen, Message};
use log::{debug, error, info};
use std::{error::Error, thread};
use winit::{
    application::ApplicationHandler, dpi::PhysicalSize, event::WindowEvent, event_loop::EventLoop,
};

const ROBOTO_REGULAR: &[u8] = include_bytes!("../assets/Roboto-Regular.ttf");
const DEFAULT_FONT_SIZE: f32 = 32.0;
const BLACK: femtovg::Color = femtovg::Color::black();
const WHITE: femtovg::Color = femtovg::Color::white();
#[cfg(not(feature = "gatt"))]
const DELAY_SECONDS: u64 = 1;

mod helpers;

fn get_colors(dark: bool) -> (Color, Color) {
    if dark { (WHITE, BLACK) } else { (BLACK, WHITE) }
}

#[cfg(feature = "gatt")]
fn listener() -> lipl_gatt_zbus::GattListener {
    lipl_gatt_zbus::GattListener::default()
}

#[cfg(not(feature = "gatt"))]
fn listener() -> impl futures_util::TryStream<Ok = Message, Error = std::io::Error> {
    use deserialize_lines::lines;
    use futures_util::StreamExt;
    use std::time::Duration;
    use tokio_stream::StreamExt as _;
    lines::<Message, _>(include_bytes!("../lipl-gatt-input.txt").as_ref())
        .into_stream()
        .throttle(Duration::from_secs(DELAY_SECONDS))
        .boxed()
}

fn main() -> Result<(), Box<dyn Error>> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();
    let event_loop = EventLoop::<Message>::with_user_event().build()?;
    let proxy = event_loop.create_proxy();

    thread::spawn(|| {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(async move {
                let mut listener = listener();
                while let Ok(Some(message)) = listener.try_next().await {
                    info!("Message = {}", message);
                    if let Err(error) = proxy.send_event(message) {
                        error!("Error sending to main loop: {error}");
                        break;
                    }
                }
                #[cfg(feature = "gatt")]
                let _ = listener.await;
            });
    });

    event_loop.run_app(&mut Application::default())?;

    Ok(())
}

struct ApplicationGraphics {
    canvas: Canvas<OpenGl>,
    context: glutin::context::PossiblyCurrentContext,
    font_id: FontId,
    surface: glutin::surface::Surface<glutin::surface::WindowSurface>,
    window: winit::window::Window,
}

struct Application {
    screen: LiplScreen,
    graphics: Option<ApplicationGraphics>,
}

impl Default for Application {
    fn default() -> Self {
        let mut screen = LiplScreen::new(false, DEFAULT_FONT_SIZE);
        screen.handle_message(Message::Part("Even geduld a.u.b. ..".to_owned()));
        Self {
            screen,
            graphics: None,
        }
    }
}

impl Application {
    fn draw(&mut self) {
        if let Some(graphics) = self.graphics.as_mut() {
            let dpi_factor = graphics.window.scale_factor();
            let size = graphics.window.inner_size();
            graphics
                .canvas
                .set_size(size.width, size.height, dpi_factor as f32);
            let x = 0.05 * graphics.canvas.width() as f32;
            let mut y = 0.05 * graphics.canvas.height() as f32 + self.screen.font_size;

            let (fg_color, bg_color) = get_colors(self.screen.dark);
            graphics
                .canvas
                .clear_rect(0, 0, size.width, size.height, bg_color);
            let mut paint = Paint::color(fg_color);
            paint.set_font(&[graphics.font_id]);
            paint.set_font_size(self.screen.font_size);

            let font_metrics = graphics
                .canvas
                .measure_font(&paint)
                .expect("Error measuring font");

            let width = graphics.canvas.width();

            let lines = graphics
                .canvas
                .break_text_vec(width as f32, &self.screen.text, &paint)
                .expect("Error while breaking text");

            for line_range in lines {
                if let Ok(_res) =
                    graphics
                        .canvas
                        .fill_text(x, y, &self.screen.text[line_range], &paint)
                {
                    y += font_metrics.height();
                }
            }

            y = graphics.canvas.height() as f32 - font_metrics.height();
            match graphics.canvas.fill_text(x, y, &self.screen.status, &paint) {
                Ok(_) => {}
                Err(e) => {
                    error!("Error: {e}");
                }
            }

            graphics.canvas.flush();

            if let Err(error) = graphics.surface.swap_buffers(&graphics.context) {
                error!("Cannot swap buffers: {error}");
            }
        }
    }

    fn resize(&mut self, size: PhysicalSize<u32>) {
        if let Some(graphics) = self.graphics.as_mut() {
            graphics.surface.resize(
                &graphics.context,
                size.width.try_into().unwrap(),
                size.height.try_into().unwrap(),
            );
        }
    }

    fn handle_message(&mut self, message: Message) {
        self.screen.handle_message(message);
        if let Some(graphics) = self.graphics.as_ref() {
            graphics.window.request_redraw();
        }
    }
}

impl ApplicationHandler<Message> for Application {
    fn resumed(&mut self, event_loop: &winit::event_loop::ActiveEventLoop) {
        info!("resumed");
        let (mut canvas, window, context, surface) =
            helpers::create_window("Lipl Display", event_loop);
        let font_id = canvas.add_font_mem(ROBOTO_REGULAR).ok().unwrap();

        self.graphics = Some(ApplicationGraphics {
            canvas,
            context,
            font_id,
            surface,
            window,
        });

        if let Some(graphics) = self.graphics.as_ref() {
            graphics.window.request_redraw();
        }
    }

    fn window_event(
        &mut self,
        event_loop: &winit::event_loop::ActiveEventLoop,
        _window_id: winit::window::WindowId,
        event: WindowEvent,
    ) {
        match event {
            WindowEvent::ScaleFactorChanged {
                scale_factor,
                inner_size_writer: _,
            } => {
                info!("window_event scalefactorchanged {scale_factor}");
            }
            WindowEvent::Resized(physical_size) => {
                info!("window_event: resized");
                self.resize(physical_size);
            }
            WindowEvent::CloseRequested => {
                debug!("window_event close");
                event_loop.exit();
            }
            _ => {
                debug!("window_event {event:#?}");
            }
        }
    }

    fn new_events(
        &mut self,
        _event_loop: &winit::event_loop::ActiveEventLoop,
        cause: winit::event::StartCause,
    ) {
        debug!("new_events {cause:#?}");
    }

    fn user_event(&mut self, event_loop: &winit::event_loop::ActiveEventLoop, event: Message) {
        info!("user_event: {event}");
        if [
            Message::Command(Command::Exit),
            Message::Command(Command::Poweroff),
        ]
        .contains(&event)
        {
            event_loop.exit();
        } else {
            self.handle_message(event);
        }
    }

    fn device_event(
        &mut self,
        _event_loop: &winit::event_loop::ActiveEventLoop,
        device_id: winit::event::DeviceId,
        _event: winit::event::DeviceEvent,
    ) {
        debug!("device_event {device_id:?}");
    }

    fn about_to_wait(&mut self, _event_loop: &winit::event_loop::ActiveEventLoop) {
        info!("About to wait");
        self.draw();
    }

    fn suspended(&mut self, _event_loop: &winit::event_loop::ActiveEventLoop) {
        self.graphics = None;
    }

    fn exiting(&mut self, event_loop: &winit::event_loop::ActiveEventLoop) {
        event_loop.exit();
    }

    fn memory_warning(&mut self, _event_loop: &winit::event_loop::ActiveEventLoop) {}
}
