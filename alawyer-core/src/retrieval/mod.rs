use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::UNIX_EPOCH;

use jieba_rs::Jieba;
use once_cell::sync::Lazy;
use tantivy::collector::TopDocs;
use tantivy::query::QueryParser;
use tantivy::schema::Value;
use tantivy::schema::{
    IndexRecordOption, NumericOptions, SchemaBuilder, TextFieldIndexing, TextOptions, STORED,
};
use tantivy::{doc, Index, ReloadPolicy};
use walkdir::WalkDir;

use crate::error::{CoreError, CoreResult};

/// Process-level singleton for Jieba tokenizer.
/// Loading the built-in dictionary is expensive (~350K entries decompressed at runtime).
/// Sharing a single instance across all RetrievalEngine instances avoids repeated init.
static JIEBA: Lazy<Arc<Jieba>> = Lazy::new(|| Arc::new(Jieba::new()));

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, uniffi::Record)]
pub struct SearchResult {
    pub file_path: String,
    pub title: String,
    pub snippet: String,
    pub line_start: u32,
    pub line_end: u32,
    pub score: f32,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, uniffi::Record)]
pub struct KnowledgeInfo {
    pub kb_path: String,
    pub file_count: u32,
    pub updated_at: i64,
}

#[derive(Debug, Clone)]
struct KbChunk {
    file_path: String,
    title: String,
    snippet: String,
    line_start: u32,
    line_end: u32,
}

#[derive(Clone)]
pub struct RetrievalEngine {
    kb_root: PathBuf,
    jieba: Arc<Jieba>,
}

impl RetrievalEngine {
    pub fn new<P: AsRef<Path>>(kb_root: P) -> Self {
        Self {
            kb_root: kb_root.as_ref().to_path_buf(),
            jieba: JIEBA.clone(),
        }
    }

    pub fn search(
        &self,
        query: &str,
        scenario: &str,
        top_k: usize,
    ) -> CoreResult<Vec<SearchResult>> {
        let query = query.trim();
        if query.is_empty() {
            return Ok(Vec::new());
        }

        let chunks = self.collect_chunks(scenario)?;
        if chunks.is_empty() {
            return Ok(Vec::new());
        }

        let mut schema_builder = SchemaBuilder::default();
        let text_indexing = TextFieldIndexing::default()
            .set_tokenizer("default")
            .set_index_option(IndexRecordOption::WithFreqsAndPositions);
        let text_options = TextOptions::default()
            .set_indexing_options(text_indexing)
            .set_stored();

        let file_path_f = schema_builder.add_text_field("file_path", STORED);
        let title_f = schema_builder.add_text_field("title", STORED);
        let snippet_f = schema_builder.add_text_field("snippet", STORED);
        let content_f = schema_builder.add_text_field("content", text_options);

        let number_options = NumericOptions::default().set_stored().set_fast();
        let line_start_f = schema_builder.add_u64_field("line_start", number_options.clone());
        let line_end_f = schema_builder.add_u64_field("line_end", number_options);

        let schema = schema_builder.build();
        let index = Index::create_in_ram(schema);
        let mut writer = index
            .writer(50_000_000)
            .map_err(|e| CoreError::Unknown(format!("index writer failed: {e}")))?;

        for chunk in &chunks {
            let tokenized = self.tokenize_zh(&chunk.snippet);
            writer
                .add_document(doc!(
                    file_path_f => chunk.file_path.clone(),
                    title_f => chunk.title.clone(),
                    snippet_f => chunk.snippet.clone(),
                    content_f => tokenized,
                    line_start_f => u64::from(chunk.line_start),
                    line_end_f => u64::from(chunk.line_end),
                ))
                .map_err(|e| CoreError::Unknown(format!("index add document failed: {e}")))?;
        }

        writer
            .commit()
            .map_err(|e| CoreError::Unknown(format!("index commit failed: {e}")))?;

        let reader = index
            .reader_builder()
            .reload_policy(ReloadPolicy::Manual)
            .try_into()
            .map_err(|e| CoreError::Unknown(format!("index reader failed: {e}")))?;
        reader
            .reload()
            .map_err(|e| CoreError::Unknown(format!("index reload failed: {e}")))?;

        let searcher = reader.searcher();
        let query_parser = QueryParser::for_index(&index, vec![content_f]);
        let parsed_query = query_parser
            .parse_query(&self.tokenize_zh(query))
            .map_err(|e| CoreError::Unknown(format!("query parse failed: {e}")))?;

        let top_docs = searcher
            .search(&parsed_query, &TopDocs::with_limit(top_k))
            .map_err(|e| CoreError::Unknown(format!("search failed: {e}")))?;

        let mut results = Vec::with_capacity(top_docs.len());
        for (score, addr) in top_docs {
            let retrieved = searcher
                .doc::<tantivy::schema::TantivyDocument>(addr)
                .map_err(|e| CoreError::Unknown(format!("doc read failed: {e}")))?;

            let file_path = retrieved
                .get_first(file_path_f)
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .to_owned();
            let title = retrieved
                .get_first(title_f)
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .to_owned();
            let snippet = retrieved
                .get_first(snippet_f)
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .to_owned();
            let line_start = retrieved
                .get_first(line_start_f)
                .and_then(|v| v.as_u64())
                .unwrap_or_default() as u32;
            let line_end = retrieved
                .get_first(line_end_f)
                .and_then(|v| v.as_u64())
                .unwrap_or_default() as u32;

            results.push(SearchResult {
                file_path,
                title,
                snippet,
                line_start,
                line_end,
                score,
            });
        }

        Ok(results)
    }

    pub fn read_file(&self, file_path: &str) -> CoreResult<String> {
        let path = Path::new(file_path);
        fs::read_to_string(path)
            .map_err(|e| CoreError::Storage(format!("read kb file failed: {e}")))
    }

    pub fn knowledge_info(&self) -> CoreResult<KnowledgeInfo> {
        let files = self.collect_markdown_files(&self.kb_root)?;
        let mut latest_updated = 0_i64;

        for file in &files {
            if let Ok(meta) = fs::metadata(file) {
                if let Ok(modified) = meta.modified() {
                    if let Ok(duration) = modified.duration_since(UNIX_EPOCH) {
                        latest_updated = latest_updated.max(duration.as_secs() as i64);
                    }
                }
            }
        }

        Ok(KnowledgeInfo {
            kb_path: self.kb_root.to_string_lossy().to_string(),
            file_count: files.len() as u32,
            updated_at: latest_updated,
        })
    }

    fn tokenize_zh(&self, input: &str) -> String {
        self.jieba
            .cut(input, false)
            .into_iter()
            .filter(|token| !token.trim().is_empty())
            .collect::<Vec<_>>()
            .join(" ")
    }

    fn collect_chunks(&self, scenario: &str) -> CoreResult<Vec<KbChunk>> {
        let scenario_path = self.kb_root.join(scenario);
        let target_root = if scenario_path.exists() {
            scenario_path
        } else {
            self.kb_root.clone()
        };

        let files = self.collect_markdown_files(&target_root)?;
        let mut chunks = Vec::new();

        for file in files {
            let content = fs::read_to_string(&file)
                .map_err(|e| CoreError::Storage(format!("read kb file failed: {e}")))?;
            let title = extract_title(&file, &content);
            chunks.extend(chunk_markdown(&file, &title, &content, 20));
        }

        Ok(chunks)
    }

    fn collect_markdown_files(&self, root: &Path) -> CoreResult<Vec<PathBuf>> {
        if !root.exists() {
            return Ok(Vec::new());
        }

        let mut files = Vec::new();
        for entry in WalkDir::new(root).into_iter().flatten() {
            if !entry.file_type().is_file() {
                continue;
            }
            if entry
                .path()
                .extension()
                .map(|ext| ext.eq_ignore_ascii_case("md"))
                .unwrap_or(false)
            {
                files.push(entry.path().to_path_buf());
            }
        }

        files.sort();
        Ok(files)
    }
}

fn extract_title(file_path: &Path, content: &str) -> String {
    if let Some(title_line) = content
        .lines()
        .find(|line| line.trim_start().starts_with('#'))
    {
        return title_line.trim_start_matches('#').trim().to_owned();
    }

    file_path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("知识库文档")
        .to_owned()
}

fn chunk_markdown(
    file_path: &Path,
    title: &str,
    content: &str,
    lines_per_chunk: usize,
) -> Vec<KbChunk> {
    let lines: Vec<&str> = content.lines().collect();
    if lines.is_empty() {
        return Vec::new();
    }

    let mut chunks = Vec::new();
    let mut start = 0usize;

    while start < lines.len() {
        let end = (start + lines_per_chunk).min(lines.len());
        let snippet = lines[start..end].join("\n").trim().to_owned();

        if !snippet.is_empty() {
            chunks.push(KbChunk {
                file_path: file_path.to_string_lossy().to_string(),
                title: title.to_owned(),
                snippet,
                line_start: (start + 1) as u32,
                line_end: end as u32,
            });
        }

        start = end;
    }

    chunks
}

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::TempDir;

    use super::RetrievalEngine;

    fn setup_kb() -> (TempDir, RetrievalEngine) {
        let dir = TempDir::new().expect("temp dir");
        let labor_dir = dir.path().join("labor");
        let rental_dir = dir.path().join("rental");
        fs::create_dir_all(&labor_dir).expect("create labor dir");
        fs::create_dir_all(&rental_dir).expect("create rental dir");

        fs::write(
            labor_dir.join("wage.md"),
            "# 劳动仲裁流程\n拖欠工资可以申请劳动仲裁。\n准备劳动合同和工资流水。",
        )
        .expect("write labor file");

        fs::write(
            rental_dir.join("deposit.md"),
            "# 租房押金\n押金不退可提起诉讼或调解。",
        )
        .expect("write rental file");

        let engine = RetrievalEngine::new(dir.path());
        (dir, engine)
    }

    #[test]
    fn search_returns_labor_result() {
        let (_dir, engine) = setup_kb();

        let results = engine.search("拖欠工资", "labor", 5).expect("search labor");
        assert!(!results.is_empty());
        assert!(results[0].snippet.contains("拖欠工资"));
    }

    #[test]
    fn scenario_isolation_works() {
        let (_dir, engine) = setup_kb();

        let results = engine
            .search("押金", "labor", 5)
            .expect("search labor with rental term");

        // labor 场景不应直接命中 rental 文档
        assert!(results
            .iter()
            .all(|item| item.file_path.contains("labor") || !item.file_path.contains("rental")));
    }

    #[test]
    fn empty_index_returns_empty() {
        let dir = TempDir::new().expect("temp dir");
        let engine = RetrievalEngine::new(dir.path());
        let results = engine.search("劳动仲裁", "labor", 3).expect("search empty");
        assert!(results.is_empty());
    }

    #[test]
    fn result_contains_file_and_line_range() {
        let (_dir, engine) = setup_kb();
        let results = engine.search("劳动仲裁", "labor", 1).expect("search");
        let first = results.first().expect("has result");

        assert!(first.file_path.ends_with("wage.md"));
        assert!(first.line_start >= 1);
        assert!(first.line_end >= first.line_start);
    }
}
