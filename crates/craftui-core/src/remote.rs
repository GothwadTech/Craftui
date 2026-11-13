//! Remote preview protocol data types shared between CraftUI CLI and device runtimes.

use serde::{Deserialize, Serialize};

/// Default TCP port used for CraftUI remote live preview broadcast and ADB reverse tunnels.
pub const DEFAULT_REMOTE_PREVIEW_PORT: u16 = 9876;

/// Message exchanged over the line-delimited JSON stream between host preview server and client devices.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum RemotePreviewMessage {
    /// Host -> Device: Push source code to hot-reload in memory.
    Reload {
        path: String,
        source: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        component: Option<String>,
        timestamp: u64,
    },
    /// Ping message for connection heartbeat.
    Ping,
    /// Pong response for connection heartbeat.
    Pong,
    /// Device -> Host: Handshake emitted upon establishing connection.
    Connected {
        device: String,
        os: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        screen_width: Option<f32>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        screen_height: Option<f32>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        scale_factor: Option<f32>,
    },
    /// Device -> Host: Successful compilation and mount confirmation with performance metrics.
    Ack {
        path: String,
        status: String,
        duration_ms: f64,
    },
    /// Device -> Host: Compilation, syntax, or runtime evaluation failure report.
    Error { path: String, message: String },
}

impl RemotePreviewMessage {
    /// Serialize message to a single line JSON string with newline delimiter.
    pub fn to_line(&self) -> Result<String, serde_json::Error> {
        let mut s = serde_json::to_string(self)?;
        s.push('\n');
        Ok(s)
    }

    /// Parse a single line JSON string into a [`RemotePreviewMessage`].
    pub fn from_line(line: &str) -> Result<Self, serde_json::Error> {
        serde_json::from_str(line.trim())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_remote_message_serialization_roundtrip() {
        let msg = RemotePreviewMessage::Reload {
            path: "ui/main.cui".into(),
            source: "component App { Text { text: \"Hello\" } }".into(),
            component: Some("App".into()),
            timestamp: 1700000000,
        };
        let line = msg.to_line().expect("must serialize");
        assert!(line.ends_with('\n'));
        let parsed = RemotePreviewMessage::from_line(&line).expect("must parse");
        assert_eq!(msg, parsed);

        let ack = RemotePreviewMessage::Ack {
            path: "ui/main.cui".into(),
            status: "ok".into(),
            duration_ms: 3.45,
        };
        let line_ack = ack.to_line().expect("must serialize");
        let parsed_ack = RemotePreviewMessage::from_line(&line_ack).expect("must parse");
        assert_eq!(ack, parsed_ack);
    }
}
