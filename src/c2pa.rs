//! C2PA content credentials: sign + verify of provenance manifests.

/// Watermark metadata carried inside the C2PA manifest assertion
/// (`com.sigil.watermark`), mirroring the pixel-watermark embed parameters.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct WatermarkClaim {
    pub mode: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub recipient_id: Option<String>,
    pub keyed: bool,
}
