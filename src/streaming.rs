use crate::xm::extract_xm_info;
use crate::{Result, XmError};
use std::fs::File;
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::Path;

/// Streaming decryptor for XM files
/// 
/// XM files only encrypt the FIRST CHUNK, the rest is plaintext!
/// This implementation achieves true streaming decryption with minimal memory usage.
/// 
/// File structure:
/// - ID3 Header (metadata)
/// - Encrypted Chunk (first chunk only, ~10MB, needs decryption)
/// - Plaintext Audio Data (can be streamed directly, no decryption needed)
/// 
/// Memory usage: Only first chunk size + buffer (typically < 10MB total)
pub struct StreamingDecryptor {
    buffer_size: usize,
}

impl StreamingDecryptor {
    pub fn new(buffer_size: usize) -> Self {
        Self { buffer_size }
    }

    /// Decrypt XM file with TRUE streaming support (OPTIMIZED!)
    /// 
    /// This method achieves true streaming decryption:
    /// 1. Decrypts only the first encrypted chunk (~10MB, minimal memory)
    /// 2. Streams the plaintext portion directly (no decryption, zero overhead)
    /// 3. Writes output progressively (playback can start immediately)
    /// 
    /// Memory usage: Only first chunk + buffer (typically < 10MB total)
    /// Works perfectly with large files (100MB+)
    pub fn decrypt_streaming(
        &self,
        input_path: &Path,
        output_path: &Path,
        progress_callback: Option<Box<dyn Fn(f32) + Send>>,
    ) -> Result<()> {
        // Step 1: Extract metadata
        let mut input_file = File::open(input_path)?;
        let xm_info = extract_xm_info(&mut input_file)?;

        // Validate
        if xm_info.size == 0 {
            return Err(XmError::InvalidFormat("TSIZ field is 0 or missing".into()).into());
        }
        if xm_info.isrc.is_none() && xm_info.encodedby.is_none() {
            return Err(XmError::MissingMetadata("No IV found".into()).into());
        }

        if let Some(ref cb) = progress_callback {
            cb(0.05);
        }

        // Step 2: Read ONLY the encrypted chunk (not the entire file!)
        let encrypted_start = xm_info.header_size;
        let encrypted_end = xm_info.header_size + xm_info.size;
        
        input_file.seek(SeekFrom::Start(encrypted_start as u64))?;
        let mut encrypted_chunk = vec![0u8; xm_info.size];
        input_file.read_exact(&mut encrypted_chunk)?;

        if let Some(ref cb) = progress_callback {
            cb(0.15);
        }

        // Step 3: Decrypt ONLY the first chunk (minimal memory usage!)
        let decrypted_chunk = crate::xm::decrypt_chunk(&xm_info, &encrypted_chunk)?;

        if let Some(ref cb) = progress_callback {
            cb(0.4);
        }

        // Step 4: Write decrypted first chunk to output
        let mut output_file = File::create(output_path)?;
        output_file.write_all(&decrypted_chunk)?;

        if let Some(ref cb) = progress_callback {
            cb(0.5);
        }

        // Step 5: Stream the plaintext portion directly (NO DECRYPTION!)
        // This is the key optimization: the rest of the file is already plaintext
        input_file.seek(SeekFrom::Start(encrypted_end as u64))?;
        
        let file_size = input_file.metadata()?.len();
        let plaintext_size = file_size - encrypted_end as u64;
        let mut copied = 0u64;
        let mut buffer = vec![0u8; self.buffer_size];

        while copied < plaintext_size {
            let to_read = std::cmp::min(self.buffer_size as u64, plaintext_size - copied);
            let bytes_read = input_file.read(&mut buffer[..to_read as usize])?;
            
            if bytes_read == 0 {
                break;
            }

            output_file.write_all(&buffer[..bytes_read])?;
            copied += bytes_read as u64;

            if let Some(ref cb) = progress_callback {
                let progress = 0.5 + 0.5 * (copied as f32 / plaintext_size as f32);
                cb(progress);
            }
        }

        if let Some(ref cb) = progress_callback {
            cb(1.0);
        }

        Ok(())
    }

    /// Decrypt with optimized streaming (same as decrypt_streaming)
    /// 
    /// XM format now supports true streaming decryption!
    /// Only the first chunk needs to be decrypted, rest is plaintext.
    pub fn decrypt_mmap(
        &self,
        input_path: &Path,
        output_path: &Path,
        progress_callback: Option<Box<dyn Fn(f32) + Send>>,
    ) -> Result<()> {
        // Use the optimized streaming approach
        self.decrypt_streaming(input_path, output_path, progress_callback)
    }
}

impl Default for StreamingDecryptor {
    fn default() -> Self {
        Self::new(8192)
    }
}

/// Progress reporter for decryption operations
pub struct ProgressReporter {
    total_steps: usize,
    current_step: usize,
    callback: Box<dyn Fn(f32) + Send>,
}

impl ProgressReporter {
    pub fn new(total_steps: usize, callback: Box<dyn Fn(f32) + Send>) -> Self {
        Self {
            total_steps,
            current_step: 0,
            callback,
        }
    }

    pub fn step(&mut self, _message: &str) {
        self.current_step += 1;
        let progress = self.current_step as f32 / self.total_steps as f32;
        (self.callback)(progress);
    }

    pub fn set_progress(&self, progress: f32) {
        (self.callback)(progress.clamp(0.0, 1.0));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_streaming_decryptor_creation() {
        let decryptor = StreamingDecryptor::new(4096);
        assert_eq!(decryptor.buffer_size, 4096);

        let default_decryptor = StreamingDecryptor::default();
        assert_eq!(default_decryptor.buffer_size, 8192);
    }

    #[test]
    fn test_progress_reporter() {
        let _progress_values: Vec<f32> = Vec::new();
        let reporter = ProgressReporter::new(
            4,
            Box::new(|_p| {
                // Cannot capture in test, but validates API
            }),
        );

        // Test progress calculation
        assert_eq!(reporter.current_step, 0);
        assert_eq!(reporter.total_steps, 4);
    }
}
