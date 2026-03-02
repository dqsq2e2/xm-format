pub mod id3;
pub mod xm;
pub mod xm_algorithm;
pub mod plugin;
pub mod c_api;
pub mod detector;
pub mod streaming;
pub mod metadata;

pub use plugin::XmFormatPlugin;
pub use xm_algorithm::is_xm_decryption_available;
pub use detector::{XmDetector, ValidationResult, CorruptionReport};
pub use streaming::{StreamingDecryptor, ProgressReporter};
pub use metadata::{MetadataExtractor, AudioMetadata, AudioFormat};

// Re-export the probestack symbol to satisfy the linker if needed
// This is a last-resort workaround for cross-compilation issues
#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
#[no_mangle]
pub extern "C" fn __rust_probestack() {}

pub type Result<T> = std::result::Result<T, Box<dyn std::error::Error + Send + Sync>>;

#[derive(Debug, thiserror::Error)]
pub enum XmError {
    #[error("Invalid XM file format: {0}")]
    InvalidFormat(String),
    
    #[error("Missing metadata: {0}")]
    MissingMetadata(String),
    
    #[error("Decryption failed: {0}")]
    DecryptionError(String),
    
    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),
    
    #[error("ID3 tag error: {0}")]
    Id3Error(String),
    
    #[error("WASM runtime error: {0}")]
    WasmError(String),
    
    #[error("File corrupted: {0}")]
    FileCorrupted(String),
    
    #[error("Unsupported format: {0}")]
    UnsupportedFormat(String),
}

impl XmError {
    /// Get error code for API responses
    pub fn error_code(&self) -> &'static str {
        match self {
            XmError::InvalidFormat(_) => "INVALID_FORMAT",
            XmError::MissingMetadata(_) => "MISSING_METADATA",
            XmError::DecryptionError(_) => "DECRYPTION_FAILED",
            XmError::IoError(_) => "IO_ERROR",
            XmError::Id3Error(_) => "ID3_ERROR",
            XmError::WasmError(_) => "WASM_ERROR",
            XmError::FileCorrupted(_) => "FILE_CORRUPTED",
            XmError::UnsupportedFormat(_) => "UNSUPPORTED_FORMAT",
        }
    }

    /// Get user-friendly error message
    pub fn user_message(&self) -> String {
        match self {
            XmError::InvalidFormat(msg) => format!("文件格式无效: {}", msg),
            XmError::MissingMetadata(msg) => format!("缺少必要的元数据: {}", msg),
            XmError::DecryptionError(msg) => format!("解密失败: {}", msg),
            XmError::IoError(e) => format!("文件读写错误: {}", e),
            XmError::Id3Error(msg) => format!("ID3 标签解析错误: {}", msg),
            XmError::WasmError(msg) => format!("WASM 运行时错误: {}", msg),
            XmError::FileCorrupted(msg) => format!("文件已损坏: {}", msg),
            XmError::UnsupportedFormat(msg) => format!("不支持的格式: {}", msg),
        }
    }

    /// Check if error is recoverable
    pub fn is_recoverable(&self) -> bool {
        matches!(self, XmError::IoError(_))
    }
}
