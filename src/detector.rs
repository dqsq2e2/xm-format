use crate::xm::extract_xm_info;
use crate::Result;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

/// XM format detector
pub struct XmDetector;

impl XmDetector {
    /// Detect if a file is in XM format
    /// 
    /// XM files are identified by:
    /// 1. Valid ID3v2 tag
    /// 2. TSIZ field (encrypted data size) > 0
    /// 3. TSRC or TENC field (IV for decryption)
    /// 4. Optional TSSE field (encoding technology)
    pub fn detect(file_path: &Path) -> Result<bool> {
        let mut file = File::open(file_path)?;
        
        // Check file size (must be at least large enough for ID3v2 header)
        let file_size = file.metadata()?.len();
        if file_size < 10 {
            return Ok(false);
        }

        // Check for ID3v2 header
        let mut header = [0u8; 10];
        file.read_exact(&mut header)?;
        
        if &header[0..3] != b"ID3" {
            return Ok(false);
        }

        // Reset to beginning for full parsing
        file.seek(SeekFrom::Start(0))?;

        // Try to extract XM info
        match extract_xm_info(file) {
            Ok(info) => Ok(Self::validate_xm_info(&info)),
            Err(_) => Ok(false),
        }
    }

    /// Validate XM metadata
    fn validate_xm_info(info: &crate::xm::XmInfo) -> bool {
        // Must have encrypted data size
        if info.size == 0 {
            return false;
        }

        // Must have IV (in TSRC or TENC)
        if info.isrc.is_none() && info.encodedby.is_none() {
            return false;
        }

        // Validate IV format (should be hex string)
        if let Some(ref isrc) = info.isrc {
            if hex::decode(isrc).is_err() {
                return false;
            }
        } else if let Some(ref encodedby) = info.encodedby {
            if hex::decode(encodedby).is_err() {
                return false;
            }
        }

        true
    }

    /// Validate file header
    /// 
    /// Performs additional validation beyond basic detection
    pub fn validate_file(file_path: &Path) -> Result<ValidationResult> {
        let file = File::open(file_path)?;
        let file_size = file.metadata()?.len();

        // Extract XM info
        let info = extract_xm_info(file)?;

        let mut issues = Vec::new();
        let mut warnings = Vec::new();

        // Check encrypted data size
        if info.size == 0 {
            issues.push("TSIZ field is 0 or missing".to_string());
        } else if info.header_size + info.size > file_size as usize {
            issues.push(format!(
                "Encrypted data size ({}) exceeds file size ({})",
                info.header_size + info.size,
                file_size
            ));
        }

        // Check IV
        if info.isrc.is_none() && info.encodedby.is_none() {
            issues.push("No IV found (TSRC or TENC missing)".to_string());
        } else {
            let iv_result = if let Some(ref isrc) = info.isrc {
                hex::decode(isrc)
            } else if let Some(ref encodedby) = info.encodedby {
                hex::decode(encodedby)
            } else {
                Err(hex::FromHexError::InvalidStringLength)
            };

            match iv_result {
                Ok(iv) => {
                    if iv.len() != 16 {
                        warnings.push(format!("IV length is {} bytes (expected 16)", iv.len()));
                    }
                }
                Err(_) => {
                    issues.push("IV is not valid hex string".to_string());
                }
            }
        }

        // Check metadata completeness
        if info.title.is_none() {
            warnings.push("Title (TIT2) is missing".to_string());
        }
        if info.artist.is_none() {
            warnings.push("Artist (TPE1) is missing".to_string());
        }
        if info.album.is_none() {
            warnings.push("Album (TALB) is missing".to_string());
        }
        if info.tracknumber == 0 {
            warnings.push("Track number (TRCK) is 0 or missing".to_string());
        }

        // Check for file corruption indicators
        if info.header_size < 10 {
            issues.push(format!("ID3 header size too small: {}", info.header_size));
        }
        if info.header_size > file_size as usize {
            issues.push(format!(
                "ID3 header size ({}) exceeds file size ({})",
                info.header_size, file_size
            ));
        }

        let is_valid = issues.is_empty();

        Ok(ValidationResult {
            is_valid,
            issues,
            warnings,
            metadata: info,
        })
    }

    /// Detect file corruption
    /// 
    /// Checks for common corruption indicators
    pub fn detect_corruption(file_path: &Path) -> Result<CorruptionReport> {
        let validation = Self::validate_file(file_path)?;
        
        let mut corruption_indicators = Vec::new();
        
        // Check for critical issues
        for issue in &validation.issues {
            if issue.contains("exceeds file size") {
                corruption_indicators.push("Data size mismatch".to_string());
            }
            if issue.contains("header size") {
                corruption_indicators.push("Invalid header size".to_string());
            }
            if issue.contains("not valid hex") {
                corruption_indicators.push("Corrupted IV data".to_string());
            }
        }

        let is_corrupted = !corruption_indicators.is_empty();

        Ok(CorruptionReport {
            is_corrupted,
            indicators: corruption_indicators,
            validation_result: validation,
        })
    }
}

/// Validation result
#[derive(Debug)]
pub struct ValidationResult {
    pub is_valid: bool,
    pub issues: Vec<String>,
    pub warnings: Vec<String>,
    pub metadata: crate::xm::XmInfo,
}

/// Corruption detection report
#[derive(Debug)]
pub struct CorruptionReport {
    pub is_corrupted: bool,
    pub indicators: Vec<String>,
    pub validation_result: ValidationResult,
}

impl ValidationResult {
    /// Check if file is valid XM format
    pub fn is_valid(&self) -> bool {
        self.is_valid
    }

    /// Get critical issues that prevent decryption
    pub fn issues(&self) -> &[String] {
        &self.issues
    }

    /// Get warnings (non-critical issues)
    pub fn warnings(&self) -> &[String] {
        &self.warnings
    }

    /// Get detailed error message
    pub fn error_message(&self) -> Option<String> {
        if self.issues.is_empty() {
            None
        } else {
            Some(format!("XM validation failed:\n{}", self.issues.join("\n")))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validate_xm_info() {
        use crate::xm::XmInfo;

        // Valid XM info
        let valid_info = XmInfo {
            size: 1000,
            isrc: Some("0123456789abcdef0123456789abcdef".to_string()),
            ..Default::default()
        };
        assert!(XmDetector::validate_xm_info(&valid_info));

        // Invalid: no size
        let invalid_info = XmInfo {
            size: 0,
            isrc: Some("0123456789abcdef0123456789abcdef".to_string()),
            ..Default::default()
        };
        assert!(!XmDetector::validate_xm_info(&invalid_info));

        // Invalid: no IV
        let invalid_info = XmInfo {
            size: 1000,
            isrc: None,
            encodedby: None,
            ..Default::default()
        };
        assert!(!XmDetector::validate_xm_info(&invalid_info));

        // Valid: IV in encodedby
        let valid_info = XmInfo {
            size: 1000,
            isrc: None,
            encodedby: Some("0123456789abcdef0123456789abcdef".to_string()),
            ..Default::default()
        };
        assert!(XmDetector::validate_xm_info(&valid_info));

        // Invalid: bad hex in IV
        let invalid_info = XmInfo {
            size: 1000,
            isrc: Some("not_hex_string".to_string()),
            ..Default::default()
        };
        assert!(!XmDetector::validate_xm_info(&invalid_info));
    }
}
