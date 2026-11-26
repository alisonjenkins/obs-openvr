extern crate obs;
extern crate openvr;
#[cfg(feature = "mirror-source")]
extern crate image;
#[macro_use] extern crate log;
extern crate env_logger;
extern crate thiserror;
extern crate mcoffin_option_ext;

pub use obs::sys as obs_sys;

mod logging;
pub mod module;
pub(crate) mod timing;
pub(crate) mod thread_utils;
#[cfg(feature = "overlay-source")]
pub mod overlay;
#[cfg(feature = "mirror-source")]
pub mod mirror;

pub use openvr::sys as openvr_sys;

use std::{
    borrow::Cow,
    sync::RwLock,
};

#[derive(Debug, Clone, Copy, thiserror::Error)]
pub enum ObsOpenVRError {
    #[error("OpenVR initialization has not yet been attempted")]
    InitNotAttempted,
    #[error("OpenVR failed to initialize: {0:?}")]
    OpenVRInit(openvr_sys::EVRInitError),
    #[error("OpenVR failed to initialize, but with no error")]
    OpenVRInitNoError,
    #[error("OpenVR was not actually shut down on obs_module_unload")]
    OpenVRShutdown,
}

struct ObsOpenVRModule {}

impl ObsOpenVRModule {
    fn unload_internal() -> Result<(), <Self as obs::ObsModule>::UnloadErr> {
        trace!("unloading");
        // Nothing to do if SteamVR never connected (or already went away)
        let was_initialized = openvr::is_initialized();
        if was_initialized && !openvr::shutdown() {
            return Err(ObsOpenVRError::OpenVRShutdown);
        }
        trace!("unloaded");
        Ok(())
    }
}

static OPENVR_INIT_RESULT: RwLock<Result<(), ObsOpenVRError>> = RwLock::new(Err(ObsOpenVRError::InitNotAttempted));
pub fn init_openvr() -> Result<(), ObsOpenVRError> {
    {
        let init_result = OPENVR_INIT_RESULT.read().unwrap();
        if init_result.is_ok() {
            return Ok(());
        }
    }
    let mut init_result = OPENVR_INIT_RESULT.write().unwrap();
    if init_result.is_ok() {
        return Ok(());
    }
    *init_result = (|| {
        let vr_initialized = openvr::init(openvr_sys::EVRApplicationType::EVRApplicationType_VRApplication_Background)
            .map(|result| result.value())
            .map_err(ObsOpenVRError::OpenVRInit)?;
        if !vr_initialized {
            return Err(ObsOpenVRError::OpenVRInitNoError);
        }
        Ok(())
    })();
    if init_result.is_ok() {
        session::on_connected();
    }
    *init_result
}

/// Tracks the SteamVR connection so sources can recover when SteamVR starts after OBS, quits, or
/// restarts.
pub mod session {
    use std::{
        sync::{
            Mutex,
            atomic::{
                AtomicU64,
                Ordering,
            },
        },
        time::{
            Duration,
            Instant,
        },
    };

    /// How often to try connecting while SteamVR isn't running
    const RETRY_INTERVAL: Duration = Duration::from_secs(2);
    /// How long sources get to release their textures after SteamVR asks us to quit
    const QUIT_GRACE: Duration = Duration::from_millis(500);

    enum State {
        Down { last_attempt: Option<Instant> },
        Up,
        Quitting { since: Instant },
    }

    static STATE: Mutex<State> = Mutex::new(State::Down { last_attempt: None });
    /// Bumped every time we (re)connect, so sources can tell their textures are stale
    static GENERATION: AtomicU64 = AtomicU64::new(0);

    pub(crate) fn on_connected() {
        let generation = GENERATION.fetch_add(1, Ordering::SeqCst) + 1;
        info!("connected to SteamVR (session {})", generation);
        *STATE.lock().unwrap() = State::Up;
    }

    /// Called from every source tick. Returns the current session number while SteamVR is
    /// connected, or `None` if sources should drop anything they hold from OpenVR.
    pub fn tick() -> Option<u64> {
        let mut state = STATE.lock().unwrap();
        match *state {
            State::Up => {
                if openvr::poll_quit() {
                    info!("SteamVR is quitting; releasing mirror textures");
                    *state = State::Quitting { since: Instant::now() };
                    return None;
                }
                Some(GENERATION.load(Ordering::SeqCst))
            },
            State::Quitting { since } => {
                if since.elapsed() >= QUIT_GRACE {
                    drop(state);
                    shutdown();
                }
                None
            },
            State::Down { last_attempt } => {
                if last_attempt.map_or(false, |t| t.elapsed() < RETRY_INTERVAL) {
                    return None;
                }
                *state = State::Down { last_attempt: Some(Instant::now()) };
                drop(state);
                match crate::init_openvr() {
                    Ok(()) => Some(GENERATION.load(Ordering::SeqCst)),
                    Err(e) => {
                        trace!("SteamVR not available yet: {}", e);
                        None
                    },
                }
            },
        }
    }

    fn shutdown() {
        openvr::shutdown();
        *crate::OPENVR_INIT_RESULT.write().unwrap() = Err(crate::ObsOpenVRError::InitNotAttempted);
        *STATE.lock().unwrap() = State::Down { last_attempt: Some(Instant::now()) };
        info!("disconnected from SteamVR; will reconnect when it starts again");
    }
}

impl obs::ObsModule for ObsOpenVRModule {
    const CRATE_NAME: &'static str = env!("CARGO_CRATE_NAME");
    type LoadErr = ObsOpenVRError;
    type UnloadErr = ObsOpenVRError;
    fn load() -> Result<(), Self::LoadErr> {
        // Initialize logging
        logging::init();
        info!("logging initialized");

        // Try to Initialize OpenVR
        if let Err(e) = init_openvr() {
            warn!("error initializing openvr on startup: {}", &e);
        }

        // Create source info struct, and register it
        #[cfg(feature = "mirror-source")]
        obs::register_video_source!(mirror::OpenVRMirrorSource);
        #[cfg(feature = "overlay-source")]
        obs::register_video_source!(overlay::OpenVROverlaySource);
        #[cfg(feature = "overlay-source")]
        obs::register_async_video_source!(overlay::OpenVRAsyncOverlaySource);

        trace!("loaded");
        Ok(())
    }
    fn unload() -> Result<(), Self::UnloadErr> {
        let ret = Self::unload_internal();
        if let Err(e) = ret.as_ref() {
            warn!("error unloading {}: {}", Self::CRATE_NAME, e);
        }
        ret
    }
}

obs::register_module!(ObsOpenVRModule);
