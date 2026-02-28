use crate::detector::XmDetector;
use crate::metadata::{AudioMetadata, MetadataExtractor};
use crate::streaming::StreamingDecryptor;
use crate::xm::{decrypt, extract_xm_info};
use crate::{Result, XmError};
use serde::{Deserialize, Serialize};
use std::fs;
use std::io::{Read, Write};
use std::path::Path;

/// XM Format Plugin
/// 
/// This plugin implements the FormatPlugin interface for XM encrypted audio files.
/// It provides detection, decryption, and metadata extraction capabilities.
pub struct XmFormatPlugin {
    config: PluginConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginConfig {
    pub enable_streaming: bool,
    pub buffer_size: usize,
}

impl Default for PluginConfig {
    fn default() -> Self {
        Self {
            enable_streaming: true,  // Streaming is now optimized for XM format!
            buffer_size: 8192,
        }
    }
}

impl XmFormatPlugin {
    pub fn new(config: PluginConfig) -> Self {
        Self { config }
    }

    /// Detect if a file is XM format
    /// 
    /// XM files have ID3v2 tags with specific fields (TSIZ, TSRC/TENC, TSSE)
    pub fn detect(&self, file_path: &Path) -> Result<bool> {
        XmDetector::detect(file_path)
    }

    /// Decrypt XM file
    /// 
    /// Reads the encrypted XM file, extracts metadata, decrypts the content,
    /// and writes the decrypted audio to the output file.
    /// 
    /// If streaming is enabled in config, uses streaming decryption to reduce memory usage.
    pub fn decrypt_file(
        &self,
        input_path: &Path,
        output_path: &Path,
        progress_callback: Option<Box<dyn Fn(f32) + Send>>,
    ) -> Result<()> {
        if self.config.enable_streaming {
            // Use streaming decryptor
            let decryptor = StreamingDecryptor::new(self.config.buffer_size);
            decryptor.decrypt_streaming(input_path, output_path, progress_callback)
        } else {
            // Use in-memory decryption
            self.decrypt_in_memory(input_path, output_path, progress_callback)
        }
    }

    /// Decrypt file in memory (legacy method)
    fn decrypt_in_memory(
        &self,
        input_path: &Path,
        output_path: &Path,
        progress_callback: Option<Box<dyn Fn(f32) + Send>>,
    ) -> Result<()> {
        // Report initial progress
        if let Some(ref cb) = progress_callback {
            cb(0.0);
        }

        // Read input file
        let mut file = fs::File::open(input_path)?;
        let mut content = Vec::new();
        file.read_to_end(&mut content)?;

        if let Some(ref cb) = progress_callback {
            cb(0.2);
        }

        // Extract XM metadata
        let file = fs::File::open(input_path)?;
        let xm_info = extract_xm_info(file)?;

        if let Some(ref cb) = progress_callback {
            cb(0.3);
        }

        // Validate XM format
        if xm_info.size == 0 {
            return Err(XmError::InvalidFormat("TSIZ field is 0 or missing".into()).into());
        }
        if xm_info.isrc.is_none() && xm_info.encodedby.is_none() {
            return Err(XmError::MissingMetadata("No IV found (TSRC or TENC missing)".into()).into());
        }

        if let Some(ref cb) = progress_callback {
            cb(0.4);
        }

        // Decrypt content
        let decrypted_data = decrypt(&xm_info, &content)?;

        if let Some(ref cb) = progress_callback {
            cb(0.8);
        }

        // Write output file
        let mut output_file = fs::File::create(output_path)?;
        output_file.write_all(&decrypted_data)?;

        if let Some(ref cb) = progress_callback {
            cb(1.0);
        }

        Ok(())
    }

    /// Extract metadata from XM file
    pub fn extract_metadata(&self, file_path: &Path) -> Result<AudioMetadata> {
        MetadataExtractor::extract(file_path)
    }

    /// Get suggested output filename
    pub fn get_output_filename(&self, file_path: &Path) -> Result<String> {
        MetadataExtractor::get_output_filename(file_path)
    }

    /// Extract XM ID3 metadata for scraping purposes
    /// 
    /// This method extracts metadata from XM file's ID3 header without decryption.
    /// Useful for scraper plugins to get book/chapter information.
    /// 
    /// Returns XmInfo containing:
    /// - title: Chapter/track title (TIT2)
    /// - artist: Narrator/artist (TPE1)
    /// - album: Book/album name (TALB)
    /// - tracknumber: Chapter number (TRCK)
    pub fn extract_id3_metadata(&self, file_path: &Path) -> Result<crate::xm::XmInfo> {
        let file = std::fs::File::open(file_path)?;
        crate::xm::extract_xm_info(file)
    }

    /// Get required read size for metadata extraction
    pub fn get_metadata_read_size(&self, header_probe: &[u8]) -> Option<usize> {
        MetadataExtractor::get_id3_size(header_probe)
    }

    /// Get decryption plan for streaming
    pub fn get_decryption_plan(&self, header_probe: &[u8]) -> Result<DecryptionPlan> {
        // Parse ID3 header from probe data
        // We use a Cursor to simulate a reader
        let cursor = std::io::Cursor::new(header_probe);
        let xm_info = crate::xm::extract_xm_info(cursor)?;
        
        let mut segments = Vec::new();
        
        // 1. Encrypted segment
        // Starts at header_size, length is xm_info.size
        // We need to pass IV and other params for decryption
        let iv = xm_info.iv()?;
        let iv_hex = hex::encode(iv);
        
        let params = serde_json::json!({
            "iv": iv_hex,
            "track_number": xm_info.tracknumber,
            "encoding_technology": xm_info.encoding_technology
        });
        
        segments.push(DecryptionSegment::Encrypted {
            offset: xm_info.header_size as u64,
            length: xm_info.size as i64,
            params
        });
        
        // 2. Plain segment (remaining content)
        // Starts after encrypted segment, length -1 (until end)
        segments.push(DecryptionSegment::Plain {
            offset: (xm_info.header_size + xm_info.size) as u64,
            length: -1
        });
        
        Ok(DecryptionPlan {
            segments,
            total_size: None // We don't know total size from header only
        })
    }

    /// Decrypt a chunk of data in memory
    /// 
    /// Used for streaming decryption of the encrypted segment
    pub fn decrypt_chunk_data(&self, encrypted_data: &[u8], params: &serde_json::Value) -> Result<Vec<u8>> {
        // Reconstruct XmInfo from params
        // We only need IV and encoding_technology for decrypt_chunk
        let iv_hex = params["iv"].as_str().ok_or_else(|| XmError::MissingMetadata("Missing IV".into()))?;
        let track_number = params["track_number"].as_u64().unwrap_or(0);
        let encoding_technology = params["encoding_technology"].as_str().map(|s| s.to_string());
        
        let mut xm_info = crate::xm::XmInfo::default();
        xm_info.isrc = Some(iv_hex.to_string()); // decrypt_chunk uses iv() which checks isrc
        xm_info.tracknumber = track_number;
        xm_info.encoding_technology = encoding_technology;
        
        crate::xm::decrypt_chunk(&xm_info, encrypted_data)
    }
}

/// Plan for decrypting a file stream
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DecryptionPlan {
    /// Segments of the file to process
    pub segments: Vec<DecryptionSegment>,
    /// Total size of the output stream (if known)
    pub total_size: Option<u64>,
}

/// A segment of the decryption plan
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum DecryptionSegment {
    /// Plain text segment (direct copy)
    #[serde(rename = "plain")]
    Plain { 
        /// Start offset in the source file
        offset: u64, 
        /// Length of the segment (-1 or 0 for "until end")
        length: i64 
    },
    
    /// Encrypted segment (needs decryption)
    #[serde(rename = "encrypted")]
    Encrypted { 
        /// Start offset in the source file
        offset: u64, 
        /// Length of the segment
        length: i64,
        /// Parameters for decryption (passed to decrypt_chunk)
        params: serde_json::Value 
    },
}

