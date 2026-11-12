use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct AppConfig {
    pub theme: String,
    pub selected_engine: Option<String>,
    pub show_performance_overlay: bool,
}
impl Default for AppConfig {
    fn default() -> Self {
        Self {
            theme: "dark".into(),
            selected_engine: None,
            show_performance_overlay: false,
        }
    }
}
pub fn parse_config(json: &str) -> Result<AppConfig, serde_json::Error> {
    serde_json::from_str(json)
}
#[cfg(test)]
mod tests {
    #[test]
    fn defaults_are_applied() {
        let c = super::parse_config("{}").unwrap();
        assert_eq!(c.theme, "dark");
    }
}
