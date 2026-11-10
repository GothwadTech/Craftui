use thiserror::Error;

/// Unified error type across CraftUI crates.
#[derive(Debug, Error)]
pub enum CraftError {
    #[error("layout error: {0}")]
    Layout(String),
    #[error("render error: {0}")]
    Render(String),
    #[error("text shaping error: {0}")]
    Text(String),
    #[error("dsl error: {0}")]
    Dsl(String),
    #[error("runtime error: {0}")]
    Runtime(String),
    #[error("platform error: {0}")]
    Platform(String),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
}

pub type Result<T> = std::result::Result<T, CraftError>;
