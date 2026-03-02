use std::ffi::{CStr, CString};
use std::os::raw::{c_char, c_int};
use std::sync::{Arc, Mutex};
use lazy_static::lazy_static;
use serde_json::Value;
use base64::Engine;
#[cfg(target_os = "linux")]
use libc;

use crate::plugin::{XmFormatPlugin, PluginConfig};

// Global plugin instance
lazy_static! {
    static ref PLUGIN: Arc<Mutex<Option<XmFormatPlugin>>> = Arc::new(Mutex::new(None));
    static ref INIT_COUNT: Arc<Mutex<u32>> = Arc::new(Mutex::new(0));
}

// Memory tracking helper
fn log_memory_usage(tag: &str) {
    #[cfg(target_os = "linux")]
    {
        if let Ok(contents) = std::fs::read_to_string("/proc/self/statm") {
            let parts: Vec<&str> = contents.split_whitespace().collect();
            if parts.len() >= 2 {
                let rss_pages = parts[1].parse::<usize>().unwrap_or(0);
                let rss_mb = (rss_pages * 4) / 1024; // Assuming 4KB pages
                println!("[xm-format] Memory [{}]: RSS={} MB", tag, rss_mb);
            }
        }
    }
}

/// Initialize the plugin
fn initialize(params: Value) -> Result<Value, String> {
    log_memory_usage("pre-init");
    let mut count = INIT_COUNT.lock().map_err(|e| format!("Failed to acquire lock: {}", e))?;
    *count += 1;
    println!("[xm-format] Initialize called. Count: {}", *count);

    let config_json = params.get("config").unwrap_or(&serde_json::json!({})).clone();
    
    // Parse configuration
    let config: PluginConfig = serde_json::from_value(config_json)
        .map_err(|e| format!("Invalid configuration: {}", e))?;
        
    let plugin = XmFormatPlugin::new(config);
    
    // Store global instance
    let mut instance = PLUGIN.lock().map_err(|e| format!("Failed to acquire lock: {}", e))?;
    *instance = Some(plugin);
    
    log_memory_usage("post-init");
    Ok(serde_json::json!({"status": "initialized"}))
}

/// Shutdown the plugin
fn shutdown(_params: Value) -> Result<Value, String> {
    log_memory_usage("pre-shutdown");
    let mut instance = PLUGIN.lock().map_err(|e| format!("Failed to acquire lock: {}", e))?;
    *instance = None;
    
    // Force cleanup
    #[cfg(target_os = "linux")]
    unsafe {
        let _ = libc::malloc_trim(0);
    }
    
    log_memory_usage("post-shutdown");
    Ok(serde_json::json!({"status": "shutdown"}))
}

/// Detect format
fn detect(params: Value) -> Result<Value, String> {
    let file_path_str = params["file_path"]
        .as_str()
        .ok_or("Missing 'file_path' parameter")?;
        
    let file_path = std::path::Path::new(file_path_str);
    
    let instance = PLUGIN.lock().map_err(|e| format!("Failed to acquire lock: {}", e))?;
    if let Some(plugin) = instance.as_ref() {
        let is_xm = plugin.detect(file_path)
            .map_err(|e| e.to_string())?;
            
        Ok(serde_json::json!({"is_xm": is_xm}))
    } else {
        Err("Plugin not initialized".to_string())
    }
}

/// Decrypt file
fn decrypt(params: Value) -> Result<Value, String> {
    let input_path_str = params["input_path"]
        .as_str()
        .ok_or("Missing 'input_path' parameter")?;
        
    let output_path_str = params["output_path"]
        .as_str()
        .ok_or("Missing 'output_path' parameter")?;
        
    let input_path = std::path::Path::new(input_path_str);
    let output_path = std::path::Path::new(output_path_str);
    
    let instance = PLUGIN.lock().map_err(|e| format!("Failed to acquire lock: {}", e))?;
    if let Some(plugin) = instance.as_ref() {
        plugin.decrypt_file(input_path, output_path, None)
            .map_err(|e| e.to_string())?;
            
        Ok(serde_json::json!({
            "status": "success",
            "mime_type": "audio/mp4",
            "extension": "m4a"
        }))
    } else {
        Err("Plugin not initialized".to_string())
    }
}

/// Extract metadata
fn extract_metadata(params: Value) -> Result<Value, String> {
    let file_path_str = params["file_path"]
        .as_str()
        .ok_or("Missing 'file_path' parameter")?;
        
    let file_path = std::path::Path::new(file_path_str);
    
    let instance = PLUGIN.lock().map_err(|e| format!("Failed to acquire lock: {}", e))?;
    if let Some(plugin) = instance.as_ref() {
        let metadata = plugin.extract_metadata(file_path)
            .map_err(|e| e.to_string())?;
            
        Ok(serde_json::to_value(metadata).map_err(|e| e.to_string())?)
    } else {
        Err("Plugin not initialized".to_string())
    }
}

/// Extract ID3 metadata (for scraper)
fn extract_id3_metadata(params: Value) -> Result<Value, String> {
    let file_path_str = params["file_path"]
        .as_str()
        .ok_or("Missing 'file_path' parameter")?;
        
    let file_path = std::path::Path::new(file_path_str);
    
    let instance = PLUGIN.lock().map_err(|e| format!("Failed to acquire lock: {}", e))?;
    if let Some(plugin) = instance.as_ref() {
        let xm_info = plugin.extract_id3_metadata(file_path)
            .map_err(|e| e.to_string())?;
            
        // Convert XmInfo to a simplified structure for JSON response
        let result = serde_json::json!({
            "title": xm_info.title,
            "artist": xm_info.artist,
            "album": xm_info.album,
            "track_number": xm_info.tracknumber,
            "has_cover": xm_info.has_cover || xm_info.cover_url.is_some(),
            "cover_url": xm_info.cover_url,
        });
            
        Ok(result)
    } else {
        Err("Plugin not initialized".to_string())
    }
}

/// Get required metadata read size
fn get_metadata_read_size(params: Value) -> Result<Value, String> {
    let header_base64 = params["header_base64"]
        .as_str()
        .ok_or("Missing 'header_base64' parameter")?;
        
    let header_bytes = base64::engine::general_purpose::STANDARD
        .decode(header_base64)
        .map_err(|e| format!("Base64 decode failed: {}", e))?;
    
    let instance = PLUGIN.lock().map_err(|e| format!("Failed to acquire lock: {}", e))?;
    if let Some(plugin) = instance.as_ref() {
        let size = plugin.get_metadata_read_size(&header_bytes);
        Ok(serde_json::json!({"size": size}))
    } else {
        Err("Plugin not initialized".to_string())
    }
}

/// Get decryption plan
fn get_decryption_plan(params: Value) -> Result<Value, String> {
    let header_base64 = params["header_base64"]
        .as_str()
        .ok_or("Missing 'header_base64' parameter")?;
        
    let header_bytes = base64::engine::general_purpose::STANDARD
        .decode(header_base64)
        .map_err(|e| format!("Base64 decode failed: {}", e))?;
    
    let instance = PLUGIN.lock().map_err(|e| format!("Failed to acquire lock: {}", e))?;
    if let Some(plugin) = instance.as_ref() {
        let plan = plugin.get_decryption_plan(&header_bytes)
            .map_err(|e| e.to_string())?;
        Ok(serde_json::to_value(plan).map_err(|e| e.to_string())?)
    } else {
        Err("Plugin not initialized".to_string())
    }
}

/// Decrypt chunk in memory
fn decrypt_chunk(params: Value) -> Result<Value, String> {
    let data_base64 = params["data_base64"]
        .as_str()
        .ok_or("Missing 'data_base64' parameter")?;
        
    let decrypt_params = params["params"]
        .clone();
        
    let data = base64::engine::general_purpose::STANDARD
        .decode(data_base64)
        .map_err(|e| format!("Base64 decode failed: {}", e))?;
    
    let instance = PLUGIN.lock().map_err(|e| format!("Failed to acquire lock: {}", e))?;
    if let Some(plugin) = instance.as_ref() {
        let decrypted = plugin.decrypt_chunk_data(&data, &decrypt_params)
            .map_err(|e| e.to_string())?;
            
        let decrypted_base64 = base64::engine::general_purpose::STANDARD.encode(decrypted);
        Ok(serde_json::json!({"data_base64": decrypted_base64}))
    } else {
        Err("Plugin not initialized".to_string())
    }
}

/// Garbage collect
fn garbage_collect(_params: Value) -> Result<Value, String> {
    log_memory_usage("pre-gc");
    println!("[xm-format] Garbage collect requested");
    
    // Explicitly call malloc_trim to release memory back to OS
    // This is crucial for glibc which tends to hold onto memory
    #[cfg(target_os = "linux")]
    unsafe {
        let ret = libc::malloc_trim(0);
        println!("[xm-format] malloc_trim(0) returned: {}", ret);
    }
    
    log_memory_usage("post-gc");
    Ok(serde_json::json!({"status": "ok"}))
}

/// Main entry point for plugin invocation
///
/// # Safety
/// This function is unsafe because it deals with raw pointers and FFI.
#[no_mangle]
pub unsafe extern "C" fn plugin_invoke(
    method: *const u8,
    params: *const u8,
    result_ptr: *mut *mut u8,
) -> c_int {
    // Validate inputs
    if method.is_null() || params.is_null() || result_ptr.is_null() {
        return -1;
    }
    
    // Convert C strings to Rust strings
    let method_str = match CStr::from_ptr(method as *const c_char).to_str() {
        Ok(s) => s,
        Err(_) => return -2,
    };
    
    let params_str = match CStr::from_ptr(params as *const c_char).to_str() {
        Ok(s) => s,
        Err(_) => return -3,
    };
    
    // Parse params JSON
    let params_json: Value = match serde_json::from_str(params_str) {
        Ok(v) => v,
        Err(_) => return -4,
    };
    
    // Dispatch method call
    // println!("[xm-format] Invoking method: {}", method_str); // Commented out to reduce noise
    let result = match method_str {
        "initialize" => initialize(params_json),
        "shutdown" => shutdown(params_json),
        "garbage_collect" => garbage_collect(params_json),
        "detect" => detect(params_json),
        "decrypt" => decrypt(params_json),
        "extract_metadata" => extract_metadata(params_json),
        "extract_id3_metadata" => extract_id3_metadata(params_json),
        "get_metadata_read_size" => get_metadata_read_size(params_json),
        "get_decryption_plan" => get_decryption_plan(params_json),
        "decrypt_chunk" => decrypt_chunk(params_json),
        _ => Err(format!("Unknown method: {}", method_str)),
    };
    
    // Handle result
    match result {
        Ok(value) => {
            let json_str = serde_json::to_string(&value).unwrap_or_else(|_| "{}".to_string());
            let c_string = CString::new(json_str).unwrap();
            *result_ptr = c_string.into_raw() as *mut u8; // Caller must free this!
            println!("[xm-format] Method {} success", method_str);
            0
        }
        Err(err) => {
            eprintln!("[xm-format] Plugin error in {}: {}", method_str, err);
            -5
        }
    }
}

/// Free the result string allocated by plugin_invoke
///
/// # Safety
/// This function is unsafe because it deals with raw pointers and FFI.
#[no_mangle]
pub unsafe extern "C" fn plugin_free(ptr: *mut u8) {
    if !ptr.is_null() {
        let _ = CString::from_raw(ptr as *mut c_char);
    }
}
