use fu_vfs::VfsPath;

/// Trait for types that can be decoded from raw bytes and cached in [`crate::AssetCache`].
pub trait Asset: Send + Sync + 'static {
    /// Error type produced during asset decoding.
    type Error: std::error::Error + Send + Sync + 'static;

    /// Decodes an asset from its raw bytes.
    ///
    /// The virtual path is provided for diagnostic and metadata purposes.
    fn decode(bytes: &[u8], path: &VfsPath) -> Result<Self, Self::Error>
    where
        Self: Sized;
}

impl Asset for Vec<u8> {
    type Error = std::convert::Infallible;

    fn decode(bytes: &[u8], _path: &VfsPath) -> Result<Self, Self::Error> {
        Ok(bytes.to_vec())
    }
}

impl Asset for String {
    type Error = std::string::FromUtf8Error;

    fn decode(bytes: &[u8], _path: &VfsPath) -> Result<Self, Self::Error> {
        String::from_utf8(bytes.to_vec())
    }
}

impl Asset for serde_json::Value {
    type Error = serde_json::Error;

    fn decode(bytes: &[u8], _path: &VfsPath) -> Result<Self, Self::Error> {
        serde_json::from_slice(bytes)
    }
}
