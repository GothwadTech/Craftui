use crate::UiDispatcher;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

/// File extension filter for native file open/save dialogs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileFilter {
    pub name: String,
    pub extensions: Vec<String>,
}

impl FileFilter {
    pub fn new(
        name: impl Into<String>,
        extensions: impl IntoIterator<Item = impl Into<String>>,
    ) -> Self {
        Self {
            name: name.into(),
            extensions: extensions.into_iter().map(Into::into).collect(),
        }
    }
}

/// Request parameters for native file open / save / folder selection dialogs.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct FileDialogRequest {
    pub title: String,
    pub default_directory: Option<PathBuf>,
    pub default_name: Option<String>,
    pub filters: Vec<FileFilter>,
    pub multiple: bool,
    pub directory: bool,
}

impl FileDialogRequest {
    pub fn open(title: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            ..Default::default()
        }
    }

    pub fn save(title: impl Into<String>, default_name: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            default_name: Some(default_name.into()),
            ..Default::default()
        }
    }

    pub fn with_filter(mut self, filter: FileFilter) -> Self {
        self.filters.push(filter);
        self
    }

    pub fn with_multiple(mut self, multiple: bool) -> Self {
        self.multiple = multiple;
        self
    }

    pub fn with_directory(mut self, directory: bool) -> Self {
        self.directory = directory;
        self
    }
}

/// Cross-platform OS clipboard abstraction implemented per platform shell.
pub trait ClipboardBackend: Send {
    fn get_text(&mut self) -> Option<String>;
    fn set_text(&mut self, text: &str) -> bool;
}

/// Default in-memory clipboard fallback used in headless tests or unsupported hosts.
#[derive(Debug, Default)]
pub struct MemoryClipboard {
    text: String,
}

impl MemoryClipboard {
    pub fn new(initial: impl Into<String>) -> Self {
        Self {
            text: initial.into(),
        }
    }
}

impl ClipboardBackend for MemoryClipboard {
    fn get_text(&mut self) -> Option<String> {
        Some(self.text.clone())
    }

    fn set_text(&mut self, text: &str) -> bool {
        self.text.clear();
        self.text.push_str(text);
        true
    }
}

/// Cross-platform native file picker abstraction implemented per platform shell.
pub trait FileDialogBackend: Send + Sync {
    fn open_files(&self, request: &FileDialogRequest) -> Vec<PathBuf>;
    fn save_file(&self, request: &FileDialogRequest) -> Option<PathBuf>;
}

/// Headless fallback file dialog backend returning no selection unless overridden.
#[derive(Debug, Default)]
pub struct NoopFileDialog;

impl FileDialogBackend for NoopFileDialog {
    fn open_files(&self, _request: &FileDialogRequest) -> Vec<PathBuf> {
        Vec::new()
    }

    fn save_file(&self, _request: &FileDialogRequest) -> Option<PathBuf> {
        None
    }
}

/// Deterministic mock file dialog backend useful in unit/integration tests.
#[derive(Debug, Default)]
pub struct MockFileDialog {
    pub open_result: Mutex<Vec<PathBuf>>,
    pub save_result: Mutex<Option<PathBuf>>,
}

impl MockFileDialog {
    pub fn new(open_files: Vec<PathBuf>, save_file: Option<PathBuf>) -> Self {
        Self {
            open_result: Mutex::new(open_files),
            save_result: Mutex::new(save_file),
        }
    }
}

impl FileDialogBackend for MockFileDialog {
    fn open_files(&self, _request: &FileDialogRequest) -> Vec<PathBuf> {
        self.open_result
            .lock()
            .map(|g| g.clone())
            .unwrap_or_default()
    }

    fn save_file(&self, _request: &FileDialogRequest) -> Option<PathBuf> {
        self.save_result.lock().ok().and_then(|g| g.clone())
    }
}

/// Bundle of OS/platform service backends shared via reactive context or `WidgetTree`.
#[derive(Clone)]
pub struct PlatformServices {
    clipboard: Arc<Mutex<Box<dyn ClipboardBackend>>>,
    file_dialog: Arc<dyn FileDialogBackend>,
}

impl Default for PlatformServices {
    fn default() -> Self {
        Self::in_memory()
    }
}

impl PlatformServices {
    pub fn in_memory() -> Self {
        Self {
            clipboard: Arc::new(Mutex::new(Box::new(MemoryClipboard::default()))),
            file_dialog: Arc::new(NoopFileDialog),
        }
    }

    pub fn new(
        clipboard: impl ClipboardBackend + 'static,
        file_dialog: impl FileDialogBackend + 'static,
    ) -> Self {
        Self {
            clipboard: Arc::new(Mutex::new(Box::new(clipboard))),
            file_dialog: Arc::new(file_dialog),
        }
    }

    pub fn with_clipboard(mut self, clipboard: impl ClipboardBackend + 'static) -> Self {
        self.clipboard = Arc::new(Mutex::new(Box::new(clipboard)));
        self
    }

    pub fn with_file_dialog(mut self, file_dialog: impl FileDialogBackend + 'static) -> Self {
        self.file_dialog = Arc::new(file_dialog);
        self
    }

    pub fn read_clipboard(&self) -> Option<String> {
        self.clipboard.lock().ok().and_then(|mut cb| cb.get_text())
    }

    pub fn write_clipboard(&self, text: &str) -> bool {
        self.clipboard
            .lock()
            .map(|mut cb| cb.set_text(text))
            .unwrap_or(false)
    }

    pub fn pick_open_files(&self, request: &FileDialogRequest) -> Vec<PathBuf> {
        self.file_dialog.open_files(request)
    }

    pub fn pick_open_file(&self, request: &FileDialogRequest) -> Option<PathBuf> {
        self.file_dialog.open_files(request).into_iter().next()
    }

    pub fn pick_save_file(&self, request: &FileDialogRequest) -> Option<PathBuf> {
        self.file_dialog.save_file(request)
    }

    /// Non-blocking file open dialog executed on a worker thread (native) and delivered onto `dispatcher`.
    pub fn pick_open_files_async<F>(
        &self,
        request: FileDialogRequest,
        dispatcher: UiDispatcher,
        on_done: F,
    ) where
        F: FnOnce(Vec<PathBuf>) + Send + 'static,
    {
        let backend = self.file_dialog.clone();
        #[cfg(not(target_arch = "wasm32"))]
        {
            std::thread::spawn(move || {
                let paths = backend.open_files(&request);
                dispatcher.post(move || on_done(paths));
            });
        }
        #[cfg(target_arch = "wasm32")]
        {
            let paths = backend.open_files(&request);
            dispatcher.post(move || on_done(paths));
        }
    }

    /// Non-blocking single-file open dialog executed on a worker thread, delivering the result onto `dispatcher`.
    pub fn pick_open_file_async<F>(
        &self,
        request: FileDialogRequest,
        dispatcher: UiDispatcher,
        on_done: F,
    ) where
        F: FnOnce(Option<PathBuf>) + Send + 'static,
    {
        self.pick_open_files_async(request, dispatcher, move |paths| {
            on_done(paths.into_iter().next());
        });
    }

    /// Non-blocking save-file dialog executed on a worker thread (native) and delivered onto `dispatcher`.
    pub fn pick_save_file_async<F>(
        &self,
        request: FileDialogRequest,
        dispatcher: UiDispatcher,
        on_done: F,
    ) where
        F: FnOnce(Option<PathBuf>) + Send + 'static,
    {
        let backend = self.file_dialog.clone();
        #[cfg(not(target_arch = "wasm32"))]
        {
            std::thread::spawn(move || {
                let path = backend.save_file(&request);
                dispatcher.post(move || on_done(path));
            });
        }
        #[cfg(target_arch = "wasm32")]
        {
            let path = backend.save_file(&request);
            dispatcher.post(move || on_done(path));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn platform_services_clipboard_and_mock_dialog_work() {
        let services = PlatformServices::in_memory().with_file_dialog(MockFileDialog::new(
            vec![PathBuf::from("/tmp/example.txt")],
            Some(PathBuf::from("/tmp/out.png")),
        ));

        assert!(services.write_clipboard("hello CraftUI"));
        assert_eq!(services.read_clipboard().as_deref(), Some("hello CraftUI"));

        let opened = services.pick_open_file(&FileDialogRequest::open("Open File"));
        assert_eq!(opened, Some(PathBuf::from("/tmp/example.txt")));

        let saved = services.pick_save_file(&FileDialogRequest::save("Save Image", "out.png"));
        assert_eq!(saved, Some(PathBuf::from("/tmp/out.png")));

        let disp = UiDispatcher::new();
        let got_open = Arc::new(Mutex::new(None));
        let go_c = got_open.clone();
        services.pick_open_file_async(FileDialogRequest::open("Async Open"), disp.clone(), move |p| {
            *go_c.lock().unwrap() = Some(p);
        });
        for _ in 0..50 {
            let _ = disp.drain();
            if got_open.lock().unwrap().is_some() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
        assert_eq!(
            *got_open.lock().unwrap(),
            Some(Some(PathBuf::from("/tmp/example.txt")))
        );

        let got_save = Arc::new(Mutex::new(None));
        let gs_c = got_save.clone();
        services.pick_save_file_async(
            FileDialogRequest::save("Async Save", "out.png"),
            disp.clone(),
            move |p| {
                *gs_c.lock().unwrap() = Some(p);
            },
        );
        for _ in 0..50 {
            let _ = disp.drain();
            if got_save.lock().unwrap().is_some() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
        assert_eq!(
            *got_save.lock().unwrap(),
            Some(Some(PathBuf::from("/tmp/out.png")))
        );
    }
}
