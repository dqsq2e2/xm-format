use crate::xm::{extract_xm_info, XmInfo};
use crate::Result;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

/// Audio metadata extractor
pub struct MetadataExtractor;

impl MetadataExtractor {
    /// Extract metadata from XM file
    pub fn extract(file_path: &Path) -> Result<AudioMetadata> {
        let mut file = File::open(file_path)?;
        let xm_info = extract_xm_info(&mut file)?;

        // Read file header to detect audio format
        file.seek(SeekFrom::Start(0))?;
        let mut header = vec![0u8; 512];
        file.read_exact(&mut header)?;

        let format = Self::detect_audio_format(&header);

        // Clean titles
        let clean_album = Self::clean_book_title(xm_info.album.as_deref().unwrap_or(""));
        let clean_title = Self::clean_chapter_title(
            xm_info.title.as_deref().unwrap_or(""), 
            &clean_album
        );

        Ok(AudioMetadata {
            title: Some(clean_title),
            artist: None, // TPE1 in XM files is usually the Narrator, not Author
            narrator: xm_info.artist.clone(),
            album: Some(clean_album),
            track_number: Some(xm_info.tracknumber),
            format,
            duration: xm_info.duration, // Now available from ID3 tags
            bitrate: None,
            sample_rate: None,
            channels: None,
            encrypted_size: Some(xm_info.size),
            header_size: Some(xm_info.header_size),
            cover_url: xm_info.cover_url.clone(),
        })
    }

    fn clean_book_title(title: &str) -> String {
        // Support both ASCII pipe and CJK Unified Ideograph separator
        if let Some(idx) = title.find(|c| c == '|' || c == '丨') {
            title[..idx].trim().to_string()
        } else {
            title.trim().to_string()
        }
    }

    fn clean_chapter_title(title: &str, book_title: &str) -> String {
        let mut cleaned = title.trim();
        
        // 1. Remove book title prefix
        if !book_title.is_empty() && cleaned.starts_with(book_title) {
            cleaned = &cleaned[book_title.len()..].trim();
        }

        // 2. Remove Episode/第xx集 prefix
        // Manual parsing to avoid regex dependency
        // Check for "Episode" or "Ep"
        if let Some(rest) = cleaned.strip_prefix("Episode") {
            cleaned = rest.trim_start();
        } else if let Some(rest) = cleaned.strip_prefix("Ep") {
            cleaned = rest.trim_start();
        } else if let Some(rest) = cleaned.strip_prefix("第") {
            cleaned = rest.trim_start();
        }

        // Skip digits
        cleaned = cleaned.trim_start_matches(|c: char| c.is_ascii_digit());
        
        // Skip "集"
        if let Some(rest) = cleaned.strip_prefix("集") {
            cleaned = rest.trim_start();
        }

        // Skip separators
        cleaned = cleaned.trim_start_matches(|c: char| c == ':' || c == '-' || c == ' ' || c == '.');

        cleaned.to_string()
    }

    /// Extract metadata from decrypted audio file
    /// 
    /// This provides more complete metadata including duration, bitrate, etc.
    pub fn extract_from_decrypted(file_path: &Path) -> Result<AudioMetadata> {
        let mut file = File::open(file_path)?;
        let mut header = vec![0u8; 512];
        file.read_exact(&mut header)?;

        let format = Self::detect_audio_format(&header);

        // For now, return basic metadata
        // Full audio metadata extraction would require additional libraries like symphonia
        Ok(AudioMetadata {
            title: None,
            artist: None,
            album: None,
            track_number: None,
            format,
            duration: None,
            bitrate: None,
            sample_rate: None,
            channels: None,
            encrypted_size: None,
            header_size: None,
            cover_url: None,
            narrator: None,
        })
    }

    /// Detect audio format from file header
    fn detect_audio_format(header: &[u8]) -> AudioFormat {
        if header.len() < 4 {
            return AudioFormat::Unknown;
        }

        // MP3: ID3 tag or MPEG frame sync
        if header.starts_with(b"ID3") || (header[0] == 0xFF && (header[1] & 0xE0) == 0xE0) {
            return AudioFormat::Mp3;
        }

        // M4A/MP4: ftyp box
        if header.len() >= 8 && &header[4..8] == b"ftyp" {
            return AudioFormat::M4a;
        }

        // FLAC: fLaC signature
        if header.starts_with(b"fLaC") {
            return AudioFormat::Flac;
        }

        // WAV: RIFF header
        if header.starts_with(b"RIFF") && header.len() >= 12 && &header[8..12] == b"WAVE" {
            return AudioFormat::Wav;
        }

        // OGG: OggS signature
        if header.starts_with(b"OggS") {
            return AudioFormat::Ogg;
        }

        AudioFormat::Unknown
    }

    /// Get suggested output filename from XM metadata
    pub fn get_output_filename(file_path: &Path) -> Result<String> {
        let mut file = File::open(file_path)?;
        let xm_info = extract_xm_info(&mut file)?;

        // Read header to detect format
        file.seek(SeekFrom::Start(0))?;
        let mut header = vec![0u8; 512];
        file.read_exact(&mut header)?;

        Ok(Self::format_filename(&xm_info, &header))
    }

    /// Format filename from metadata
    fn format_filename(info: &XmInfo, header: &[u8]) -> String {
        let header_chars: Vec<u8> = header
            .iter()
            .filter(|b| (&&0x20u8..=&&0x7Eu8).contains(&b))
            .copied()
            .collect();
        let header_str = String::from_utf8(header_chars)
            .unwrap_or_default()
            .to_ascii_lowercase();

        // Detect audio format from header
        let ext_name = if header_str.contains("m4a") {
            "m4a"
        } else if header_str.contains("mp3") {
            "mp3"
        } else if header_str.contains("flac") {
            "flac"
        } else if header_str.contains("wav") {
            "wav"
        } else if header_str.contains("ogg") {
            "ogg"
        } else {
            "m4a"
        };

        // Build filename from metadata
        let artist = info.artist.clone().unwrap_or_else(|| "Unknown Artist".to_string());
        let album = info.album.clone().unwrap_or_else(|| "Unknown Album".to_string());
        let title = info.title.clone().unwrap_or_else(|| "Unknown Title".to_string());

        format!("{} - {} - {}.{}", artist, album, title, ext_name)
            .replace(['\\', ':', '/', '*', '?', '\"', '<', '>', '|'], "")
    }

    /// Get ID3 tag size from header bytes
    pub fn get_id3_size(header: &[u8]) -> Option<usize> {
        if header.len() < 10 {
            return None;
        }

        // Check for ID3 identifier
        if &header[0..3] != b"ID3" {
            return None;
        }

        // ID3v2 header:
        // 0-2: "ID3"
        // 3: Major version
        // 4: Minor version
        // 5: Flags
        // 6-9: Size (Synchsafe integer)

        let size_bytes = &header[6..10];
        let size = ((size_bytes[0] as usize) << 21) |
                   ((size_bytes[1] as usize) << 14) |
                   ((size_bytes[2] as usize) << 7) |
                   (size_bytes[3] as usize);
        
        // Header size (10 bytes) is NOT included in the size field
        Some(size + 10)
    }
}

/// Audio metadata structure
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct AudioMetadata {
    pub title: Option<String>,
    pub artist: Option<String>,
    pub album: Option<String>,
    pub track_number: Option<u64>,
    pub format: AudioFormat,
    pub duration: Option<f64>, // seconds
    pub bitrate: Option<u32>,
    pub sample_rate: Option<u32>,
    pub channels: Option<u8>,
    // XM-specific fields
    pub encrypted_size: Option<usize>,
    pub header_size: Option<usize>,
    pub cover_url: Option<String>,
    pub narrator: Option<String>,
}

/// Audio format enum
#[derive(Debug, Clone, Copy, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
pub enum AudioFormat {
    Mp3,
    M4a,
    Flac,
    Wav,
    Ogg,
    Unknown,
}

impl std::fmt::Display for AudioFormat {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AudioFormat::Mp3 => write!(f, "MP3"),
            AudioFormat::M4a => write!(f, "M4A"),
            AudioFormat::Flac => write!(f, "FLAC"),
            AudioFormat::Wav => write!(f, "WAV"),
            AudioFormat::Ogg => write!(f, "OGG"),
            AudioFormat::Unknown => write!(f, "Unknown"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_detect_audio_format() {
        // MP3 with ID3
        let mp3_header = b"ID3\x03\x00\x00\x00\x00\x00\x00";
        assert_eq!(MetadataExtractor::detect_audio_format(mp3_header), AudioFormat::Mp3);

        // MP3 with MPEG sync
        let mp3_sync = b"\xFF\xFB\x90\x00\x00\x00\x00\x00";
        assert_eq!(MetadataExtractor::detect_audio_format(mp3_sync), AudioFormat::Mp3);

        // M4A
        let m4a_header = b"\x00\x00\x00\x20ftypM4A ";
        assert_eq!(MetadataExtractor::detect_audio_format(m4a_header), AudioFormat::M4a);

        // FLAC
        let flac_header = b"fLaC\x00\x00\x00\x22";
        assert_eq!(MetadataExtractor::detect_audio_format(flac_header), AudioFormat::Flac);

        // WAV
        let wav_header = b"RIFF\x00\x00\x00\x00WAVEfmt ";
        assert_eq!(MetadataExtractor::detect_audio_format(wav_header), AudioFormat::Wav);

        // OGG
        let ogg_header = b"OggS\x00\x02\x00\x00";
        assert_eq!(MetadataExtractor::detect_audio_format(ogg_header), AudioFormat::Ogg);

        // Unknown
        let unknown_header = b"XXXX\x00\x00\x00\x00";
        assert_eq!(MetadataExtractor::detect_audio_format(unknown_header), AudioFormat::Unknown);
    }

    #[test]
    fn test_format_filename() {
        use crate::xm::XmInfo;

        let info = XmInfo {
            title: Some("Test Title".to_string()),
            artist: Some("Test Artist".to_string()),
            album: Some("Test Album".to_string()),
            ..Default::default()
        };

        let header = b"ftyp M4A";
        let filename = MetadataExtractor::format_filename(&info, header);
        assert_eq!(filename, "Test Artist - Test Album - Test Title.m4a");

        // Test with special characters
        let info = XmInfo {
            title: Some("Title: With/Special*Chars?".to_string()),
            artist: Some("Artist".to_string()),
            album: Some("Album".to_string()),
            ..Default::default()
        };
        let filename = MetadataExtractor::format_filename(&info, header);
        assert!(!filename.contains(':'));
        assert!(!filename.contains('/'));
        assert!(!filename.contains('*'));
        assert!(!filename.contains('?'));
    }
}
