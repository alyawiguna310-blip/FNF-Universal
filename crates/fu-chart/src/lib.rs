use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ChartMetadata {
    pub song_name: Option<String>,
    pub bpm: Option<f64>,
    pub note_count: Option<usize>,
}

pub fn inspect_json_metadata(input: &str) -> anyhow::Result<ChartMetadata> {
    let value: serde_json::Value = serde_json::from_str(input)?;
    let song_name = value.get("song").and_then(|v| v.as_str()).map(str::to_owned)
        .or_else(|| value.get("songName").and_then(|v| v.as_str()).map(str::to_owned));
    let bpm = value.get("bpm").and_then(|v| v.as_f64());
    let note_count = value.get("notes").and_then(|v| v.as_array()).map(Vec::len);
    Ok(ChartMetadata { song_name, bpm, note_count })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test] fn reads_simple_metadata() {
        let m = inspect_json_metadata(r#"{"song":"Tutorial","bpm":100,"notes":[{},{}]}"#).unwrap();
        assert_eq!(m.song_name.as_deref(), Some("Tutorial"));
        assert_eq!(m.note_count, Some(2));
    }
}
