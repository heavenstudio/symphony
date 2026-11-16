//! The statements of `store/embeddings.rs`.

/// Every source the board embeds: each task's brief, its title beside it, and each message that
/// is not the board's own record.
pub(in crate::store) const SOURCES: &str = "
LET $briefs = SELECT slug, 'brief' AS source, title, body AS text FROM task;
LET $messages = SELECT slug, string::concat('message:', <string> sid) AS source, text FROM message WHERE kind != 'system';
RETURN { briefs: $briefs, messages: $messages };
";

/// The source `$source` of `$slug`, of `$hash` under `$model`, published: its chunks replaced by
/// `$chunks` and its manifest written, the board's checkpoint dropped.
const PUBLISH: &str = "
DELETE task_embedding_checkpoint RETURN NONE;
DELETE task_chunk WHERE slug = $slug AND source = $source RETURN NONE;
FOR $chunk IN $chunks {
    CREATE type::record('task_chunk', [$slug, $source, $chunk.part]) SET slug = $slug, source = $source,
        source_hash = $hash, text_hash = $chunk.text_hash, model = $model, text = $chunk.text, embedding = $chunk.embedding RETURN NONE;
};
UPSERT type::record('task_embedding_source', [$slug, $source]) SET slug = $slug, source = $source, source_hash = $hash, model = $model, parts = array::len($chunks) RETURN NONE;
";

/// The digest of what a whole-board pass embeds under `$model` (the message counter and every
/// task's stamps), and whether it is the checkpoint's.
pub(in crate::store) const CHECKPOINT: &str = "LET $digest = crypto::sha256(<string> {model: $model,
                message: {last: (SELECT VALUE last FROM ONLY counter:message) ?? 0,
                    ts: (SELECT VALUE ts FROM ONLY counter:message) ?? 0},
                tasks: (SELECT slug, updated, version FROM task ORDER BY slug)});
             RETURN {digest: $digest,
                current: (SELECT VALUE digest FROM ONLY task_embedding_checkpoint:board) = $digest};";

/// The board's checkpoint kept as `$digest`.
pub(in crate::store) const KEEP_CHECKPOINT: &str =
    "UPSERT task_embedding_checkpoint:board CONTENT {digest: $digest} RETURN NONE; RETURN NONE;";

/// The sources ([`SOURCES`]) of the briefs of `$brief_slugs` and the messages `$message_keys`.
pub(in crate::store) const SOURCES_NAMED: &str = "LET $brief_records = $brief_slugs.map(|$slug| type::record('task', $slug));
             LET $message_records = $message_keys.map(|$key| type::record('message', $key));
             LET $briefs = SELECT slug, 'brief' AS source, title, body AS text FROM $brief_records;
             LET $messages = SELECT slug, string::concat('message:', <string> sid) AS source, text FROM $message_records WHERE kind != 'system';
             RETURN { briefs: $briefs, messages: $messages };";

/// Every source's manifest.
pub(in crate::store) const CACHED: &str = "RETURN SELECT * FROM task_embedding_source;";

/// How many chunks each source has.
pub(in crate::store) const PARTS: &str =
    "RETURN SELECT slug, source, count() AS parts FROM task_chunk GROUP BY slug, source;";

/// The chunks and manifests of the sources `$removed`, with the board's checkpoint, deleted.
pub(in crate::store) const FORGET: &str = "DELETE task_chunk WHERE [slug, source] IN $removed RETURN NONE; DELETE task_embedding_source WHERE [slug, source] IN $removed RETURN NONE; DELETE task_embedding_checkpoint RETURN NONE; RETURN NONE;";

/// A source published ([`PUBLISH`]) from `$chunks`, as a request of its own.
pub(in crate::store) fn publish() -> String {
    format!("{PUBLISH}\nRETURN NONE;")
}

/// A source published ([`PUBLISH`]) from the chunks staged in `{staging}_chunks`, which is removed.
pub(in crate::store) fn publish_staged(staging: &str) -> String {
    format!(
        "LET $chunks = (SELECT VALUE row FROM {staging}_chunks);\n{PUBLISH}\nREMOVE TABLE {staging}_chunks; RETURN NONE;"
    )
}

/// The staging table `{staging}_chunks` removed, if it is there: a staging request.
pub(in crate::store) fn remove_staged(staging: &str) -> String {
    super::copy::staging_request(&format!("REMOVE TABLE IF EXISTS {staging}_chunks;"))
}

/// How many chunks the board holds, of every model.
pub(in crate::store) const TOTAL: &str =
    "RETURN (SELECT count() AS total FROM task_chunk GROUP ALL)[0].total ?? 0;";

/// Every chunk of `$model` with its cosine distance from `$vector`, nearest first; `filtered`,
/// only those of the tasks `$eligible`.
pub(in crate::store) fn exact_neighbours(filtered: bool) -> String {
    let filter = if filtered {
        " AND slug IN $eligible"
    } else {
        ""
    };
    format!(
        "RETURN SELECT slug, source, source_hash, text, 1 - vector::similarity::cosine(embedding, $vector) AS distance FROM task_chunk WHERE model = $model{filter} ORDER BY distance ASC, slug ASC;"
    )
}

/// The `count` chunks the index finds nearest `$vector`, those of `$model`, nearest first.
pub(in crate::store) fn nearest_neighbours(count: usize) -> String {
    format!(
        "RETURN SELECT slug, source, source_hash, text, vector::distance::knn() AS distance FROM task_chunk WHERE embedding <|{count}, {}|> $vector AND model = $model ORDER BY distance ASC, slug ASC;",
        count.max(100)
    )
}

/// Whether the board holds a chunk of `$model`.
pub(in crate::store) const HAS: &str =
    "RETURN count(SELECT id FROM task_chunk WHERE model = $model LIMIT 1) > 0;";
