use anyhow::Result;
use rmcp::model::{ErrorData as McpError, Content};

use crate::mcp::types::{McpResponse, McpResponseContent};

/// Parse MCP response content
pub fn parse_mcp_response(response: &str) -> Result<Vec<Content>, McpError> {
    if response.trim() == "CANCELLED" {
        return Ok(vec![Content::text("Operation cancelled by user".to_string())]);
    }

    // Try structured format first
    if let Ok(structured_response) = serde_json::from_str::<McpResponse>(response) {
        return parse_structured_response(structured_response);
    }

    // Fallback to legacy format
    match serde_json::from_str::<Vec<McpResponseContent>>(response) {
        Ok(content_array) => {
            let mut result = Vec::new();
            let mut image_count = 0;

            let mut user_text_parts = Vec::new();
            let mut image_info_parts = Vec::new();

            for content in content_array {
                match content.content_type.as_str() {
                    "text" => {
                        if let Some(text) = content.text {
                            user_text_parts.push(text);
                        }
                    }
                    "image" => {
                        if let Some(source) = content.source {
                            if source.source_type == "base64" {
                                image_count += 1;

                                result.push(Content::image(source.data.clone(), source.media_type.clone()));

                                let base64_len = source.data.len();
                                let preview = if base64_len > 50 {
                                    format!("{}...", &source.data[..50])
                                } else {
                                    source.data.clone()
                                };

                                let estimated_size = (base64_len * 3) / 4;
                                let size_str = if estimated_size < 1024 {
                                    format!("{} B", estimated_size)
                                } else if estimated_size < 1024 * 1024 {
                                    format!("{:.1} KB", estimated_size as f64 / 1024.0)
                                } else {
                                    format!("{:.1} MB", estimated_size as f64 / (1024.0 * 1024.0))
                                };

                                let image_info = format!(
                                    "=== Image {} ===\nType: {}\nSize: {}\nBase64 preview: {}\nFull Base64 length: {} chars",
                                    image_count, source.media_type, size_str, preview, base64_len
                                );
                                image_info_parts.push(image_info);
                            }
                        }
                    }
                    _ => {
                        if let Some(text) = content.text {
                            user_text_parts.push(text);
                        }
                    }
                }
            }

            let mut all_text_parts = Vec::new();

            if !user_text_parts.is_empty() {
                all_text_parts.extend(user_text_parts);
            }

            if !image_info_parts.is_empty() {
                all_text_parts.extend(image_info_parts);
            }

            if image_count > 0 {
                all_text_parts.push(format!(
                    "Note: User provided {} image(s). Image data is included in Base64 format above.",
                    image_count
                ));
            }
            if !all_text_parts.is_empty() {
                let combined_text = all_text_parts.join("\n\n");
                result.push(Content::text(combined_text));
            }

            if result.is_empty() {
                result.push(Content::text("No content provided".to_string()));
            }

            Ok(result)
        }
        Err(_) => {
            Ok(vec![Content::text(response.to_string())])
        }
    }
}

/// Parse structured response format
fn parse_structured_response(response: McpResponse) -> Result<Vec<Content>, McpError> {
    let mut result = Vec::new();
    let mut text_parts = Vec::new();

    if !response.selected_options.is_empty() {
        text_parts.push(format!("Selected: {}", response.selected_options.join(", ")));
    }
    if let Some(user_input) = response.user_input {
        if !user_input.trim().is_empty() {
            text_parts.push(user_input.trim().to_string());
        }
    }

    let mut image_info_parts = Vec::new();
    for (index, image) in response.images.iter().enumerate() {
        result.push(Content::image(image.data.clone(), image.media_type.clone()));

        let base64_len = image.data.len();
        let preview = if base64_len > 50 {
            format!("{}...", &image.data[..50])
        } else {
            image.data.clone()
        };

        let estimated_size = (base64_len * 3) / 4;
        let size_str = if estimated_size < 1024 {
            format!("{} B", estimated_size)
        } else if estimated_size < 1024 * 1024 {
            format!("{:.1} KB", estimated_size as f64 / 1024.0)
        } else {
            format!("{:.1} MB", estimated_size as f64 / (1024.0 * 1024.0))
        };

        let filename_info = image.filename.as_ref()
            .map(|f| format!("\nFilename: {}", f))
            .unwrap_or_default();

        let image_info = format!(
            "=== Image {} ==={}
Type: {}
Size: {}
Base64 preview: {}
Full Base64 length: {} chars",
            index + 1, filename_info, image.media_type, size_str, preview, base64_len
        );
        image_info_parts.push(image_info);
    }

    let mut all_text_parts = text_parts;
    all_text_parts.extend(image_info_parts);

    if !response.images.is_empty() {
        all_text_parts.push(format!(
            "Note: User provided {} image(s). Image data is included in Base64 format above.",
            response.images.len()
        ));
    }

    if !all_text_parts.is_empty() {
        let combined_text = all_text_parts.join("\n\n");
        result.push(Content::text(combined_text));
    }

    if result.is_empty() {
        result.push(Content::text("No content provided".to_string()));
    }

    Ok(result)
}
