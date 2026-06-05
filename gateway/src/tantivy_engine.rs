// gateway/src/tantivy_engine.rs
//
// JIT Karpathy Compiler — BM25 skill search via embedded Tantivy RAM index.
// Error hashes are indexed with their verified markdown strategies.
// LIFO prioritization: latest strategy wins on collision.
// Live reload is triggered by the R&D loop after each new skill consolidation.

use std::sync::Arc;
use anyhow::{anyhow, Result};
use tantivy::{
    collector::TopDocs,
    doc,
    query::QueryParser,
    schema::{Schema, Value, STORED, TEXT},
    Index, IndexWriter, TantivyDocument,
};
use tokio::sync::RwLock;
use tracing::{info, warn};

// ─────────────────────────────────────────────────────────────────────────────
// Schema definition
// ─────────────────────────────────────────────────────────────────────────────

#[derive(Clone)]
pub struct TantivySchema {
    pub schema: Schema,
    pub field_hash: tantivy::schema::Field,
    pub field_strategy: tantivy::schema::Field,
    pub field_timestamp: tantivy::schema::Field,
}

impl TantivySchema {
    pub fn build() -> Self {
        let mut schema_builder = Schema::builder();
        let field_hash = schema_builder.add_text_field("hash", TEXT | STORED);
        let field_strategy = schema_builder.add_text_field("strategy", TEXT | STORED);
        let field_timestamp = schema_builder.add_text_field("timestamp", STORED);
        let schema = schema_builder.build();
        TantivySchema { schema, field_hash, field_strategy, field_timestamp }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// TantivyEngine — thread-safe index with live reload support
// ─────────────────────────────────────────────────────────────────────────────

pub struct TantivyEngine {
    schema: TantivySchema,
    /// The index lives in RAM (no disk I/O)
    index: Index,
    writer: Arc<RwLock<IndexWriter>>,
}

impl TantivyEngine {
    pub fn new() -> Result<Self> {
        let schema = TantivySchema::build();

        // RAM directory — embedded inside the Axum process
        let index = Index::create_in_ram(schema.schema.clone());

        // 50MB RAM budget for the writer (matches spec)
        let writer = index.writer(50_000_000)?;

        info!("Tantivy BM25 RAM index initialized (50MB budget)");

        Ok(TantivyEngine {
            schema,
            index,
            writer: Arc::new(RwLock::new(writer)),
        })
    }

    /// Index a new skill (called on boot from OmniMem warm-up, and after R&D consolidation)
    pub async fn index_skill(&self, error_hash: &str, strategy: &str) -> Result<()> {
        let timestamp = chrono::Utc::now().to_rfc3339();
        let mut writer = self.writer.write().await;

        // Delete existing document for this hash (LIFO: new version wins)
        let hash_term = tantivy::Term::from_field_text(self.schema.field_hash, error_hash);
        writer.delete_term(hash_term);

        // Add new document
        writer.add_document(doc!(
            self.schema.field_hash => error_hash,
            self.schema.field_strategy => strategy,
            self.schema.field_timestamp => timestamp.as_str(),
        ))?;

        writer.commit()?;

        info!(hash = %error_hash, "Skill indexed in Tantivy (LIFO update)");
        Ok(())
    }

    /// Search for the best strategy matching an error hash or description.
    /// Returns the single most recently indexed matching strategy (LIFO).
    pub fn search_skill(&self, query_str: &str) -> Result<Option<String>> {
        let reader = self.index.reader()?;
        let searcher = reader.searcher();

        let query_parser = QueryParser::for_index(
            &self.index,
            vec![self.schema.field_hash, self.schema.field_strategy],
        );

        let query = match query_parser.parse_query(query_str) {
            Ok(q) => q,
            Err(e) => {
                warn!(query = %query_str, error = %e, "Query parse failed — no skill injection");
                return Ok(None);
            }
        };

        // Get top 1 result (LIFO — latest indexed wins due to delete+reindex)
        let top_docs = searcher.search(&query, &TopDocs::with_limit(1))?;

        if top_docs.is_empty() {
            return Ok(None);
        }

        let (_score, doc_address) = top_docs[0];
        let doc: TantivyDocument = searcher.doc(doc_address)?;

        let strategy = doc
            .get_first(self.schema.field_strategy)
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        Ok(strategy)
    }

    /// Exact hash lookup — used when the Gateway already computed the SHA256
    pub fn lookup_by_hash(&self, error_hash: &str) -> Result<Option<String>> {
        // Escape the hash for Tantivy term query
        let query_str = format!("hash:\"{}\"", error_hash);
        self.search_skill(&query_str)
    }

    /// Total documents in index (for status endpoint)
    pub fn doc_count(&self) -> u64 {
        self.index
            .reader()
            .ok()
            .map(|r| r.searcher().num_docs())
            .unwrap_or(0)
    }
}

pub type SharedTantivyEngine = Arc<TantivyEngine>;

pub fn new_tantivy_engine() -> Result<SharedTantivyEngine> {
    Ok(Arc::new(TantivyEngine::new()?))
}
