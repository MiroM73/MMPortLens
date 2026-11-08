//! Window size limit of the GPU renderer (#43).
//!
//! eframe draws with wgpu, whose surface (and the depth / MSAA textures egui creates with it)
//! cannot be larger than the device's maximum texture size. egui-wgpu asks for a device with
//! 8192 px, so a window taller or wider than 8192 physical pixels – three 4K monitors side by
//! side, or a tool that moves windows – ended the program with a validation error in
//! `Surface::configure`. Three layers prevent that:
//!
//! - [`wgpu_options`] asks for the largest texture size the adapter supports (16384 or more on
//!   current GPUs) instead of the fixed 8192;
//! - [`limit_window_size`], called at the start of every frame of every viewport, gives the
//!   window a maximum inner size of that limit (Windows then stops a resize by the user at it) and
//!   shrinks a window that got larger anyway – `MoveWindow` / `SetWindowPos` of another program
//!   are not clamped by Windows (checked: a 32728 px high `MoveWindow` went through);
//! - [`keep_running_on_gpu_errors`] replaces wgpu's default error handler, which panics, with one
//!   that logs: the surface of such a window cannot be configured until it is shrunk in the next
//!   frame, and the program must not end in between.

use eframe::egui;
use eframe::egui_wgpu::{WgpuConfiguration, WgpuSetup, wgpu};
use std::sync::Arc;

/// Renderer options: the default ones, but with the adapter's maximum texture size.
pub fn wgpu_options() -> WgpuConfiguration {
    let mut options = WgpuConfiguration::default();
    if let WgpuSetup::CreateNew(setup) = &mut options.wgpu_setup {
        let default_descriptor = Arc::clone(&setup.device_descriptor);
        setup.device_descriptor = Arc::new(move |adapter: &wgpu::Adapter| {
            let mut descriptor = default_descriptor(adapter);
            let supported = adapter.limits().max_texture_dimension_2d;
            descriptor.required_limits.max_texture_dimension_2d = supported;
            descriptor
        });
    }
    options
}

/// Window options of the program's windows: the renderer options of [`wgpu_options`].
pub fn native_options(viewport: egui::ViewportBuilder) -> eframe::NativeOptions {
    eframe::NativeOptions {
        viewport,
        wgpu_options: wgpu_options(),
        ..Default::default()
    }
}

/// Largest inner size in points for a window drawn with textures of at most `max_side` pixels
/// at `pixels_per_point`. `None` when the values make no sense (no limit then).
pub fn max_inner_points(max_side: usize, pixels_per_point: f32) -> Option<egui::Vec2> {
    if max_side == 0 || !pixels_per_point.is_finite() || pixels_per_point <= 0.0 {
        return None;
    }
    // One pixel of margin for the rounding of points to pixels.
    let side = (max_side as f32 - 1.0) / pixels_per_point;
    Some(egui::vec2(side, side))
}

/// The size a window of `inner` points must be shrunk to so it fits `max`; `None` when it fits.
pub fn shrink_to(inner: egui::Vec2, max: egui::Vec2) -> Option<egui::Vec2> {
    (inner.x > max.x || inner.y > max.y).then(|| inner.min(max))
}

/// Sets the maximum inner size of the current viewport from the renderer's texture limit (sent
/// only when the limit or the scale changed, stored per viewport in the context's memory) and
/// shrinks the window when it is larger than that.
pub fn limit_window_size(ctx: &egui::Context) {
    let (max_side, ppp, inner) = ctx.input(|i| {
        (
            i.max_texture_side,
            i.pixels_per_point,
            i.viewport().inner_rect.map(|r| r.size()),
        )
    });
    let Some(max) = max_inner_points(max_side, ppp) else {
        return;
    };
    let id = egui::Id::unique(("max_inner_size", ctx.viewport_id()));
    let sent = ctx.data(|d| d.get_temp::<egui::Vec2>(id));
    if sent != Some(max) {
        ctx.data_mut(|d| d.insert_temp(id, max));
        ctx.send_viewport_cmd(egui::ViewportCommand::MaxInnerSize(max));
    }
    if let Some(size) = inner.and_then(|s| shrink_to(s, max)) {
        ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(size));
    }
}

/// Makes wgpu errors of the renderer log instead of panic (wgpu's default handler panics). Call
/// it in every frame; only the first call per device does something.
pub fn keep_running_on_gpu_errors(frame: &eframe::Frame) {
    use std::sync::atomic::{AtomicBool, Ordering};
    static INSTALLED: AtomicBool = AtomicBool::new(false);
    let Some(state) = frame.wgpu_render_state() else {
        return;
    };
    if INSTALLED.swap(true, Ordering::Relaxed) {
        return;
    }
    state
        .device
        .on_uncaptured_error(Arc::new(|e: wgpu::Error| eprintln!("wgpu error: {e}")));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn max_size_in_points_stays_below_the_texture_limit() {
        for ppp in [1.0f32, 1.25, 1.5, 1.75, 2.0, 3.0] {
            let max = max_inner_points(8192, ppp).unwrap();
            assert!(max.x * ppp <= 8192.0, "{ppp}: {}", max.x * ppp);
            assert!(max.x * ppp >= 8190.0, "{ppp}: {}", max.x * ppp);
            assert_eq!(max.x, max.y);
        }
    }

    #[test]
    fn only_a_too_large_window_is_shrunk() {
        let max = egui::vec2(8000.0, 8000.0);
        assert_eq!(shrink_to(egui::vec2(1500.0, 900.0), max), None);
        assert_eq!(shrink_to(egui::vec2(8000.0, 8000.0), max), None);
        assert_eq!(
            shrink_to(egui::vec2(1484.0, 32728.0), max),
            Some(egui::vec2(1484.0, 8000.0))
        );
    }

    #[test]
    fn nonsense_values_give_no_limit() {
        assert_eq!(max_inner_points(0, 1.0), None);
        assert_eq!(max_inner_points(8192, 0.0), None);
        assert_eq!(max_inner_points(8192, f32::NAN), None);
    }
}
