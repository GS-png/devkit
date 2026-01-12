use anyhow::Result;
use base64::Engine;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use uuid::Uuid;

use crate::mcp::types::{McpResponse, PopupRequest};

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct HistoryEntrySummary {
    pub id: String,
    pub timestamp: String,
    pub request_id: Option<String>,
    pub source: Option<String>,
    pub preview: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct HistoryEntryDetail {
    pub summary: HistoryEntrySummary,
    pub request: Option<PopupRequest>,
    pub response: serde_json::Value,
    pub markdown: String,
    pub images: Vec<HistoryImage>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct HistoryImage {
    pub filename: String,
    pub media_type: String,
    pub data_uri: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
struct HistoryEntryMeta {
    pub id: String,
    pub timestamp: String,
    pub request_id: Option<String>,
    pub source: Option<String>,
    pub request: Option<PopupRequest>,
    pub response: serde_json::Value,
    pub image_files: Vec<String>,
}

fn preview_from_meta(meta: &HistoryEntryMeta) -> String {
    if let Some(req) = meta.request.as_ref() {
        let first = req.message.lines().next().unwrap_or("").trim();
        if !first.is_empty() {
            return first.to_string();
        }
    }

    if let Ok(r) = serde_json::from_value::<McpResponse>(meta.response.clone()) {
        if let Some(input) = r.user_input {
            let first = input.lines().next().unwrap_or("").trim();
            if !first.is_empty() {
                return first.to_string();
            }
        }
    }

    String::new()
}

pub fn history_base_dir() -> Result<PathBuf> {
    let base = dirs::data_dir()
        .or_else(dirs::config_dir)
        .ok_or_else(|| anyhow::anyhow!("无法获取数据目录"))?
        .join("sanshu")
        .join("mcp_history");
    fs::create_dir_all(&base)?;
    Ok(base)
}

fn entry_dir_from_id(base: &Path, id: &str) -> PathBuf {
    base.join(id)
}

fn safe_filename(ext: &str) -> String {
    let ext = ext.trim_start_matches('.');
    format!("{}.{}", Uuid::new_v4(), ext)
}

fn ext_from_media_type(media_type: &str) -> &'static str {
    match media_type {
        "image/png" => "png",
        "image/jpeg" => "jpg",
        "image/jpg" => "jpg",
        "image/webp" => "webp",
        "image/gif" => "gif",
        "image/svg+xml" => "svg",
        _ => "bin",
    }
}

fn build_markdown(
    request: Option<&PopupRequest>,
    response: &serde_json::Value,
    image_files: &[String],
) -> String {
    let mut out = String::new();

    if let Some(req) = request {
        out.push_str("# 请求\n\n");
        out.push_str(&req.message);
        out.push_str("\n\n");

        if let Some(opts) = &req.predefined_options {
            if !opts.is_empty() {
                out.push_str("## 选项\n\n");
                for o in opts {
                    out.push_str("- ");
                    out.push_str(o);
                    out.push_str("\n");
                }
                out.push_str("\n");
            }
        }
    }

    out.push_str("# 回复\n\n");
    match serde_json::from_value::<McpResponse>(response.clone()) {
        Ok(r) => {
            if let Some(input) = r.user_input {
                out.push_str(&input);
                out.push_str("\n\n");
            }
            if !r.selected_options.is_empty() {
                out.push_str("## 选择\n\n");
                for o in r.selected_options {
                    out.push_str("- ");
                    out.push_str(&o);
                    out.push_str("\n");
                }
                out.push_str("\n");
            }
        }
        Err(_) => {
            if let Some(s) = response.as_str() {
                out.push_str(s);
                out.push_str("\n");
            } else {
                out.push_str(&response.to_string());
                out.push_str("\n");
            }
        }
    }

    if !image_files.is_empty() {
        out.push_str("\n## 图片\n\n");
        for f in image_files {
            out.push_str(&format!("![](images/{})\n\n", f));
        }
    }

    out
}

pub fn save_history_entry(request: Option<PopupRequest>, response: serde_json::Value) -> Result<()> {
    let base = history_base_dir()?;

    let now: DateTime<Utc> = Utc::now();
    let id = format!("{}-{}", now.format("%Y%m%dT%H%M%S%.3fZ"), Uuid::new_v4());
    let dir = entry_dir_from_id(&base, &id);
    let images_dir = dir.join("images");
    fs::create_dir_all(&images_dir)?;

    let (timestamp, request_id, source, image_files) = match serde_json::from_value::<McpResponse>(response.clone()) {
        Ok(r) => {
            let ts = r.metadata.timestamp.unwrap_or_else(|| now.to_rfc3339());
            let rid = r.metadata.request_id;
            let src = r.metadata.source;

            let mut files = Vec::new();
            for img in r.images {
                let ext = ext_from_media_type(&img.media_type);
                let filename = img.filename.unwrap_or_else(|| safe_filename(ext));
                let bytes = base64::engine::general_purpose::STANDARD.decode(img.data)?;
                fs::write(images_dir.join(&filename), bytes)?;
                files.push(filename);
            }

            (ts, rid, src, files)
        }
        Err(_) => (now.to_rfc3339(), None, None, Vec::new()),
    };

    let response_for_meta = if let Some(obj) = response.as_object() {
        let mut obj = obj.clone();
        if let Some(images) = obj.get("images").and_then(|v| v.as_array()) {
            let mut sanitized_images = Vec::new();
            for (idx, img) in images.iter().enumerate() {
                let media_type = img
                    .get("media_type")
                    .and_then(|v| v.as_str())
                    .unwrap_or("application/octet-stream")
                    .to_string();
                let filename = img
                    .get("filename")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string())
                    .or_else(|| image_files.get(idx).cloned());

                let mut item = serde_json::Map::new();
                item.insert("media_type".to_string(), serde_json::Value::String(media_type));
                if let Some(filename) = filename {
                    item.insert("filename".to_string(), serde_json::Value::String(filename));
                }
                sanitized_images.push(serde_json::Value::Object(item));
            }
            obj.insert("images".to_string(), serde_json::Value::Array(sanitized_images));
        }
        serde_json::Value::Object(obj)
    } else {
        response.clone()
    };

    let markdown = build_markdown(request.as_ref(), &response, &image_files);
    fs::write(dir.join("entry.md"), &markdown)?;

    let meta = HistoryEntryMeta {
        id: id.clone(),
        timestamp,
        request_id,
        source,
        request,
        response: response_for_meta,
        image_files,
    };

    fs::write(dir.join("meta.json"), serde_json::to_string_pretty(&meta)?)?;

    Ok(())
}

pub fn list_history_entries(limit: usize) -> Result<Vec<HistoryEntrySummary>> {
    let base = history_base_dir()?;
    let mut entries = Vec::new();

    for item in fs::read_dir(base)? {
        let item = item?;
        if !item.file_type()?.is_dir() {
            continue;
        }
        let dir = item.path();
        let meta_path = dir.join("meta.json");
        if !meta_path.exists() {
            continue;
        }
        if let Ok(content) = fs::read_to_string(&meta_path) {
            if let Ok(meta) = serde_json::from_str::<HistoryEntryMeta>(&content) {
                let preview = preview_from_meta(&meta);

                entries.push(HistoryEntrySummary {
                    id: meta.id,
                    timestamp: meta.timestamp,
                    request_id: meta.request_id,
                    source: meta.source,
                    preview,
                });
            }
        }
    }

    entries.sort_by(|a, b| b.timestamp.cmp(&a.timestamp));
    if entries.len() > limit {
        entries.truncate(limit);
    }

    Ok(entries)
}

pub fn get_history_entry(id: String) -> Result<HistoryEntryDetail> {
    let base = history_base_dir()?;
    let dir = entry_dir_from_id(&base, &id);
    let meta_path = dir.join("meta.json");

    let meta: HistoryEntryMeta = serde_json::from_str(&fs::read_to_string(meta_path)?)?;
    let markdown = fs::read_to_string(dir.join("entry.md")).unwrap_or_default();

    let mut images = Vec::new();
    let images_dir = dir.join("images");
    for filename in &meta.image_files {
        let path = images_dir.join(filename);
        if let Ok(bytes) = fs::read(&path) {
            let media_type = if filename.ends_with(".png") {
                "image/png"
            } else if filename.ends_with(".jpg") || filename.ends_with(".jpeg") {
                "image/jpeg"
            } else if filename.ends_with(".webp") {
                "image/webp"
            } else if filename.ends_with(".gif") {
                "image/gif"
            } else if filename.ends_with(".svg") {
                "image/svg+xml"
            } else {
                "application/octet-stream"
            };

            let b64 = base64::engine::general_purpose::STANDARD.encode(bytes);
            images.push(HistoryImage {
                filename: filename.clone(),
                media_type: media_type.to_string(),
                data_uri: format!("data:{};base64,{}", media_type, b64),
            });
        }
    }

    let preview = preview_from_meta(&meta);
    let summary = HistoryEntrySummary {
        id: meta.id.clone(),
        timestamp: meta.timestamp.clone(),
        request_id: meta.request_id.clone(),
        source: meta.source.clone(),
        preview,
    };

    Ok(HistoryEntryDetail {
        summary,
        request: meta.request,
        response: meta.response,
        markdown,
        images,
    })
}

pub fn delete_history_entry(id: String) -> Result<()> {
    let base = history_base_dir()?;
    let dir = entry_dir_from_id(&base, &id);
    if dir.exists() {
        fs::remove_dir_all(dir)?;
    }
    Ok(())
}

pub fn delete_history_entries_by_time_range(
    start: Option<String>,
    end: Option<String>,
) -> Result<u32> {
    let start_ts = match start {
        Some(s) if !s.trim().is_empty() => {
            Some(DateTime::parse_from_rfc3339(&s)?.with_timezone(&Utc))
        }
        _ => None,
    };
    let end_ts = match end {
        Some(s) if !s.trim().is_empty() => Some(DateTime::parse_from_rfc3339(&s)?.with_timezone(&Utc)),
        _ => None,
    };

    let base = history_base_dir()?;
    let mut deleted: u32 = 0;

    for item in fs::read_dir(&base)? {
        let item = item?;
        if !item.file_type()?.is_dir() {
            continue;
        }

        let dir = item.path();
        let meta_path = dir.join("meta.json");
        if !meta_path.exists() {
            continue;
        }

        let meta_content = match fs::read_to_string(&meta_path) {
            Ok(s) => s,
            Err(_) => continue,
        };
        let meta: HistoryEntryMeta = match serde_json::from_str(&meta_content) {
            Ok(m) => m,
            Err(_) => continue,
        };

        let ts = match DateTime::parse_from_rfc3339(&meta.timestamp) {
            Ok(t) => t.with_timezone(&Utc),
            Err(_) => continue,
        };

        if let Some(ref start_ts) = start_ts {
            if ts < *start_ts {
                continue;
            }
        }
        if let Some(ref end_ts) = end_ts {
            if ts > *end_ts {
                continue;
            }
        }

        if fs::remove_dir_all(dir).is_ok() {
            deleted += 1;
        }
    }

    Ok(deleted)
}

pub fn export_history_entry_zip(id: String, target_dir: PathBuf) -> Result<PathBuf> {
    let base = history_base_dir()?;
    let src_dir = entry_dir_from_id(&base, &id);
    if !src_dir.exists() {
        return Err(anyhow::anyhow!("历史记录不存在: {}", id));
    }

    fs::create_dir_all(&target_dir)?;
    let zip_path = target_dir.join(format!("sanshu-mcp-history-{}.zip", id));

    let file = fs::File::create(&zip_path)?;
    let mut zip = zip::ZipWriter::new(file);
    let options = zip::write::SimpleFileOptions::default();

    fn add_dir_to_zip(
        zip: &mut zip::ZipWriter<fs::File>,
        options: zip::write::SimpleFileOptions,
        base_dir: &Path,
        dir: &Path,
    ) -> Result<()> {
        for entry in fs::read_dir(dir)? {
            let entry = entry?;
            let path = entry.path();
            let rel = path.strip_prefix(base_dir)?;
            let name = rel.to_string_lossy().replace('\\', "/");

            if entry.file_type()?.is_dir() {
                zip.add_directory(format!("{}/", name), options)?;
                add_dir_to_zip(zip, options, base_dir, &path)?;
            } else {
                zip.start_file(name, options)?;
                let mut f = fs::File::open(&path)?;
                let mut buf = Vec::new();
                f.read_to_end(&mut buf)?;
                zip.write_all(&buf)?;
            }
        }
        Ok(())
    }

    add_dir_to_zip(&mut zip, options, &src_dir, &src_dir)?;
    zip.finish()?;

    Ok(zip_path)
}

pub fn export_history_by_time_range_zip(
    start: Option<String>,
    end: Option<String>,
    target_dir: PathBuf,
) -> Result<PathBuf> {
    let start_ts = match start {
        Some(s) if !s.trim().is_empty() => {
            Some(DateTime::parse_from_rfc3339(&s)?.with_timezone(&Utc))
        }
        _ => None,
    };
    let end_ts = match end {
        Some(s) if !s.trim().is_empty() => Some(DateTime::parse_from_rfc3339(&s)?.with_timezone(&Utc)),
        _ => None,
    };

    let base = history_base_dir()?;

    fs::create_dir_all(&target_dir)?;
    let now: DateTime<Utc> = Utc::now();
    let zip_path = target_dir.join(format!(
        "sanshu-mcp-history-range-{}.zip",
        now.format("%Y%m%dT%H%M%S%.3fZ")
    ));

    let file = fs::File::create(&zip_path)?;
    let mut zip = zip::ZipWriter::new(file);
    let options = zip::write::SimpleFileOptions::default();

    fn add_dir_to_zip_with_prefix(
        zip: &mut zip::ZipWriter<fs::File>,
        options: zip::write::SimpleFileOptions,
        base_dir: &Path,
        dir: &Path,
        prefix: &str,
    ) -> Result<()> {
        for entry in fs::read_dir(dir)? {
            let entry = entry?;
            let path = entry.path();
            let rel = path.strip_prefix(base_dir)?;
            let rel_name = rel.to_string_lossy().replace('\\', "/");
            let name = format!("{}/{}", prefix.trim_end_matches('/'), rel_name);

            if entry.file_type()?.is_dir() {
                zip.add_directory(format!("{}/", name), options)?;
                add_dir_to_zip_with_prefix(zip, options, base_dir, &path, prefix)?;
            } else {
                zip.start_file(name, options)?;
                let mut f = fs::File::open(&path)?;
                let mut buf = Vec::new();
                f.read_to_end(&mut buf)?;
                zip.write_all(&buf)?;
            }
        }
        Ok(())
    }

    let mut added: u32 = 0;
    for item in fs::read_dir(&base)? {
        let item = item?;
        if !item.file_type()?.is_dir() {
            continue;
        }

        let dir = item.path();
        let meta_path = dir.join("meta.json");
        if !meta_path.exists() {
            continue;
        }

        let meta_content = match fs::read_to_string(&meta_path) {
            Ok(s) => s,
            Err(_) => continue,
        };
        let meta: HistoryEntryMeta = match serde_json::from_str(&meta_content) {
            Ok(m) => m,
            Err(_) => continue,
        };

        let ts = match DateTime::parse_from_rfc3339(&meta.timestamp) {
            Ok(t) => t.with_timezone(&Utc),
            Err(_) => continue,
        };

        if let Some(ref start_ts) = start_ts {
            if ts < *start_ts {
                continue;
            }
        }
        if let Some(ref end_ts) = end_ts {
            if ts > *end_ts {
                continue;
            }
        }

        let entry_id = meta.id.clone();
        add_dir_to_zip_with_prefix(&mut zip, options, &dir, &dir, &format!("mcp_history/{}", entry_id))?;
        added += 1;
    }

    zip.finish()?;
    if added == 0 {
        let _ = fs::remove_file(&zip_path);
        return Err(anyhow::anyhow!("没有匹配的历史记录"));
    }

    Ok(zip_path)
}
