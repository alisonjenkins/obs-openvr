use std::fmt::{
    self,
    Debug,
};
use crate::mirror::utils::{
    self,
    TextureFormat,
};
use obs::graphics::GsTexture;

#[derive(Debug, thiserror::Error)]
pub enum TextureCreationError {
    #[error("Error translating texture format to obs")]
    FormatTranslation(TextureFormat),
    #[error("Error allocating OBS texture")]
    TextureAllocation,
    #[error("OBS texture has no OpenGL object (is OBS using the OpenGL renderer?)")]
    NoGlObject,
}

#[derive(Debug, thiserror::Error)]
pub enum CopyTextureError {
    #[error("OpenGL error: {0:#x}")]
    Gl(i32),
    #[error("Error creating OBS texture: {0}")]
    TextureCreation(#[from] TextureCreationError),
}

/// How frames get from the OpenVR mirror texture into OBS
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CopyMode {
    /// Blit straight from the mirror texture into an OBS texture on the GPU
    Gpu,
    /// Read the texture back into memory and upload it again (slow, for debugging)
    Cpu,
}

/// A region of the mirror texture, in texture memory coordinates
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CropRect {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}

impl CropRect {
    pub const fn full(dimensions: (i32, i32)) -> Self {
        CropRect {
            x: 0,
            y: 0,
            width: dimensions.0,
            height: dimensions.1,
        }
    }

    #[inline(always)]
    pub fn dimensions(&self) -> (i32, i32) {
        (self.width, self.height)
    }
}

/// User-facing crop settings, turned into a `CropRect` once the texture size is known
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CropSettings {
    /// Output aspect ratio (width / height), or `None` to keep the whole texture
    pub aspect: Option<f64>,
    /// Size of the crop, as a percentage of the largest rectangle of that aspect that fits
    pub size_percent: f64,
    /// Moves the crop centre, as a percentage of half the texture size (-100..100)
    pub offset_x_percent: f64,
    pub offset_y_percent: f64,
    /// Whether the image is drawn flipped; offsets are given as the user sees the image
    pub flip: bool,
}

impl Default for CropSettings {
    fn default() -> Self {
        CropSettings {
            aspect: Some(16.0 / 9.0),
            size_percent: 70.0,
            offset_x_percent: 0.0,
            offset_y_percent: 0.0,
            flip: false,
        }
    }
}

impl CropSettings {
    pub fn rect_for(&self, dimensions: (i32, i32)) -> CropRect {
        let (tw, th) = dimensions;
        if tw <= 0 || th <= 0 {
            return CropRect::full(dimensions);
        }
        let (tw_f, th_f) = (tw as f64, th as f64);
        let (mut w, mut h) = match self.aspect {
            Some(aspect) if aspect > 0.0 => {
                if tw_f / th_f > aspect {
                    (th_f * aspect, th_f)
                } else {
                    (tw_f, tw_f / aspect)
                }
            },
            _ => (tw_f, th_f),
        };
        let scale = (self.size_percent / 100.0).clamp(0.05, 1.0);
        w = (w * scale).round().clamp(1.0, tw_f);
        h = (h * scale).round().clamp(1.0, th_f);

        let offset_y = if self.flip { -self.offset_y_percent } else { self.offset_y_percent };
        let cx = tw_f / 2.0 + (self.offset_x_percent / 100.0).clamp(-1.0, 1.0) * tw_f / 2.0;
        let cy = th_f / 2.0 + (offset_y / 100.0).clamp(-1.0, 1.0) * th_f / 2.0;
        let x = (cx - w / 2.0).round().clamp(0.0, tw_f - w);
        let y = (cy - h / 2.0).round().clamp(0.0, th_f - h);
        CropRect {
            x: x as i32,
            y: y as i32,
            width: w as i32,
            height: h as i32,
        }
    }
}

pub struct OpenVRMirrorCapture {
    eye: openvr::sys::EVREye,
    texture_info: openvr::compositor::MirrorTextureInfo,
    dimensions: (i32, i32),
    format: TextureFormat,
    mode: CopyMode,
    crop: CropRect,
    /// Full-texture readback buffer (CPU mode only)
    buffer: Vec<u8>,
    /// Cropped rows (CPU mode only)
    crop_buffer: Vec<u8>,
    texture: Option<obs::graphics::Texture>,
}

impl Debug for OpenVRMirrorCapture {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("OpenVRMirrorCapture")
            .field("eye", &self.eye)
            .field("texture_info", &self.texture_info)
            .field("dimensions", &self.dimensions)
            .field("format", &self.format)
            .field("mode", &self.mode)
            .field("crop", &self.crop)
            .finish()
    }
}

impl OpenVRMirrorCapture {
    /// Must be called inside the OBS graphics context
    pub unsafe fn new(eye: openvr::sys::EVREye, mode: CopyMode, crop: &CropSettings) -> Result<Self, openvr::sys::EVRCompositorError> {
        trace!("Creating OpenVRMirrorCapture with eye: {:?}", &eye);
        let texture_info = openvr::compositor::get_mirror_texture_gl(eye)?;
        let dimensions: (i32, i32) = utils::get_gl_texture_size(texture_info.id).into();
        let format = TextureFormat::Rgba;
        let buffer = match mode {
            CopyMode::Cpu => vec![0; dimensions.0.max(0) as usize * dimensions.1.max(0) as usize * format.bytes_per_pixel() as usize],
            CopyMode::Gpu => Vec::new(),
        };
        let ret = OpenVRMirrorCapture {
            eye,
            texture_info,
            dimensions,
            format,
            mode,
            crop: crop.rect_for(dimensions),
            buffer,
            crop_buffer: Vec::new(),
            texture: None,
        };
        info!("Created mirror capture: {:?}", &ret);
        Ok(ret)
    }

    /// Changes the crop without recreating the mirror texture
    pub fn set_crop(&mut self, crop: &CropSettings) {
        let rect = crop.rect_for(self.dimensions);
        if rect != self.crop {
            self.crop = rect;
            // Size changed, so the output texture has to be reallocated
            self.texture = None;
        }
    }

    #[inline(always)]
    pub fn mode(&self) -> CopyMode {
        self.mode
    }

    #[inline(always)]
    pub fn eye(&self) -> openvr::sys::EVREye {
        self.eye
    }

    /// Size of the texture OpenVR hands us
    #[inline(always)]
    pub fn source_dimensions(&self) -> (i32, i32) {
        self.dimensions
    }

    /// Size of the image we output (after cropping)
    #[inline(always)]
    pub fn dimensions(&self) -> (i32, i32) {
        self.crop.dimensions()
    }

    #[inline(always)]
    pub fn texture<'a>(&'a self) -> Option<&'a obs::graphics::Texture> {
        self.texture.as_ref()
    }

    /// Copies the latest frame into our OBS texture. Must be called inside the OBS graphics
    /// context.
    pub unsafe fn copy_texture(&mut self) -> Result<(), CopyTextureError> {
        if self.texture.is_none() {
            self.texture = Some(self.create_texture()?);
        }
        // Dropped at the end of this function, long before the texture is released
        let _lock = self.texture_info.lock_detached();
        match self.mode {
            CopyMode::Gpu => self.copy_gpu(),
            CopyMode::Cpu => self.copy_cpu(),
        }
    }

    unsafe fn copy_gpu(&mut self) -> Result<(), CopyTextureError> {
        let texture = self.texture.as_mut().unwrap();
        let dst = gl_object(texture.as_mut())
            .ok_or(TextureCreationError::NoGlObject)?;
        let c = self.crop;
        let status = utils::blit_gl_texture(self.texture_info.id, c.x, c.y, c.width, c.height, dst, c.width, c.height);
        if status != 0 {
            return Err(CopyTextureError::Gl(status));
        }
        Ok(())
    }

    unsafe fn copy_cpu(&mut self) -> Result<(), CopyTextureError> {
        utils::copy_gl_texture(self.texture_info.id, self.format.into(), self.buffer.as_mut_ptr())
            .map_err(|e| CopyTextureError::Gl(e as i32))?;
        let bpp = self.format.bytes_per_pixel() as usize;
        let c = self.crop;
        let src_line = self.dimensions.0 as usize * bpp;
        let dst_line = c.width as usize * bpp;
        let pixels: &[u8] = if c == CropRect::full(self.dimensions) {
            self.buffer.as_slice()
        } else {
            self.crop_buffer.resize(dst_line * c.height as usize, 0);
            for row in 0..c.height as usize {
                let src_start = (c.y as usize + row) * src_line + c.x as usize * bpp;
                let dst_start = row * dst_line;
                self.crop_buffer[dst_start..dst_start + dst_line]
                    .copy_from_slice(&self.buffer[src_start..src_start + dst_line]);
            }
            self.crop_buffer.as_slice()
        };
        let texture = self.texture.as_mut().unwrap();
        texture.set_image_unchecked(pixels, dst_line as u32, false);
        Ok(())
    }

    unsafe fn create_texture(&self) -> Result<obs::graphics::Texture, TextureCreationError> {
        let (w, h) = self.dimensions();
        let format: Option<obs::sys::gs_color_format> = self.format.into();
        let format = format.ok_or(TextureCreationError::FormatTranslation(self.format))?;
        let flags = match self.mode {
            CopyMode::Gpu => obs::sys::GS_RENDER_TARGET,
            CopyMode::Cpu => obs::sys::GS_DYNAMIC,
        };
        obs::graphics::Texture::new(w as u32, h as u32, format, &[std::ptr::null()], flags)
            .ok_or(TextureCreationError::TextureAllocation)
    }
}

/// The OpenGL texture name behind an OBS texture. With the OpenGL renderer,
/// `gs_texture_get_obj` returns a pointer to the `GLuint`.
unsafe fn gl_object(texture: *mut obs::sys::gs_texture_t) -> Option<u32> {
    let obj = obs::sys::gs_texture_get_obj(texture) as *const u32;
    obj.as_ref().copied().filter(|&id| id != 0)
}
