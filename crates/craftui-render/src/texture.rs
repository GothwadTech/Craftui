use craftui_core::{Rect, TextureId};
use slotmap::SlotMap;
use std::sync::{Arc, Mutex};

#[derive(Debug)]
struct PixelBufferData {
    id: TextureId,
    width: u32,
    height: u32,
    rgba: Vec<u8>,
    version: u64,
    dirty_rect: Option<Rect>,
}

/// Shared mutable RGBA8 pixel surface for real-time CV, camera, and filter pipelines.
#[derive(Clone, Debug)]
pub struct PixelBufferHandle {
    inner: Arc<Mutex<PixelBufferData>>,
}

impl PixelBufferHandle {
    pub(crate) fn new(id: TextureId, width: u32, height: u32, rgba: Vec<u8>) -> Self {
        assert_eq!(rgba.len(), (width as usize) * (height as usize) * 4);
        Self {
            inner: Arc::new(Mutex::new(PixelBufferData {
                id,
                width,
                height,
                rgba,
                version: 1,
                dirty_rect: Some(Rect::new(0.0, 0.0, width as f32, height as f32)),
            })),
        }
    }

    pub fn id(&self) -> TextureId {
        self.inner.lock().map(|g| g.id).unwrap_or_default()
    }

    pub fn dimensions(&self) -> (u32, u32) {
        self.inner
            .lock()
            .map(|g| (g.width, g.height))
            .unwrap_or((0, 0))
    }

    pub fn version(&self) -> u64 {
        self.inner.lock().map(|g| g.version).unwrap_or(0)
    }

    /// Mutate raw straight RGBA8 pixels in place. Call [`commit_damage`] or [`commit_full`] afterwards.
    pub fn with_pixels_mut<R>(&self, f: impl FnOnce(u32, u32, &mut [u8]) -> R) -> R {
        let mut guard = self.inner.lock().expect("pixel buffer lock poisoned");
        let w = guard.width;
        let h = guard.height;
        f(w, h, &mut guard.rgba)
    }

    /// Mark a sub-rectangle as modified and bump the upload version.
    pub fn commit_damage(&self, rect: Rect) {
        let mut guard = self.inner.lock().expect("pixel buffer lock poisoned");
        guard.version = guard.version.wrapping_add(1);
        guard.dirty_rect = Some(rect);
    }

    /// Mark the entire buffer dirty and bump the upload version.
    pub fn commit_full(&self) {
        let mut guard = self.inner.lock().expect("pixel buffer lock poisoned");
        guard.version = guard.version.wrapping_add(1);
        guard.dirty_rect = Some(Rect::new(0.0, 0.0, guard.width as f32, guard.height as f32));
    }

    /// Resize and replace buffer contents atomically.
    pub fn replace_rgba(&self, width: u32, height: u32, rgba: Vec<u8>) {
        assert_eq!(rgba.len(), (width as usize) * (height as usize) * 4);
        let mut guard = self.inner.lock().expect("pixel buffer lock poisoned");
        guard.width = width;
        guard.height = height;
        guard.rgba = rgba;
        guard.version = guard.version.wrapping_add(1);
        guard.dirty_rect = Some(Rect::new(0.0, 0.0, width as f32, height as f32));
    }

    pub fn snapshot(&self) -> (u32, u32, u64, Vec<u8>) {
        let guard = self.inner.lock().expect("pixel buffer lock poisoned");
        (guard.width, guard.height, guard.version, guard.rgba.clone())
    }

    pub fn take_dirty_rect(&self) -> Option<Rect> {
        self.inner.lock().ok().and_then(|mut g| g.dirty_rect.take())
    }
}

/// Lightweight handle referencing an image texture registered in [`TextureRegistry`].
#[derive(Clone, Debug)]
pub struct TextureHandle {
    pub id: TextureId,
    pub buffer: PixelBufferHandle,
}

impl TextureHandle {
    pub fn dimensions(&self) -> (u32, u32) {
        self.buffer.dimensions()
    }
}

/// Central registry owning RGBA pixel buffers and tracking versions across GPU/CPU caches.
#[derive(Default)]
pub struct TextureRegistry {
    textures: SlotMap<TextureId, PixelBufferHandle>,
}

impl TextureRegistry {
    pub fn new() -> Self {
        Self {
            textures: SlotMap::with_key(),
        }
    }

    /// Allocate a blank RGBA8 texture and return both a [`TextureHandle`] and its mutable [`PixelBufferHandle`].
    pub fn create_blank(&mut self, width: u32, height: u32) -> TextureHandle {
        let rgba = vec![0u8; (width as usize) * (height as usize) * 4];
        self.create_from_rgba(width, height, rgba)
    }

    /// Register an existing RGBA8 pixel buffer.
    pub fn create_from_rgba(&mut self, width: u32, height: u32, rgba: Vec<u8>) -> TextureHandle {
        let mut created_handle: Option<PixelBufferHandle> = None;
        let id = self.textures.insert_with_key(|id| {
            let handle = PixelBufferHandle::new(id, width, height, rgba);
            created_handle = Some(handle.clone());
            handle
        });
        TextureHandle {
            id,
            buffer: created_handle.expect("created handle"),
        }
    }

    /// Decode PNG bytes into a registered texture.
    pub fn create_from_png(&mut self, bytes: &[u8]) -> craftui_core::Result<TextureHandle> {
        let mut decoder = png::Decoder::new(std::io::Cursor::new(bytes));
        decoder.set_transformations(
            png::Transformations::normalize_to_color8() | png::Transformations::ALPHA,
        );
        let mut reader = decoder
            .read_info()
            .map_err(|e| craftui_core::CraftError::Render(e.to_string()))?;
        let mut buf = vec![0u8; reader.output_buffer_size().unwrap_or(0)];
        let info = reader
            .next_frame(&mut buf)
            .map_err(|e| craftui_core::CraftError::Render(e.to_string()))?;
        buf.truncate(info.buffer_size());
        Ok(self.create_from_rgba(info.width, info.height, buf))
    }

    pub fn get(&self, id: TextureId) -> Option<&PixelBufferHandle> {
        self.textures.get(id)
    }

    pub fn remove(&mut self, id: TextureId) -> Option<PixelBufferHandle> {
        self.textures.remove(id)
    }
}
