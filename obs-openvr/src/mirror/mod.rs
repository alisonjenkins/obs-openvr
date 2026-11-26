pub mod utils;
mod capture;

use capture::{
    CopyMode,
    CropSettings,
    OpenVRMirrorCapture,
};
use std::{
    ffi::CStr,
    sync::RwLock,
    time::{
        Duration,
        Instant,
    },
};
use obs::{
    graphics::with_graphics,
    data::ObsData,
    OwnedPointerContainer,
};

const DEFAULT_EYE: openvr::sys::EVREye = openvr::sys::EVREye::EVREye_Eye_Left;

macro_rules! cstr {
    ($s:expr) => {
        unsafe { CStr::from_bytes_with_nul_unchecked(concat!($s, "\0").as_bytes()) }
    };
}

mod keys {
    use std::ffi::CStr;
    pub fn eye() -> &'static CStr { cstr!("eye") }
    pub fn aspect() -> &'static CStr { cstr!("crop_aspect") }
    pub fn size() -> &'static CStr { cstr!("crop_size") }
    pub fn offset_x() -> &'static CStr { cstr!("crop_offset_x") }
    pub fn offset_y() -> &'static CStr { cstr!("crop_offset_y") }
    pub fn flip() -> &'static CStr { cstr!("flip_vertical") }
    pub fn copy_mode() -> &'static CStr { cstr!("copy_mode") }
}

/// (setting value, label, aspect ratio)
const ASPECTS: &[(&str, &str, Option<f64>)] = &[
    ("16:9\0", "16:9 (widescreen)\0", Some(16.0 / 9.0)),
    ("4:3\0", "4:3\0", Some(4.0 / 3.0)),
    ("1:1\0", "1:1 (square)\0", Some(1.0)),
    ("9:16\0", "9:16 (portrait)\0", Some(9.0 / 16.0)),
    ("none\0", "No crop (whole eye texture)\0", None),
];
const DEFAULT_ASPECT: &str = "16:9";
const DEFAULT_SIZE: f64 = 70.0;

#[derive(Debug, Clone, PartialEq)]
struct OpenVRMirrorSourceSettings {
    eye: openvr::sys::EVREye,
    copy_mode: CopyMode,
    crop: CropSettings,
}

impl OpenVRMirrorSourceSettings {
    fn from_data<D: ObsData>(data: &D) -> Self {
        use openvr::sys::EVREye::*;
        let eye = match data.get_string(keys::eye()) {
            Some("right") => EVREye_Eye_Right,
            Some("left") => EVREye_Eye_Left,
            _ => DEFAULT_EYE,
        };
        let copy_mode = match data.get_string(keys::copy_mode()) {
            Some("cpu") => CopyMode::Cpu,
            _ => CopyMode::Gpu,
        };
        let aspect_key = data.get_string(keys::aspect()).unwrap_or(DEFAULT_ASPECT);
        let aspect = ASPECTS.iter()
            .find(|(k, _, _)| k.trim_end_matches('\0') == aspect_key)
            .map(|&(_, _, a)| a)
            .unwrap_or(Some(16.0 / 9.0));
        OpenVRMirrorSourceSettings {
            eye,
            copy_mode,
            crop: CropSettings {
                aspect,
                size_percent: data.get_double(keys::size()),
                offset_x_percent: data.get_double(keys::offset_x()),
                offset_y_percent: data.get_double(keys::offset_y()),
                flip: data.get_bool(keys::flip()),
            },
        }
    }

    /// Whether going from `self` to `other` needs a new mirror texture
    fn needs_new_capture(&self, other: &Self) -> bool {
        self.eye != other.eye || self.copy_mode != other.copy_mode
    }
}

struct CaptureState {
    capture: Option<OpenVRMirrorCapture>,
    /// SteamVR session the capture belongs to
    generation: u64,
    /// Settings changed in a way that needs a new capture
    dirty: bool,
    /// Don't try creating a capture again before this (after a failure)
    retry_after: Option<Instant>,
}

/// How long to wait before retrying a mirror capture that failed to start
const CAPTURE_RETRY: Duration = Duration::from_secs(2);

pub struct OpenVRMirrorSource {
    handle: *mut obs::sys::obs_source,
    settings: RwLock<OpenVRMirrorSourceSettings>,
    state: RwLock<CaptureState>,
}

impl OpenVRMirrorSource {
    pub fn new(settings: &mut obs::sys::obs_data, handle: *mut obs::sys::obs_source) -> Self {
        OpenVRMirrorSource {
            handle,
            settings: RwLock::new(OpenVRMirrorSourceSettings::from_data(settings as &_)),
            state: RwLock::new(CaptureState {
                capture: None,
                generation: 0,
                dirty: true,
                retry_after: None,
            }),
        }
    }

    #[inline(always)]
    pub fn is_showing(&self) -> bool {
        unsafe {
            obs::sys::obs_source_showing(self.handle)
        }
    }

    /// Drops the capture inside the graphics context, so its GL texture is released in the
    /// context that owns it
    fn release_capture(state: &mut CaptureState) {
        if state.capture.is_some() {
            with_graphics(|| {
                state.capture = None;
            });
        }
    }
}

impl Drop for OpenVRMirrorSource {
    fn drop(&mut self) {
        if let Ok(mut state) = self.state.write() {
            Self::release_capture(&mut state);
        }
    }
}

unsafe extern "C" fn get_defaults(settings: *mut obs::sys::obs_data) {
    use obs::sys::*;
    obs_data_set_default_string(settings, keys::eye().as_ptr(), cstr!("left").as_ptr());
    obs_data_set_default_string(settings, keys::aspect().as_ptr(), cstr!("16:9").as_ptr());
    obs_data_set_default_double(settings, keys::size().as_ptr(), DEFAULT_SIZE);
    obs_data_set_default_double(settings, keys::offset_x().as_ptr(), 0.0);
    obs_data_set_default_double(settings, keys::offset_y().as_ptr(), 0.0);
    obs_data_set_default_bool(settings, keys::flip().as_ptr(), false);
    obs_data_set_default_string(settings, keys::copy_mode().as_ptr(), cstr!("gpu").as_ptr());
}

impl obs::source::VideoSource for OpenVRMirrorSource {
    const ID: &'static [u8] = b"obs-openvr-mirror\0";
    const OUTPUT_FLAGS: Option<u32> = None;

    fn create(settings: &mut obs::sys::obs_data, source: *mut obs::sys::obs_source_t) -> Self {
        OpenVRMirrorSource::new(settings, source)
    }

    fn get_name() -> &'static CStr {
        cstr!("OpenVR Mirror Source")
    }

    fn get_defaults() -> Option<unsafe extern "C" fn(*mut obs::sys::obs_data)> {
        Some(get_defaults)
    }

    fn get_dimensions(&self) -> (u32, u32) {
        let state = self.state.read().unwrap();
        state.capture.as_ref()
            .map(|ctx| {
                let (w, h) = ctx.dimensions();
                (w as u32, h as u32)
            })
            .unwrap_or((0, 0))
    }

    fn get_properties(&self) -> *mut obs::sys::obs_properties {
        use obs::properties::{
            Properties,
            PropertiesExt,
            PropertyDescription,
        };

        let mut props = Properties::new();

        props.add_string_list_complete(
            PropertyDescription::new(keys::eye(), Some(cstr!("Eye"))),
            [(cstr!("Left"), cstr!("left")), (cstr!("Right"), cstr!("right"))].iter().copied(),
        );
        props.add_string_list_complete(
            PropertyDescription::new(keys::aspect(), Some(cstr!("Crop to aspect ratio"))),
            ASPECTS.iter().map(|&(value, label, _)| unsafe {
                (CStr::from_bytes_with_nul_unchecked(label.as_bytes()), CStr::from_bytes_with_nul_unchecked(value.as_bytes()))
            }),
        );
        props.add_float_slider(keys::size(), cstr!("Crop size (%)"), 10.0, 100.0, 1.0);
        props.add_float_slider(keys::offset_x(), cstr!("Move crop left/right (%)"), -100.0, 100.0, 1.0);
        props.add_float_slider(keys::offset_y(), cstr!("Move crop up/down (%)"), -100.0, 100.0, 1.0);
        props.add_bool(keys::flip(), cstr!("Flip vertically"));
        props.add_string_list_complete(
            PropertyDescription::new(keys::copy_mode(), Some(cstr!("Copy method"))),
            [(cstr!("GPU (fast)"), cstr!("gpu")), (cstr!("CPU (slow, for troubleshooting)"), cstr!("cpu"))].iter().copied(),
        );

        unsafe { props.leak() }
    }

    fn update(&self, data: &obs::sys::obs_data) {
        let new_settings = OpenVRMirrorSourceSettings::from_data(data);
        let mut settings = self.settings.write().unwrap();
        let mut state = self.state.write().unwrap();
        if settings.needs_new_capture(&new_settings) {
            state.dirty = true;
        } else if let Some(capture) = state.capture.as_mut() {
            capture.set_crop(&new_settings.crop);
        }
        trace!("OpenVRMirrorSource::update: {:?}", &new_settings);
        *settings = new_settings;
    }

    fn video_tick(&self, _seconds: f32) {
        let generation = crate::session::tick();
        let mut state = self.state.write().unwrap();

        let generation = match generation {
            Some(g) => g,
            None => {
                // SteamVR is gone or going away: let go of its texture
                Self::release_capture(&mut state);
                return;
            },
        };

        if !self.is_showing() {
            return;
        }

        let stale = state.dirty || state.generation != generation;
        let waiting = !stale && state.retry_after.map_or(false, |t| Instant::now() < t);
        if (state.capture.is_none() && !waiting) || stale {
            Self::release_capture(&mut state);
            let settings = self.settings.read().unwrap();
            let result = with_graphics(|| unsafe {
                OpenVRMirrorCapture::new(settings.eye, settings.copy_mode, &settings.crop)
            });
            state.generation = generation;
            state.dirty = false;
            match result {
                Ok(capture) => {
                    state.capture = Some(capture);
                    state.retry_after = None;
                },
                Err(e) => {
                    // Usually SteamVR is still starting up, so try again shortly
                    if state.retry_after.is_none() {
                        warn!("Error creating mirror capture (will keep retrying): {:?}", &e);
                    }
                    state.retry_after = Some(Instant::now() + CAPTURE_RETRY);
                    return;
                },
            }
        }

        if let Some(capture) = state.capture.as_mut() {
            if let Err(e) = with_graphics(|| unsafe { capture.copy_texture() }) {
                error!("Error copying texture data from OpenVR mirror: {}", &e);
            }
        }
    }

    fn video_render(&self, _effect: *mut obs::sys::gs_effect_t) {
        let flip = self.settings.read().unwrap().crop.flip;
        let state = self.state.read().unwrap();
        if let Some(texture) = state.capture.as_ref().and_then(OpenVRMirrorCapture::texture) {
            obs::source::draw(&*texture, 0, 0, 0, 0, flip);
        }
    }
}
