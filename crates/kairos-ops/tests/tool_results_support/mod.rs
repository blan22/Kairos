//! tool_results 시험의 도우미.
#![allow(dead_code, clippy::unwrap_used)]

use std::fs;
use std::path::{Path, PathBuf};

use kairos_core::model::McpResponse;
use kairos_ops::{ToolResults, collect_cwd, collector_project_dir};
use serde_json::json;

use crate::common::TestHome;

pub const SESSION: &str = "4f2a1c3e-0b6d-4e8f-9a7b-1c2d3e4f5a6b";
pub const FILE: &str = "toolu_plugin_0a1b2c3d.txt";
const DEFAULT_PREVIEW: &str = r#"{"threads":[{"id":"t"#;

pub struct Paths {
    pub test: TestHome,
    pub user_home: PathBuf,
    pub project: PathBuf,
    pub results_dir: PathBuf,
    pub file: PathBuf,
}

impl Paths {
    pub fn new() -> Self {
        let test = TestHome::new();
        let user_home = test.join("user");
        fs::create_dir_all(collect_cwd(&test.home)).unwrap();
        let project = collector_project_dir(&test.home, &user_home);
        let results_dir = project.join(SESSION).join("tool-results");
        fs::create_dir_all(&results_dir).unwrap();
        let file = results_dir.join(FILE);
        Self { test, user_home, project, results_dir, file }
    }

    pub fn results(&self) -> ToolResults {
        ToolResults::new(&self.project)
    }
}

fn marker_text(path: &str, preview: &str) -> String {
    format!(
        "<persisted-output>\nOutput too large (60.3KB). Full output saved to: {path}\n\nPreview (first 2KB):\n{preview}\n...\n</persisted-output>"
    )
}

pub fn marker(path: &Path) -> String {
    marker_text(&path.to_string_lossy(), DEFAULT_PREVIEW)
}

pub fn persisted_text(id: &str, path: &str) -> McpResponse {
    let text = marker_text(path, DEFAULT_PREVIEW);
    McpResponse { id: id.to_owned(), is_error: false, content: json!([{ "type": "text", "text": text }]) }
}

pub fn persisted(id: &str, path: &Path) -> McpResponse {
    persisted_text(id, &path.to_string_lossy())
}

pub fn persisted_with(id: &str, path: &Path, preview: &str) -> McpResponse {
    let text = marker_text(&path.to_string_lossy(), preview);
    McpResponse { id: id.to_owned(), is_error: false, content: json!([{ "type": "text", "text": text }]) }
}

pub fn text_of_response(response: &McpResponse) -> String {
    response.content[0]["text"].as_str().unwrap().to_owned()
}

/// 원본의 `join(results, '..', '..', '..', 'escape.txt')`. 풀린 경로는 프로젝트 폴더 바로 아래다.
pub fn js_join_escape(paths: &Paths) -> String {
    paths.project.parent().unwrap().join("escape.txt").display().to_string()
}
