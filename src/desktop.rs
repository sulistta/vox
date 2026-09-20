//! Wayland RemoteDesktop + ScreenCast session and the PipeWire frame buffer.
//!
//! The portal session is intentionally kept in one object.  A frame request only
//! reads the most recent buffer produced by the PipeWire worker; it never opens a
//! second portal session.

use std::{
    os::fd::OwnedFd,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::Duration,
};

use ashpd::desktop::{
    PersistMode,
    remote_desktop::{
        DeviceType, KeyState, NotifyKeyboardKeysymOptions, NotifyPointerButtonOptions,
        NotifyPointerMotionAbsoluteOptions, RemoteDesktop, SelectDevicesOptions,
    },
    screencast::{CursorMode, Screencast, SelectSourcesOptions, SourceType},
};
use enumflags2::BitFlags;
use image::DynamicImage;
use pipewire as pw;
use pw::{properties::properties, spa};
use thiserror::Error;

use crate::vision::BoundingBox;

#[derive(Clone, Debug)]
pub struct Frame {
    pub rgba: Vec<u8>,
    pub pixel_width: u32,
    pub pixel_height: u32,
    pub logical_width: u32,
    pub logical_height: u32,
    pub stream_node_id: u32,
}

impl Frame {
    pub fn from_image(image: DynamicImage) -> Self {
        let rgba = image.to_rgba8();
        Self {
            pixel_width: rgba.width(),
            pixel_height: rgba.height(),
            logical_width: rgba.width(),
            logical_height: rgba.height(),
            stream_node_id: 0,
            rgba: rgba.into_raw(),
        }
    }

    pub fn normalized_to_pixel(&self, bbox: BoundingBox) -> (u32, u32) {
        (
            (bbox.center_x.clamp(0.0, 1.0) * self.pixel_width as f64)
                .round()
                .min(self.pixel_width.saturating_sub(1) as f64) as u32,
            (bbox.center_y.clamp(0.0, 1.0) * self.pixel_height as f64)
                .round()
                .min(self.pixel_height.saturating_sub(1) as f64) as u32,
        )
    }

    pub fn normalized_to_logical(&self, bbox: BoundingBox) -> (f64, f64) {
        (
            bbox.center_x.clamp(0.0, 1.0) * self.logical_width as f64,
            bbox.center_y.clamp(0.0, 1.0) * self.logical_height as f64,
        )
    }
}

#[derive(Debug, Error)]
pub enum DesktopError {
    #[error("portal error: {0}")]
    Portal(#[from] ashpd::Error),
    #[error("PipeWire error: {0}")]
    PipeWire(#[from] pw::Error),
    #[error("portal did not return a monitor stream")]
    NoStream,
    #[error("no PipeWire frame is available yet")]
    NoFrame,
    #[error("input character {0:?} is not supported by the MVP")]
    UnsupportedCharacter(char),
}

#[derive(Clone)]
struct FrameStore(Arc<Mutex<Option<Frame>>>);

struct PipeUserData {
    format: spa::param::video::VideoInfoRaw,
    store: FrameStore,
    logical_size: (u32, u32),
    node_id: u32,
}

/// A PipeWire worker.  The worker owns the PipeWire main loop and stream and
/// continuously replaces `latest` with a copied RGBA frame.
pub struct PipewireCapture {
    latest: FrameStore,
    stop: Arc<AtomicBool>,
    worker: Option<thread::JoinHandle<()>>,
}

impl PipewireCapture {
    pub fn start(
        node_id: u32,
        logical_size: (u32, u32),
        fd: OwnedFd,
    ) -> Result<Self, DesktopError> {
        let latest = FrameStore(Arc::new(Mutex::new(None)));
        let worker_store = latest.clone();
        let stop = Arc::new(AtomicBool::new(false));
        let worker_stop = stop.clone();
        let worker = thread::Builder::new()
            .name("vocs-pipewire".into())
            .spawn(move || {
                if let Err(error) =
                    run_pipewire(node_id, logical_size, fd, worker_store, worker_stop)
                {
                    tracing::error!(%error, "PipeWire worker stopped");
                }
            })
            .map_err(|_| DesktopError::NoFrame)?;
        Ok(Self {
            latest,
            stop,
            worker: Some(worker),
        })
    }

    pub fn capture_latest(&self) -> Result<Frame, DesktopError> {
        self.latest
            .0
            .lock()
            .map_err(|_| DesktopError::NoFrame)?
            .clone()
            .ok_or(DesktopError::NoFrame)
    }
}

impl Drop for PipewireCapture {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

fn run_pipewire(
    node_id: u32,
    logical_size: (u32, u32),
    fd: OwnedFd,
    store: FrameStore,
    stop: Arc<AtomicBool>,
) -> Result<(), pw::Error> {
    let mainloop = pw::main_loop::MainLoopBox::new(None)?;
    let context = pw::context::ContextBox::new(mainloop.loop_(), None)?;
    let core = context.connect_fd(fd, None)?;
    let stream = pw::stream::StreamBox::new(
        &core,
        "vocs-screen-capture",
        properties! {
            *pw::keys::MEDIA_TYPE => "Video",
            *pw::keys::MEDIA_CATEGORY => "Capture",
            *pw::keys::MEDIA_ROLE => "Screen",
        },
    )?;

    let data = PipeUserData {
        format: Default::default(),
        store,
        logical_size,
        node_id,
    };
    let _listener = stream
        .add_local_listener_with_user_data(data)
        .param_changed(|_, user_data, id, param| {
            let Some(param) = param else { return };
            if id != pw::spa::param::ParamType::Format.as_raw() {
                return;
            }
            let Ok((media_type, media_subtype)) = pw::spa::param::format_utils::parse_format(param)
            else {
                return;
            };
            if media_type != pw::spa::param::format::MediaType::Video
                || media_subtype != pw::spa::param::format::MediaSubtype::Raw
            {
                return;
            }
            if let Err(error) = user_data.format.parse(param) {
                tracing::warn!(%error, "could not parse PipeWire video format");
            }
        })
        .process(|stream, user_data| {
            let Some(mut buffer) = stream.dequeue_buffer() else {
                return;
            };
            let datas = buffer.datas_mut();
            let Some(data) = datas.first_mut() else {
                return;
            };
            let width = user_data.format.size().width;
            let height = user_data.format.size().height;
            if width == 0 || height == 0 {
                return;
            }
            let width = width as usize;
            let height = height as usize;
            let stride = data.chunk().stride().unsigned_abs() as usize;
            let Some(bytes) = data.data() else { return };
            let bpp = match user_data.format.format() {
                spa::param::video::VideoFormat::RGBA
                | spa::param::video::VideoFormat::RGBx
                | spa::param::video::VideoFormat::BGRx => 4,
                spa::param::video::VideoFormat::RGB => 3,
                _ => return,
            };
            let row_len = width.saturating_mul(bpp);
            if bytes.len() < row_len || bytes.len() < stride.saturating_mul(height) {
                return;
            }
            let mut rgba = vec![0u8; width.saturating_mul(height).saturating_mul(4)];
            for y in 0..height {
                let src = &bytes[y * stride..y * stride + row_len];
                for x in 0..width {
                    let source = x * bpp;
                    let dest = (y * width + x) * 4;
                    match user_data.format.format() {
                        spa::param::video::VideoFormat::RGBA => {
                            rgba[dest..dest + 4].copy_from_slice(&src[source..source + 4]);
                        }
                        spa::param::video::VideoFormat::RGBx => {
                            rgba[dest..dest + 3].copy_from_slice(&src[source..source + 3]);
                            rgba[dest + 3] = 255;
                        }
                        spa::param::video::VideoFormat::BGRx => {
                            rgba[dest] = src[source + 2];
                            rgba[dest + 1] = src[source + 1];
                            rgba[dest + 2] = src[source];
                            rgba[dest + 3] = 255;
                        }
                        spa::param::video::VideoFormat::RGB => {
                            rgba[dest..dest + 3].copy_from_slice(&src[source..source + 3]);
                            rgba[dest + 3] = 255;
                        }
                        _ => return,
                    }
                }
            }
            if let Ok(mut latest) = user_data.store.0.try_lock() {
                *latest = Some(Frame {
                    rgba,
                    pixel_width: width as u32,
                    pixel_height: height as u32,
                    logical_width: if user_data.logical_size.0 == 0 {
                        width as u32
                    } else {
                        user_data.logical_size.0
                    },
                    logical_height: if user_data.logical_size.1 == 0 {
                        height as u32
                    } else {
                        user_data.logical_size.1
                    },
                    stream_node_id: user_data.node_id,
                });
            }
        })
        .register()?;

    let obj = pw::spa::pod::object!(
        pw::spa::utils::SpaTypes::ObjectParamFormat,
        pw::spa::param::ParamType::EnumFormat,
        pw::spa::pod::property!(
            pw::spa::param::format::FormatProperties::MediaType,
            Id,
            pw::spa::param::format::MediaType::Video
        ),
        pw::spa::pod::property!(
            pw::spa::param::format::FormatProperties::MediaSubtype,
            Id,
            pw::spa::param::format::MediaSubtype::Raw
        ),
        pw::spa::pod::property!(
            pw::spa::param::format::FormatProperties::VideoFormat,
            Choice,
            Enum,
            Id,
            pw::spa::param::video::VideoFormat::RGBA,
            pw::spa::param::video::VideoFormat::RGBA,
            pw::spa::param::video::VideoFormat::RGBx,
            pw::spa::param::video::VideoFormat::BGRx,
            pw::spa::param::video::VideoFormat::RGB
        ),
        pw::spa::pod::property!(
            pw::spa::param::format::FormatProperties::VideoSize,
            Choice,
            Range,
            Rectangle,
            pw::spa::utils::Rectangle {
                width: 320,
                height: 240
            },
            pw::spa::utils::Rectangle {
                width: 1,
                height: 1
            },
            pw::spa::utils::Rectangle {
                width: 8192,
                height: 8192
            }
        ),
        pw::spa::pod::property!(
            pw::spa::param::format::FormatProperties::VideoFramerate,
            Choice,
            Range,
            Fraction,
            pw::spa::utils::Fraction { num: 30, denom: 1 },
            pw::spa::utils::Fraction { num: 0, denom: 1 },
            pw::spa::utils::Fraction {
                num: 1000,
                denom: 1
            }
        )
    );
    let values: Vec<u8> = pw::spa::pod::serialize::PodSerializer::serialize(
        std::io::Cursor::new(Vec::new()),
        &pw::spa::pod::Value::Object(obj),
    )
    .expect("serialize PipeWire format")
    .0
    .into_inner();
    let mut params = [pw::spa::pod::Pod::from_bytes(&values).expect("valid PipeWire pod")];
    stream.connect(
        spa::utils::Direction::Input,
        Some(node_id),
        pw::stream::StreamFlags::AUTOCONNECT | pw::stream::StreamFlags::MAP_BUFFERS,
        &mut params,
    )?;
    while !stop.load(Ordering::Acquire) {
        mainloop
            .loop_()
            .iterate(pw::loop_::Timeout::Finite(Duration::from_millis(100)));
    }
    Ok(())
}

pub struct PortalSession {
    remote: Arc<RemoteDesktop>,
    session: Arc<ashpd::desktop::Session<RemoteDesktop>>,
    pub capture: PipewireCapture,
}

impl PortalSession {
    pub async fn start() -> Result<Self, DesktopError> {
        let remote = Arc::new(RemoteDesktop::new().await?);
        let screencast = Screencast::new().await?;
        let session = remote.create_session(Default::default()).await?;
        remote
            .select_devices(
                &session,
                SelectDevicesOptions::default()
                    .set_devices(DeviceType::Keyboard | DeviceType::Pointer)
                    .set_persist_mode(PersistMode::DoNot),
            )
            .await?
            .response()?;
        screencast
            .select_sources(
                &session,
                SelectSourcesOptions::default()
                    .set_cursor_mode(CursorMode::Hidden)
                    .set_sources(BitFlags::from_flag(SourceType::Monitor))
                    .set_multiple(false)
                    .set_persist_mode(PersistMode::DoNot),
            )
            .await?
            .response()?;
        let selected = remote
            .start(&session, None, Default::default())
            .await?
            .response()?;
        let stream = selected.streams().first().ok_or(DesktopError::NoStream)?;
        let node_id = stream.pipe_wire_node_id();
        let logical = stream.size().unwrap_or((0, 0));
        let logical_size = (logical.0.max(0) as u32, logical.1.max(0) as u32);
        let fd = screencast
            .open_pipe_wire_remote(&session, Default::default())
            .await?;
        let capture = PipewireCapture::start(node_id, logical_size, fd)?;
        Ok(Self {
            remote,
            session: Arc::new(session),
            capture,
        })
    }

    pub fn capture_latest(&self) -> Result<Frame, DesktopError> {
        self.capture.capture_latest()
    }

    pub async fn close(&self) -> Result<(), DesktopError> {
        self.session.close().await?;
        Ok(())
    }

    pub async fn click(&self, frame: &Frame, bbox: BoundingBox) -> Result<(), DesktopError> {
        let (x, y) = frame.normalized_to_logical(bbox);
        self.remote
            .notify_pointer_motion_absolute(
                &self.session,
                frame.stream_node_id,
                x,
                y,
                NotifyPointerMotionAbsoluteOptions::default(),
            )
            .await?;
        tokio::time::sleep(Duration::from_millis(30)).await;
        self.remote
            .notify_pointer_button(
                &self.session,
                0x110,
                KeyState::Pressed,
                NotifyPointerButtonOptions::default(),
            )
            .await?;
        self.remote
            .notify_pointer_button(
                &self.session,
                0x110,
                KeyState::Released,
                NotifyPointerButtonOptions::default(),
            )
            .await?;
        Ok(())
    }

    pub async fn press_enter(&self) -> Result<(), DesktopError> {
        self.key_sym(0xff0d).await
    }

    pub async fn type_text(&self, text: &str) -> Result<(), DesktopError> {
        let symbols = text
            .chars()
            .map(keysym_for_char)
            .collect::<Result<Vec<_>, _>>()?;
        for (keysym, shift) in symbols {
            if shift {
                self.key_sym(0xffe1).await?;
            }
            self.key_sym(keysym).await?;
            if shift {
                self.key_sym(0xffe1).await?;
            }
        }
        Ok(())
    }

    async fn key_sym(&self, keysym: i32) -> Result<(), DesktopError> {
        self.remote
            .notify_keyboard_keysym(
                &self.session,
                keysym,
                KeyState::Pressed,
                NotifyKeyboardKeysymOptions::default(),
            )
            .await?;
        self.remote
            .notify_keyboard_keysym(
                &self.session,
                keysym,
                KeyState::Released,
                NotifyKeyboardKeysymOptions::default(),
            )
            .await?;
        Ok(())
    }
}

fn keysym_for_char(ch: char) -> Result<(i32, bool), DesktopError> {
    let (symbol, shift) = match ch {
        'a'..='z' => (ch as i32, false),
        'A'..='Z' => (ch.to_ascii_lowercase() as i32, true),
        '0'..='9' | ' ' | '-' | '.' => (ch as i32, false),
        '_' => ('-' as i32, true),
        ':' => (';' as i32, true),
        _ => return Err(DesktopError::UnsupportedCharacter(ch)),
    };
    Ok((symbol, shift))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalized_coordinates_use_logical_size_for_input() {
        let frame = Frame {
            rgba: vec![0; 4 * 200 * 100],
            pixel_width: 200,
            pixel_height: 100,
            logical_width: 100,
            logical_height: 50,
            stream_node_id: 7,
        };
        let bbox = BoundingBox {
            center_x: 0.5,
            center_y: 0.5,
            width: 0.1,
            height: 0.1,
        };
        assert_eq!(frame.normalized_to_pixel(bbox), (100, 50));
        assert_eq!(frame.normalized_to_logical(bbox), (50.0, 25.0));
    }

    #[test]
    fn unsupported_text_is_rejected_before_input() {
        assert!(keysym_for_char('é').is_err());
        assert!(keysym_for_char('a').is_ok());
    }
}
