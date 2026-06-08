#[derive(Debug, serde::Serialize)]
pub(super) struct EditResult {
    pub(super) operation: String,
    pub(super) file_path: String,
    pub(super) applied: bool,
    pub(super) dry_run: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) diff: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) summary: Option<String>,
}
