use crate::id3::{Tag, TagLike};
use crate::{Result, XmError};

// Conditional compilation: use wasmer on native targets, pure Rust on wasm32
#[cfg(not(target_arch = "wasm32"))]
use wasmer::{imports, Instance, Module, Store, Value};

#[cfg(not(target_arch = "wasm32"))]
use wasmer_compiler_cranelift::Cranelift;

#[cfg(target_arch = "wasm32")]
use crate::xm_algorithm;

const XM_KEY: &[u8] = "ximalayaximalayaximalayaximalaya".as_bytes();

#[cfg(not(target_arch = "wasm32"))]
const XM_WASM: &[u8] = include_bytes!("xm.wasm");

// Fix for macOS linking error with wasmer: ___rust_probestack symbol missing
#[cfg(all(target_os = "macos", target_arch = "x86_64"))]
#[no_mangle]
pub extern "C" fn ___rust_probestack() {}

/// Extract XM metadata from file
pub fn extract_xm_info(reader: impl std::io::Read) -> Result<XmInfo> {
    Tag::read_from(reader)
        .map(|t| t.into())
        .map_err(|e| XmError::Id3Error(e.to_string()).into())
}

/// Decrypt XM encrypted content
pub fn decrypt(xm_info: &XmInfo, content: &[u8]) -> Result<Vec<u8>> {
    // Extract encrypted portion
    let encrypted_data = &content[xm_info.header_size..xm_info.header_size + xm_info.size];
    
    // Decrypt only the encrypted chunk
    let decrypted_chunk = decrypt_chunk(xm_info, encrypted_data)?;
    
    // Append remaining unencrypted data
    let mut result = decrypted_chunk;
    result.extend_from_slice(&content[xm_info.header_size + xm_info.size..]);
    
    Ok(result)
}

/// Decrypt only the encrypted chunk (optimized for streaming)
/// 
/// This function only needs the encrypted chunk, not the full file.
/// This enables true streaming decryption with minimal memory usage.
pub fn decrypt_chunk(xm_info: &XmInfo, encrypted_chunk: &[u8]) -> Result<Vec<u8>> {
    // Get IV from metadata
    let iv = xm_info.iv()?;
    
    // Decrypt with AES-256-CBC
    let decrypted_data = aes_util::decrypt(encrypted_chunk, XM_KEY, &iv)?;
    let decrypted_str = String::from_utf8(decrypted_data)
        .map_err(|e| XmError::DecryptionError(format!("Invalid UTF-8: {}", e)))?;

    let track_id = format!("{}", xm_info.tracknumber);

    // Process with XM algorithm - conditional compilation based on target
    #[cfg(not(target_arch = "wasm32"))]
    let result_data = decrypt_chunk_native(&decrypted_str, &track_id)?;
    
    #[cfg(target_arch = "wasm32")]
    let result_data = decrypt_chunk_wasm32(&decrypted_str, &track_id)?;
    
    // Combine with encoding technology prefix and decode base64
    let full_base64 = format!(
        "{}{}",
        xm_info.encoding_technology.clone().unwrap_or_default(),
        result_data
    );

    let decoded_data = base64_util::decode(full_base64)?;
    
    Ok(decoded_data)
}

/// Native target implementation using wasmer runtime
#[cfg(not(target_arch = "wasm32"))]
fn decrypt_chunk_native(decrypted_str: &str, track_id: &str) -> Result<String> {
    // Initialize WASM runtime for XM algorithm
    let compiler = Cranelift::new();
    let mut store = Store::new(compiler);
    
    let module = Module::from_binary(&store, XM_WASM)
        .map_err(|e| XmError::WasmError(format!("Failed to load WASM module: {}", e)))?;
    let import_object = imports! {};
    let instance = Instance::new(&mut store, &module, &import_object)
        .map_err(|e| XmError::WasmError(format!("Failed to instantiate WASM: {}", e)))?;

    // Call WASM functions to process decrypted data
    let func_a = instance.exports.get_function("a")
        .map_err(|e| XmError::WasmError(format!("Function 'a' not found: {}", e)))?;
    let stack_pointer = func_a.call(&mut store, &[Value::I32(-16)])
        .map_err(|e| XmError::WasmError(format!("Failed to call function 'a': {}", e)))?[0].clone();

    let func_c = instance.exports.get_function("c")
        .map_err(|e| XmError::WasmError(format!("Function 'c' not found: {}", e)))?;
    let de_data_offset = func_c.call(&mut store, &[Value::I32(decrypted_str.len() as i32)])
        .map_err(|e| XmError::WasmError(format!("Failed to allocate de_data: {}", e)))?[0]
        .i32()
        .ok_or_else(|| XmError::WasmError("de_data_offset is None".into()))?;

    let track_id_offset = func_c.call(&mut store, &[Value::I32(track_id.len() as i32)])
        .map_err(|e| XmError::WasmError(format!("Failed to allocate track_id: {}", e)))?[0]
        .i32()
        .ok_or_else(|| XmError::WasmError("track_id_offset is None".into()))?;

    // Write data to WASM memory
    let memory_i = instance.exports.get_memory("i")
        .map_err(|e| XmError::WasmError(format!("Memory 'i' not found: {}", e)))?;
    {
        let view = memory_i.view(&store);
        for (i, b) in decrypted_str.bytes().enumerate() {
            view.write_u8(de_data_offset as u64 + i as u64, b)
                .map_err(|e| XmError::WasmError(format!("Failed to write de_data: {}", e)))?;
        }
        for (i, b) in track_id.bytes().enumerate() {
            view.write_u8(track_id_offset as u64 + i as u64, b)
                .map_err(|e| XmError::WasmError(format!("Failed to write track_id: {}", e)))?;
        }
    }

    // Call main processing function
    let func_g = instance.exports.get_function("g")
        .map_err(|e| XmError::WasmError(format!("Function 'g' not found: {}", e)))?;
    func_g.call(
        &mut store,
        &[
            stack_pointer.clone(),
            Value::I32(de_data_offset),
            Value::I32(decrypted_str.len() as i32),
            Value::I32(track_id_offset),
            Value::I32(track_id.len() as i32),
        ],
    ).map_err(|e| XmError::WasmError(format!("Failed to call function 'g': {}", e)))?;

    // Read result from WASM memory
    let view = memory_i.view(&store);
    let mut buf = [0; 4];
    view.read(
        stack_pointer.i32().ok_or_else(|| XmError::WasmError("stack_pointer is None".into()))? as u64,
        &mut buf,
    ).map_err(|e| XmError::WasmError(format!("Failed to read result pointer: {}", e)))?;
    let result_pointer = i32::from_le_bytes(buf);
    
    view.read(
        stack_pointer.i32().ok_or_else(|| XmError::WasmError("stack_pointer is None".into()))? as u64 + 4,
        &mut buf,
    ).map_err(|e| XmError::WasmError(format!("Failed to read result length: {}", e)))?;
    let result_length = i32::from_le_bytes(buf);

    let mem = view.copy_to_vec()
        .map_err(|e| XmError::WasmError(format!("Failed to copy memory: {}", e)))?;
    let result_data =
        &mem[result_pointer as usize..result_pointer as usize + result_length as usize];
    let result_data = String::from_utf8(result_data.to_vec())
        .map_err(|e| XmError::DecryptionError(format!("Invalid UTF-8 in result: {}", e)))?;
    
    Ok(result_data)
}

/// WASM32 target implementation using pure Rust algorithm
#[cfg(target_arch = "wasm32")]
fn decrypt_chunk_wasm32(decrypted_str: &str, track_id: &str) -> Result<String> {
    // Use pure Rust implementation for wasm32 targets
    xm_algorithm::xm_decrypt_algorithm(decrypted_str, track_id)
}

/// XM file metadata
#[derive(Debug, Default, Clone)]
pub struct XmInfo {
    pub title: Option<String>,
    pub artist: Option<String>,
    pub album: Option<String>,
    pub tracknumber: u64,
    pub size: usize,
    pub header_size: usize,
    pub isrc: Option<String>,
    pub encodedby: Option<String>,
    pub encoding_technology: Option<String>,
    pub has_cover: bool,
    pub cover_url: Option<String>,
    pub duration: Option<f64>,
}

impl XmInfo {
    /// Get book name from album field
    /// 
    /// In XM files, the album field typically contains the book/audiobook name
    pub fn book_name(&self) -> Option<&str> {
        self.album.as_deref()
    }

    /// Get chapter title from title field
    pub fn chapter_title(&self) -> Option<&str> {
        self.title.as_deref()
    }

    /// Get narrator/artist name
    pub fn narrator(&self) -> Option<&str> {
        self.artist.as_deref()
    }

    /// Get chapter number
    pub fn chapter_number(&self) -> u64 {
        self.tracknumber
    }
}

impl From<Tag> for XmInfo {
    fn from(value: Tag) -> Self {
        Self {
            title: value
                .get("TIT2")
                .map(|f| f.content().text().unwrap_or_default().to_string()),
            artist: value
                .get("TPE1")
                .map(|f| f.content().text().unwrap_or_default().to_string()),
            album: value
                .get("TALB")
                .map(|f| f.content().text().unwrap_or_default().to_string()),
            tracknumber: value
                .get("TRCK")
                .map(|f| f.content().text().unwrap_or("0").parse().unwrap_or(0))
                .unwrap_or(0),
            size: value
                .get("TSIZ")
                .map(|f| f.content().text().unwrap_or("0").parse().unwrap_or(0))
                .unwrap_or(0),
            header_size: value.header_tag_size() as usize,
            isrc: value
                .get("TSRC")
                .map(|f| f.content().text().unwrap_or_default().to_string()),
            encodedby: value
                .get("TENC")
                .map(|f| f.content().text().unwrap_or_default().to_string()),
            encoding_technology: value
                .get("TSSE")
                .map(|f| f.content().text().unwrap_or_default().to_string()),
            has_cover: value.pictures().count() > 0,
            cover_url: value.comments()
                .find(|c| c.text.contains("image") && (c.text.ends_with(".jpg") || c.text.ends_with(".png") || c.text.ends_with(".jpeg")))
                .map(|c| {
                    if c.text.starts_with("//") {
                        format!("https:{}", c.text)
                    } else {
                        c.text.clone()
                    }
                }),
            duration: value
                .get("TLEN")
                .or_else(|| value.get("TDLEN")) // Fallback to TDLEN
                .and_then(|f| f.content().text())
                .and_then(|s| {
                    // Try parsing directly
                    s.parse::<f64>().ok().or_else(|| {
                        // Fallback: remove non-numeric characters (e.g. "ms")
                        let clean: String = s.chars().filter(|c| c.is_ascii_digit() || *c == '.').collect();
                        clean.parse::<f64>().ok()
                    })
                })
                .map(|val| {
                    // XM files often store duration in seconds in TLEN, contrary to ID3 spec (ms)
                    // If value is small (< 3600), assume seconds. If large, assume ms.
                    if val < 36000.0 { // Less than 10 hours (if interpreted as seconds) or 36s (if ms)
                         // Wait, 418s = 7min. 418ms = 0.4s.
                         // If we assume it's seconds, 418 -> 418s.
                         // If we assume it's ms, 418 -> 0.418s.
                         // A chapter is usually > 1 min.
                         // So if val < 1000, it's almost certainly seconds (unless it's a very short sound effect).
                         // Let's use a threshold. 
                         // If val > 10000, assume ms (10s).
                         // If val <= 10000, assume seconds.
                         if val > 10000.0 {
                             val / 1000.0
                         } else {
                             val
                         }
                    } else {
                        // Very large value, assume ms
                        val / 1000.0
                    }
                }), // TLEN/TDLEN handling
         }
    }
}

impl XmInfo {
    /// Get IV (initialization vector) from metadata
    pub fn iv(&self) -> Result<Vec<u8>> {
        if let Some(isrc) = &self.isrc {
            hex::decode(isrc).map_err(|_| XmError::DecryptionError("Invalid IV in ISRC".into()).into())
        } else if let Some(encodedby) = &self.encodedby {
            hex::decode(encodedby).map_err(|_| XmError::DecryptionError("Invalid IV in TENC".into()).into())
        } else {
            Err(XmError::MissingMetadata("No IV found in ISRC or TENC tags".into()).into())
        }
    }

    /// Generate output filename from metadata and file header
    pub fn file_name(&self, header: &[u8]) -> String {
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
        } else {
            "m4a"
        };

        format!(
            "{} - {} - {}.{}",
            self.artist.clone().unwrap_or_default(),
            self.album.clone().unwrap_or_default(),
            self.title.clone().unwrap_or_default(),
            ext_name
        )
        .replace(['\\', ':', '/', '*', '?', '\"', '<', '>', '|'], "")
    }
}

mod aes_util {
    use crate::{Result, XmError};
    use aes::cipher::block_padding::Pkcs7;
    use aes::cipher::{BlockDecryptMut, KeyIvInit};

    type Aes256CbcDec = cbc::Decryptor<aes::Aes256>;

    pub(super) fn decrypt(ciphertext: &[u8], key: &[u8], iv: &[u8]) -> Result<Vec<u8>> {
        let cipher = Aes256CbcDec::new(key.into(), iv.into());
        let mut ct_v = ciphertext.to_vec();
        let ct_clone_mut = ct_v.as_mut_slice();
        cipher
            .decrypt_padded_mut::<Pkcs7>(ct_clone_mut)
            .map(|r| r.to_vec())
            .map_err(|_| XmError::DecryptionError("Failed to decrypt with AES-256-CBC".into()).into())
    }
}

mod base64_util {
    use crate::{Result, XmError};
    use base64::Engine;

    pub(super) fn decode(input: impl AsRef<[u8]>) -> Result<Vec<u8>> {
        base64::engine::general_purpose::STANDARD
            .decode(input)
            .map_err(|e| XmError::DecryptionError(format!("Base64 decode failed: {}", e)).into())
    }
}
