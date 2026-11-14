#![cfg(test)]

//! Integration tests for the surface/presentation layer.

use fu_render3d::backend::{BackendCapabilities, RenderBackend, Viewport};
use fu_render3d::camera::Camera;
use fu_render3d::quality::{Platform, QualityLevel, QualitySettings};
use fu_render3d::renderer::Renderer3D;
use glam::Vec3;

use crate::backend::WgpuBackend;
use crate::surface::{select_present_mode, select_surface_format};

// ─────────────────────────────────────────────────────────────────────────────
// Group 1: Surface format selection (pure logic, no GPU needed)
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn surface_format_prefers_srgb() {
    let candidates = [
        wgpu::TextureFormat::Rgba8Unorm,
        wgpu::TextureFormat::Bgra8UnormSrgb,
        wgpu::TextureFormat::Rgba8UnormSrgb,
    ];
    let result = select_surface_format(&candidates, true);
    assert!(
        matches!(
            result,
            Some(wgpu::TextureFormat::Bgra8UnormSrgb) | Some(wgpu::TextureFormat::Rgba8UnormSrgb)
        ),
        "expected an sRGB format, got {:?}",
        result
    );
}

#[test]
fn surface_format_falls_back_to_first_when_no_srgb() {
    let candidates = [
        wgpu::TextureFormat::Rgba8Unorm,
        wgpu::TextureFormat::Bgra8Unorm,
    ];
    let result = select_surface_format(&candidates, true);
    // No sRGB candidate available — must return the first element.
    assert_eq!(result, Some(wgpu::TextureFormat::Rgba8Unorm));
}

#[test]
fn surface_format_returns_none_for_empty() {
    let result = select_surface_format(&[], true);
    assert_eq!(result, None);
}

#[test]
fn surface_format_no_srgb_preference_returns_first() {
    // prefer_srgb=false → always return the first candidate regardless of sRGB.
    let candidates = [
        wgpu::TextureFormat::Rgba8UnormSrgb,
        wgpu::TextureFormat::Bgra8Unorm,
    ];
    let result = select_surface_format(&candidates, false);
    assert_eq!(result, Some(wgpu::TextureFormat::Rgba8UnormSrgb));
}

// ─────────────────────────────────────────────────────────────────────────────
// Group 2: Present mode selection (pure logic, no GPU needed)
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn present_mode_vsync_always_fifo() {
    // vsync=true must return Fifo regardless of what the surface supports.
    let supported = [
        wgpu::PresentMode::Mailbox,
        wgpu::PresentMode::Immediate,
        wgpu::PresentMode::Fifo,
    ];
    let mode = select_present_mode(&supported, true);
    assert_eq!(mode, wgpu::PresentMode::Fifo);
}

#[test]
fn present_mode_no_vsync_prefers_mailbox() {
    let supported = [
        wgpu::PresentMode::Mailbox,
        wgpu::PresentMode::Immediate,
        wgpu::PresentMode::Fifo,
    ];
    let mode = select_present_mode(&supported, false);
    assert_eq!(mode, wgpu::PresentMode::Mailbox);
}

#[test]
fn present_mode_no_vsync_falls_back_to_immediate() {
    // Mailbox not available — should prefer Immediate over Fifo.
    let supported = [wgpu::PresentMode::Immediate, wgpu::PresentMode::Fifo];
    let mode = select_present_mode(&supported, false);
    assert_eq!(mode, wgpu::PresentMode::Immediate);
}

#[test]
fn present_mode_no_vsync_falls_back_to_fifo_when_neither_available() {
    // Neither Mailbox nor Immediate available — must fall back to Fifo.
    let supported = [wgpu::PresentMode::Fifo];
    let mode = select_present_mode(&supported, false);
    assert_eq!(mode, wgpu::PresentMode::Fifo);
}

// ─────────────────────────────────────────────────────────────────────────────
// Group 3: Render target cache invalidation (needs GPU, skip if none)
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn render_target_cache_reuses_on_same_dims() {
    let mut backend = match WgpuBackend::new_headless() {
        Ok(b) => b,
        Err(e) => {
            eprintln!("Skipping test on system with no GPU adapter: {e}");
            return;
        }
    };

    // First call: allocates targets.
    backend.ensure_targets(800, 600, 1);
    let depth_before = backend
        .current_depth_texture
        .as_ref()
        .map(|(_, _, _, tex, _)| tex.clone());

    // Second call with identical parameters: nothing should be reallocated.
    backend.ensure_targets(800, 600, 1);
    let depth_after = backend
        .current_depth_texture
        .as_ref()
        .map(|(_, _, _, tex, _)| tex.clone());

    assert_eq!(
        depth_before, depth_after,
        "depth texture was unexpectedly reallocated on same-dims ensure_targets"
    );
}

#[test]
fn render_target_cache_invalidates_on_resize() {
    let mut backend = match WgpuBackend::new_headless() {
        Ok(b) => b,
        Err(e) => {
            eprintln!("Skipping test on system with no GPU adapter: {e}");
            return;
        }
    };

    backend.ensure_targets(800, 600, 1);
    let color_before = backend
        .current_color_texture
        .as_ref()
        .map(|(_, _, tex, _)| tex.clone());

    // Different dimensions → both color and depth must be reallocated.
    backend.ensure_targets(1280, 720, 1);
    let color_after = backend
        .current_color_texture
        .as_ref()
        .map(|(_, _, tex, _)| tex.clone());

    assert_ne!(
        color_before, color_after,
        "color texture was NOT reallocated after resize"
    );

    // Verify cached dimensions updated.
    let (w, h, _, _) = backend.current_color_texture.as_ref().unwrap();
    assert_eq!((*w, *h), (1280, 720));
}

#[test]
fn render_target_cache_invalidates_depth_on_msaa_change() {
    let mut backend = match WgpuBackend::new_headless() {
        Ok(b) => b,
        Err(e) => {
            eprintln!("Skipping test on system with no GPU adapter: {e}");
            return;
        }
    };

    // Initial allocation at samples=1.
    backend.ensure_targets(800, 600, 1);
    let depth_before = backend
        .current_depth_texture
        .as_ref()
        .map(|(_, _, _, tex, _)| tex.clone());

    // Change MSAA — depth must be reallocated (sample count changed).
    backend.ensure_targets(800, 600, 4);
    let depth_after = backend
        .current_depth_texture
        .as_ref()
        .map(|(_, _, _, tex, _)| tex.clone());

    assert_ne!(
        depth_before, depth_after,
        "depth texture was NOT reallocated after MSAA change"
    );

    // Verify MSAA texture was also allocated.
    assert!(
        backend.current_msaa_texture.is_some(),
        "MSAA texture should be allocated when samples > 1"
    );
    let (_, _, samples, _, _) = backend.current_msaa_texture.as_ref().unwrap();
    assert_eq!(*samples, 4);
}

// ─────────────────────────────────────────────────────────────────────────────
// Group 4: Zero-sized viewport (needs GPU, skip if none)
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn zero_sized_viewport_returns_error() {
    let mut backend = match WgpuBackend::new_headless() {
        Ok(b) => b,
        Err(e) => {
            eprintln!("Skipping test on system with no GPU adapter: {e}");
            return;
        }
    };

    use crate::backend::register_builtin_shaders;
    let quality = QualitySettings::preset(QualityLevel::Low, Platform::Desktop);
    let mut renderer = Renderer3D::new(quality.clone()).expect("renderer creation failed");
    register_builtin_shaders(&mut renderer.shaders).expect("shader registration failed");

    let camera = Camera {
        transform: fu_render3d::transform::Transform::from_translation(Vec3::new(0.0, 0.0, 5.0)),
        ..Default::default()
    };

    // Zero-width viewport must be rejected by renderer.
    let result_w0 = renderer.render(
        &camera,
        Viewport {
            width: 0,
            height: 600,
        },
        &mut backend,
    );
    assert!(
        result_w0.is_err(),
        "zero-width viewport should be rejected by renderer"
    );

    // Zero-height viewport must be rejected by renderer.
    let result_h0 = renderer.render(
        &camera,
        Viewport {
            width: 800,
            height: 0,
        },
        &mut backend,
    );
    assert!(
        result_h0.is_err(),
        "zero-height viewport should be rejected by renderer"
    );

    // Verify WgpuBackend guard directly handles zero-sized viewport safely without panicking.
    let queue = fu_render3d::queue::DrawQueue::default();
    let meshes = fu_render3d::resources::Store::default();
    let materials = fu_render3d::resources::Store::default();
    let textures = fu_render3d::resources::Store::default();
    let frame_zero = fu_render3d::backend::FramePacket {
        viewport: Viewport {
            width: 0,
            height: 0,
        },
        composition: fu_render3d::backend::Composition::default(),
        view: glam::Mat4::IDENTITY,
        projection: glam::Mat4::IDENTITY,
        camera_position: Vec3::ZERO,
        quality: &quality,
        queue: &queue,
        lights: vec![],
        ambient_color: [0.0; 3],
        ambient_intensity: 0.0,
        meshes: &meshes,
        materials: &materials,
        textures: &textures,
        shaders: &renderer.shaders,
    };
    assert!(backend.render(&frame_zero).is_ok());
}

// ─────────────────────────────────────────────────────────────────────────────
// Group 5: Quality clamping via BackendCapabilities (pure logic, no GPU)
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn quality_clamping_respects_texture_limit() {
    let caps = BackendCapabilities {
        max_texture_size: 1024,
        max_msaa_samples: 4,
        supports_shadows: true,
    };

    let requested = QualitySettings::preset(QualityLevel::High, Platform::Desktop);
    assert!(
        requested.max_texture_size > caps.max_texture_size,
        "precondition: high preset must request larger texture than cap"
    );

    let clamped = caps.clamp_quality(&requested);
    assert_eq!(
        clamped.max_texture_size, 1024,
        "clamped texture size must equal the backend cap"
    );
    assert_eq!(clamped.max_lights, requested.max_lights);
    assert_eq!(clamped.render_scale, requested.render_scale);
}

#[test]
fn quality_clamping_respects_msaa_limit() {
    let caps = BackendCapabilities {
        max_texture_size: 8192,
        max_msaa_samples: 2,
        supports_shadows: true,
    };

    let requested = QualitySettings::preset(QualityLevel::High, Platform::Desktop);
    assert_eq!(
        requested.msaa_samples, 4,
        "precondition: high preset must be 4x MSAA"
    );

    let clamped = caps.clamp_quality(&requested);
    assert!(
        clamped.msaa_samples <= caps.max_msaa_samples,
        "clamped MSAA ({}) must not exceed backend cap ({})",
        clamped.msaa_samples,
        caps.max_msaa_samples
    );
    assert!(matches!(clamped.msaa_samples, 1 | 2));
}

#[test]
fn quality_clamping_disables_shadows_when_unsupported() {
    let caps = BackendCapabilities {
        max_texture_size: 8192,
        max_msaa_samples: 4,
        supports_shadows: false,
    };

    let requested = QualitySettings::preset(QualityLevel::High, Platform::Desktop);
    assert!(
        requested.shadow_map_size.is_some(),
        "precondition: high preset includes shadows"
    );

    let clamped = caps.clamp_quality(&requested);
    assert_eq!(
        clamped.shadow_map_size, None,
        "shadows must be disabled when backend reports supports_shadows=false"
    );
}
