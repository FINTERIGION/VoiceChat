pub mod clone;
pub mod sample;
pub mod service;

/// Turns arbitrary user text (e.g. a character name) into a short,
/// API-safe prefix for voice-enrollment requests.
pub fn slugify(input: &str) -> String {
    let lower: String = input
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .collect::<String>()
        .to_lowercase();
    if lower.is_empty() {
        uuid::Uuid::new_v4().simple().to_string()[..8].to_string()
    } else {
        lower.chars().take(16).collect()
    }
}
