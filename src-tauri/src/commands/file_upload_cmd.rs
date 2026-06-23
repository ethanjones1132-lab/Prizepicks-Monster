use crate::error::AppError;
use base64::{engine::general_purpose, Engine as _};
use std::fs;
use std::path::Path;

#[tauri::command]
pub async fn read_file_base64(path: String) -> Result<serde_json::Value, String> {
    let path = Path::new(&path);
    if !path.exists() {
        return Err(AppError::NotFound(format!("File not found: {}", path.display())).into());
    }

    let metadata = fs::metadata(path)
        .map_err(|e| AppError::Io(format!("Failed to read file metadata: {}", e)))?;
    let size = metadata.len();

    const MAX_SIZE: u64 = 5 * 1024 * 1024;
    if size > MAX_SIZE {
        return Err(AppError::Validation(format!(
            "File too large: {} bytes (max {} bytes / 5MB)",
            size, MAX_SIZE
        )).into());
    }

    let content = fs::read(path).map_err(|e| AppError::Io(format!("Failed to read file: {}", e)))?;
    let base64_content = general_purpose::STANDARD.encode(&content);

    let file_name = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("unknown")
        .to_string();

    let extension = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase();

    Ok(serde_json::json!({
        "name": file_name,
        "path": path.to_string_lossy().to_string(),
        "size": size,
        "extension": extension,
        "content_base64": base64_content,
    }))
}
