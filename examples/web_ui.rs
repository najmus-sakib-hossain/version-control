//! GitHub-like Web UI for Forge
//! 
//! Features:
//! - Browse files and folders (like GitHub)
//! - View file contents with syntax highlighting
//! - Download individual files
//! - Download entire repository as ZIP
//! - Responsive design with Tailwind CSS

use axum::{
    extract::{Path, State},
    http::{header, StatusCode},
    response::{Html, IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use serde::Serialize;
use std::io::Cursor;
use std::sync::Arc;
use tokio::fs;
use tower_http::services::ServeDir;
use anyhow::Result;
use zip::write::{FileOptions, ZipWriter};

// Import dx_forge modules
use dx_forge::storage::r2::{R2Config, R2Storage};

/// File tree node
#[derive(Debug, Clone, Serialize)]
struct FileNode {
    name: String,
    path: String,
    #[serde(rename = "type")]
    node_type: String, // "file" or "directory"
    size: Option<u64>,
    hash: Option<String>,
    children: Option<Vec<FileNode>>,
}

/// File content response
#[derive(Debug, Serialize)]
struct FileContent {
    path: String,
    content: String,
    size: u64,
    hash: String,
    language: String,
}

/// Application state
#[derive(Clone)]
struct AppState {
    #[allow(dead_code)]
    r2_storage: Arc<R2Storage>,
    demo_root: String,
}

#[tokio::main]
async fn main() -> Result<()> {
    // Load R2 configuration
    let r2_config = R2Config::from_env()?;
    let r2_storage = Arc::new(R2Storage::new(r2_config)?);
    
    let demo_root = "examples/forge-demo".to_string();
    
    let state = AppState {
        r2_storage,
        demo_root,
    };

    // Build router
    let app = Router::new()
        .route("/", get(index_handler))
        .route("/api/tree", get(get_file_tree))
        .route("/api/file/{*path}", get(get_file_content))
        .route("/api/download/{*path}", get(download_file))
        .route("/api/download-zip", post(download_as_zip))
        .nest_service("/static", ServeDir::new("examples/web-ui/static"))
        .with_state(state);

    // Start server
    let addr = "127.0.0.1:3000";
    println!("🚀 Forge Web UI running at http://{}", addr);
    println!("📁 Serving: examples/forge-demo");
    println!("🌐 Open your browser to explore files!");
    
    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;
    
    Ok(())
}

/// Index page handler
async fn index_handler() -> Html<String> {
    Html(HTML_TEMPLATE.to_string())
}

/// Get file tree
async fn get_file_tree(State(state): State<AppState>) -> Result<Json<FileNode>, StatusCode> {
    let root = build_file_tree(&state.demo_root, &state.demo_root)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    
    Ok(Json(root))
}

/// Build file tree recursively
fn build_file_tree<'a>(root: &'a str, path: &'a str) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<FileNode>> + Send + 'a>> {
    Box::pin(async move {
    let metadata = fs::metadata(path).await?;
    let name = std::path::Path::new(path)
        .file_name()
        .unwrap()
        .to_string_lossy()
        .to_string();
    
    let relative_path = path.strip_prefix(root).unwrap_or(path);
    // Normalize path separators for URLs (Windows uses backslashes)
    let relative_path = relative_path.replace('\\', "/");
    // Remove any leading slashes to ensure clean paths
    let relative_path = relative_path.trim_start_matches('/');
    
    if metadata.is_dir() {
        let mut children = Vec::new();
        let mut entries = fs::read_dir(path).await?;
        
        while let Some(entry) = entries.next_entry().await? {
            let child_path = entry.path().to_string_lossy().to_string();
            
            // Skip hidden files except .forge
            let child_name = entry.file_name().to_string_lossy().to_string();
            if child_name.starts_with('.') && child_name != ".forge" {
                continue;
            }
            
            if let Ok(child) = build_file_tree(root, &child_path).await {
                children.push(child);
            }
        }
        
        // Sort: directories first, then alphabetically
        children.sort_by(|a, b| {
            match (a.node_type.as_str(), b.node_type.as_str()) {
                ("directory", "file") => std::cmp::Ordering::Less,
                ("file", "directory") => std::cmp::Ordering::Greater,
                _ => a.name.cmp(&b.name),
            }
        });
        
        Ok(FileNode {
            name,
            path: relative_path.to_string(),
            node_type: "directory".to_string(),
            size: None,
            hash: None,
            children: Some(children),
        })
    } else {
        Ok(FileNode {
            name,
            path: relative_path.to_string(),
            node_type: "file".to_string(),
            size: Some(metadata.len()),
            hash: None, // TODO: Calculate from R2
            children: None,
        })
    }
    })
}

/// Get file content
async fn get_file_content(
    State(state): State<AppState>,
    Path(path): Path<String>,
) -> Result<Json<FileContent>, StatusCode> {
    // Clean up path: remove leading slashes and normalize separators
    let clean_path = path.trim_start_matches('/').replace('\\', "/");
    let full_path = format!("{}/{}", state.demo_root, clean_path);
    
    let content = fs::read_to_string(&full_path)
        .await
        .map_err(|e| {
            eprintln!("Failed to read file '{}': {}", full_path, e);
            StatusCode::NOT_FOUND
        })?;
    
    let metadata = fs::metadata(&full_path)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    
    // Detect language from extension
    let language = detect_language(&clean_path);
    
    // TODO: Get hash from R2
    let hash = format!("{:x}", md5::compute(&content));
    
    Ok(Json(FileContent {
        path: clean_path,
        content,
        size: metadata.len(),
        hash,
        language,
    }))
}

/// Download individual file
async fn download_file(
    State(state): State<AppState>,
    Path(path): Path<String>,
) -> Result<Response, StatusCode> {
    // Clean up path: remove leading slashes and normalize separators
    let clean_path = path.trim_start_matches('/').replace('\\', "/");
    let full_path = format!("{}/{}", state.demo_root, clean_path);
    
    let content = fs::read(&full_path)
        .await
        .map_err(|e| {
            eprintln!("Failed to read file '{}': {}", full_path, e);
            StatusCode::NOT_FOUND
        })?;
    
    let filename = std::path::Path::new(&path)
        .file_name()
        .unwrap()
        .to_string_lossy()
        .to_string();
    
    Ok((
        StatusCode::OK,
        [
            (header::CONTENT_TYPE, "application/octet-stream"),
            (
                header::CONTENT_DISPOSITION,
                &format!("attachment; filename=\"{}\"", filename),
            ),
        ],
        content,
    )
        .into_response())
}

/// Download repository as ZIP
async fn download_as_zip(
    State(state): State<AppState>,
) -> Result<Response, StatusCode> {
    use std::io::Cursor;
    
    let mut zip_buffer = Cursor::new(Vec::new());
    let mut zip = ZipWriter::new(&mut zip_buffer);
    
    let options = FileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);
    
    // Add all files to ZIP
    add_directory_to_zip(&mut zip, &state.demo_root, &state.demo_root, options)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    
    zip.finish().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    
    let zip_data = zip_buffer.into_inner();
    
    Ok((
        StatusCode::OK,
        [
            (header::CONTENT_TYPE, "application/zip"),
            (
                header::CONTENT_DISPOSITION,
                "attachment; filename=\"forge-demo.zip\"",
            ),
        ],
        zip_data,
    )
        .into_response())
}

/// Recursively add directory to ZIP
fn add_directory_to_zip<'a>(
    zip: &'a mut ZipWriter<&mut Cursor<Vec<u8>>>,
    root: &'a str,
    path: &'a str,
    options: FileOptions<'static, ()>,
) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<()>> + Send + 'a>> {
    Box::pin(async move {
    let mut entries = fs::read_dir(path).await?;
    
    while let Some(entry) = entries.next_entry().await? {
        let entry_path = entry.path();
        let entry_path_str = entry_path.to_string_lossy();
        
        // Skip hidden files except .forge
        let name = entry.file_name().to_string_lossy().to_string();
        if name.starts_with('.') && name != ".forge" {
            continue;
        }
        
        let relative_path = entry_path_str.strip_prefix(root).unwrap_or(&entry_path_str);
        let relative_path = relative_path.trim_start_matches('/').trim_start_matches('\\');
        
        if entry.file_type().await?.is_dir() {
            // Add directory
            zip.add_directory(relative_path, options)?;
            
            // Recursively add contents
            add_directory_to_zip(zip, root, &entry_path_str, options).await?;
        } else {
            // Add file
            let content = fs::read(&entry_path).await?;
            zip.start_file(relative_path, options)?;
            std::io::Write::write_all(zip, &content)?;
        }
    }
    
    Ok(())
    })
}

/// Detect programming language from file extension
fn detect_language(path: &str) -> String {
    let ext = std::path::Path::new(path)
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or("");
    
    match ext {
        "rs" => "rust",
        "js" => "javascript",
        "ts" => "typescript",
        "py" => "python",
        "java" => "java",
        "c" => "c",
        "cpp" | "cc" | "cxx" => "cpp",
        "go" => "go",
        "rb" => "ruby",
        "php" => "php",
        "swift" => "swift",
        "kt" => "kotlin",
        "sh" | "bash" => "bash",
        "json" => "json",
        "yaml" | "yml" => "yaml",
        "toml" => "toml",
        "xml" => "xml",
        "html" => "html",
        "css" => "css",
        "md" => "markdown",
        "sql" => "sql",
        _ => "plaintext",
    }
    .to_string()
}

/// HTML template (GitHub-like UI)
const HTML_TEMPLATE: &str = r#"<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <title>Forge Demo - File Browser</title>
    <script src="https://cdn.tailwindcss.com"></script>
    <link rel="stylesheet" href="https://cdnjs.cloudflare.com/ajax/libs/highlight.js/11.9.0/styles/github-dark.min.css">
    <script src="https://cdnjs.cloudflare.com/ajax/libs/highlight.js/11.9.0/highlight.min.js"></script>
    <style>
        .file-tree-item:hover { background-color: #2d333b; cursor: pointer; }
        .file-tree-item.active { background-color: #1f6feb; }
        .folder-icon::before { content: "📁 "; }
        .file-icon::before { content: "📄 "; }
        pre { margin: 0; }
        code { font-family: 'Consolas', 'Monaco', monospace; font-size: 14px; }
    </style>
</head>
<body class="bg-gray-900 text-gray-100">
    <!-- Header -->
    <header class="bg-gray-800 border-b border-gray-700 p-4">
        <div class="container mx-auto flex justify-between items-center">
            <h1 class="text-2xl font-bold">🔥 Forge Demo</h1>
            <div class="space-x-4">
                <button onclick="downloadZip()" class="bg-green-600 hover:bg-green-700 px-4 py-2 rounded">
                    🗜️ Download ZIP
                </button>
                <a href="https://github.com" class="text-blue-400 hover:underline">GitHub</a>
            </div>
        </div>
    </header>

    <!-- Main Content -->
    <div class="container mx-auto mt-6 flex gap-4">
        <!-- File Tree (Left Sidebar) -->
        <aside class="w-1/4 bg-gray-800 rounded-lg p-4">
            <h2 class="text-lg font-semibold mb-4 border-b border-gray-700 pb-2">📂 Files</h2>
            <div id="file-tree" class="space-y-1"></div>
        </aside>

        <!-- File Viewer (Main Content) -->
        <main class="flex-1 bg-gray-800 rounded-lg p-6">
            <div id="file-header" class="border-b border-gray-700 pb-4 mb-4 hidden">
                <div class="flex justify-between items-center">
                    <div>
                        <h2 id="file-name" class="text-xl font-semibold"></h2>
                        <p id="file-meta" class="text-sm text-gray-400 mt-1"></p>
                    </div>
                    <button onclick="downloadCurrentFile()" class="bg-blue-600 hover:bg-blue-700 px-4 py-2 rounded">
                        📥 Download
                    </button>
                </div>
            </div>
            
            <div id="file-content">
                <div class="text-center text-gray-400 py-20">
                    <p class="text-6xl mb-4">🔥</p>
                    <p class="text-xl">Select a file to view its contents</p>
                    <p class="text-sm mt-2">Or download the entire repository as ZIP</p>
                </div>
            </div>
        </main>
    </div>

    <!-- Footer -->
    <footer class="container mx-auto mt-8 text-center text-gray-400 pb-6">
        <p>Built with ❤️ and Rust 🦀 | <a href="/api/tree" class="text-blue-400 hover:underline">API</a></p>
    </footer>

    <script>
        let currentFile = null;
        let fileTree = null;

        // Load file tree on page load
        window.addEventListener('DOMContentLoaded', async () => {
            await loadFileTree();
        });

        // Load file tree from API
        async function loadFileTree() {
            try {
                const response = await fetch('/api/tree');
                fileTree = await response.json();
                renderFileTree(fileTree, document.getElementById('file-tree'), 0);
            } catch (error) {
                console.error('Failed to load file tree:', error);
                document.getElementById('file-tree').innerHTML = '<p class="text-red-400">Failed to load files</p>';
            }
        }

        // Render file tree recursively
        function renderFileTree(node, container, depth) {
            if (node.type === 'directory' && node.children) {
                // Directory
                const dirDiv = document.createElement('div');
                dirDiv.className = 'ml-' + (depth * 4);
                
                const dirHeader = document.createElement('div');
                dirHeader.className = 'file-tree-item folder-icon px-2 py-1 rounded flex items-center';
                dirHeader.textContent = node.name;
                dirHeader.onclick = () => {
                    const childrenDiv = dirDiv.querySelector('.children');
                    childrenDiv.classList.toggle('hidden');
                };
                
                dirDiv.appendChild(dirHeader);
                
                const childrenDiv = document.createElement('div');
                childrenDiv.className = 'children ml-4';
                node.children.forEach(child => renderFileTree(child, childrenDiv, depth + 1));
                dirDiv.appendChild(childrenDiv);
                
                container.appendChild(dirDiv);
            } else {
                // File
                const fileDiv = document.createElement('div');
                fileDiv.className = 'file-tree-item file-icon px-2 py-1 rounded ml-' + (depth * 4);
                fileDiv.textContent = node.name;
                fileDiv.onclick = (e) => loadFile(node.path, e.currentTarget);
                container.appendChild(fileDiv);
            }
        }

        // Load and display file content
        async function loadFile(path, clickedElement) {
            try {
                const response = await fetch(`/api/file/${path}`);
                const data = await response.json();
                currentFile = data;
                
                // Update header
                document.getElementById('file-header').classList.remove('hidden');
                document.getElementById('file-name').textContent = path;
                document.getElementById('file-meta').textContent = 
                    `${formatBytes(data.size)} • ${data.language} • ${data.hash.substring(0, 8)}`;
                
                // Render content with syntax highlighting
                const contentDiv = document.getElementById('file-content');
                contentDiv.innerHTML = `<pre><code class="language-${data.language}">${escapeHtml(data.content)}</code></pre>`;
                hljs.highlightAll();
                
                // Highlight active file in tree
                document.querySelectorAll('.file-tree-item').forEach(el => el.classList.remove('active'));
                if (clickedElement) {
                    clickedElement.classList.add('active');
                }
            } catch (error) {
                console.error('Failed to load file:', error);
                document.getElementById('file-content').innerHTML = 
                    '<p class="text-red-400">Failed to load file</p>';
            }
        }

        // Download current file
        function downloadCurrentFile() {
            if (currentFile) {
                window.location.href = `/api/download/${currentFile.path}`;
            }
        }

        // Download entire repository as ZIP
        async function downloadZip() {
            try {
                const response = await fetch('/api/download-zip', { method: 'POST' });
                const blob = await response.blob();
                const url = window.URL.createObjectURL(blob);
                const a = document.createElement('a');
                a.href = url;
                a.download = 'forge-demo.zip';
                document.body.appendChild(a);
                a.click();
                window.URL.revokeObjectURL(url);
                document.body.removeChild(a);
            } catch (error) {
                console.error('Failed to download ZIP:', error);
                alert('Failed to download ZIP');
            }
        }

        // Utility: Format bytes
        function formatBytes(bytes) {
            if (bytes === 0) return '0 Bytes';
            const k = 1024;
            const sizes = ['Bytes', 'KB', 'MB', 'GB'];
            const i = Math.floor(Math.log(bytes) / Math.log(k));
            return Math.round(bytes / Math.pow(k, i) * 100) / 100 + ' ' + sizes[i];
        }

        // Utility: Escape HTML
        function escapeHtml(text) {
            const div = document.createElement('div');
            div.textContent = text;
            return div.innerHTML;
        }
    </script>
</body>
</html>
"#;
