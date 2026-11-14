use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeviceClass {
    Desktop,
    Mobile,
}

/// What the GPU backend reports it can do. The renderer fills this in once it
/// exists; until then use [`DeviceCaps::conservative`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeviceCaps {
    pub class: DeviceClass,
    pub max_texture_size: u32,
    pub max_msaa_samples: u8,
    pub max_anisotropy: u8,
    pub supports_shadow_maps: bool,
    pub supports_float_targets: bool,
}

impl DeviceCaps {
    /// Safe floor used when nothing has been reported yet.
    pub fn conservative(class: DeviceClass) -> Self {
        Self {
            class,
            max_texture_size: match class {
                DeviceClass::Mobile => 2048,
                DeviceClass::Desktop => 4096,
            },
            max_msaa_samples: 1,
            max_anisotropy: 1,
            supports_shadow_maps: false,
            supports_float_targets: false,
        }
    }

    /// Test fixture. NOT a measurement of any real GPU.
    pub fn desktop_fixture() -> Self {
        Self {
            class: DeviceClass::Desktop,
            max_texture_size: 8192,
            max_msaa_samples: 4,
            max_anisotropy: 16,
            supports_shadow_maps: true,
            supports_float_targets: true,
        }
    }

    /// Test fixture. NOT a measurement of any real GPU.
    pub fn mobile_fixture() -> Self {
        Self {
            class: DeviceClass::Mobile,
            max_texture_size: 4096,
            max_msaa_samples: 4,
            max_anisotropy: 4,
            supports_shadow_maps: true,
            supports_float_targets: false,
        }
    }
}
