//! Display settings
//!
//! Monitor resolution, refresh rate, and display configuration.
//!
//! Provider selection:
//! - X11 (any WM): uses xrandr directly
//! - Wayland + Sway: uses swaymsg
//! - Wayland + instantWM: uses instantwmctl
//! - Wayland + Hyprland: uses hyprctl

use anyhow::Result;

use crate::common::compositor::CompositorType;
use crate::common::display::{
    DisplayMode, HyprlandDisplayProvider, InstantWMDisplayProvider, SwayDisplayProvider,
    XrandrDisplayProvider,
};
use crate::common::display_server::DisplayServer;
use crate::menu_utils::{FzfPreview, FzfSelectable, FzfWrapper};
use crate::settings::context::SettingsContext;
use crate::settings::setting::{Setting, SettingMetadata, SettingType};
use crate::ui::catppuccin::colors;
use crate::ui::prelude::*;
use crate::ui::preview::PreviewBuilder;

// ============================================================================
// Display Configuration
// ============================================================================

pub struct ConfigureDisplay;

#[derive(Clone)]
struct ModeMenuItem {
    output_name: String,
    mode: DisplayMode,
    current_mode: DisplayMode,
}

impl FzfSelectable for ModeMenuItem {
    fn fzf_display_text(&self) -> String {
        let label = self.mode.display_format();
        if self.mode == self.current_mode {
            format!("{} (current)", label)
        } else {
            label
        }
    }

    fn fzf_key(&self) -> String {
        format!(
            "{}x{}@{}",
            self.mode.width, self.mode.height, self.mode.refresh
        )
    }

    fn fzf_preview(&self) -> FzfPreview {
        build_mode_preview(&self.output_name, &self.mode, &self.current_mode)
    }
}

fn build_mode_preview(
    output_name: &str,
    mode: &DisplayMode,
    current_mode: &DisplayMode,
) -> FzfPreview {
    let delta_pixels = mode.resolution() as i128 - current_mode.resolution() as i128;
    let delta_refresh = mode.refresh as i128 - current_mode.refresh as i128;
    let same_resolution = mode.width == current_mode.width && mode.height == current_mode.height;
    let same_refresh = mode.refresh == current_mode.refresh;
    let pixel_cmp = compare_pixels(delta_pixels);
    let refresh_cmp = compare_refresh(delta_refresh);

    let candidate_aspect = aspect_ratio_label(mode.width, mode.height);
    let current_aspect = aspect_ratio_label(current_mode.width, current_mode.height);
    let candidate_mp = megapixels(mode);
    let current_mp = megapixels(current_mode);

    let mut builder = PreviewBuilder::new()
        .header(
            NerdFont::Monitor,
            &format!("{} {}", output_name, mode.display_format()),
        )
        .field(
            "Candidate",
            &format!(
                "{} ({candidate_aspect}, {candidate_mp:.2} MP)",
                mode.display_format()
            ),
        )
        .field(
            "Current",
            &format!(
                "{} ({current_aspect}, {current_mp:.2} MP)",
                current_mode.display_format()
            ),
        )
        .blank()
        .line(colors::TEAL, Some(NerdFont::GitCompare), &pixel_cmp)
        .line(colors::TEAL, Some(NerdFont::Refresh), &refresh_cmp)
        .blank()
        .line(colors::MAUVE, Some(NerdFont::Table), "Visual Comparison")
        .subtext("`#` candidate, `.` current, `*` overlap")
        .blank();

    for line in render_mode_comparison_ascii(mode, current_mode) {
        builder = builder.raw(&line);
    }

    if same_resolution && same_refresh {
        builder = builder
            .blank()
            .subtext("This entry matches the active mode.");
    } else if same_resolution {
        builder = builder
            .blank()
            .subtext("Resolution matches current output; only refresh rate changes.");
    } else if same_refresh {
        builder = builder
            .blank()
            .subtext("Refresh rate matches current output; only resolution/aspect changes.");
    }

    builder.build()
}

fn render_mode_comparison_ascii(candidate: &DisplayMode, current: &DisplayMode) -> Vec<String> {
    const CANVAS_W: usize = 28;
    const CANVAS_H: usize = 10;

    let max_w = candidate.width.max(current.width).max(1) as f64;
    let max_h = candidate.height.max(current.height).max(1) as f64;
    let scale = f64::min(CANVAS_W as f64 / max_w, CANVAS_H as f64 / max_h);

    let candidate_w = ((candidate.width as f64 * scale).round() as usize).clamp(1, CANVAS_W);
    let candidate_h = ((candidate.height as f64 * scale).round() as usize).clamp(1, CANVAS_H);
    let current_w = ((current.width as f64 * scale).round() as usize).clamp(1, CANVAS_W);
    let current_h = ((current.height as f64 * scale).round() as usize).clamp(1, CANVAS_H);

    let mut canvas = vec![vec![' '; CANVAS_W]; CANVAS_H];
    draw_centered_box(&mut canvas, current_w, current_h, '.');
    draw_centered_box(&mut canvas, candidate_w, candidate_h, '#');

    let mut lines = Vec::with_capacity(CANVAS_H + 2);
    lines.push(format!(
        "  {:<14} {}",
        "candidate",
        candidate.display_format()
    ));
    lines.push(format!("  {:<14} {}", "current", current.display_format()));
    lines.push(String::new());
    for row in canvas {
        let mut line: String = row.into_iter().collect();
        while line.ends_with(' ') {
            line.pop();
        }
        lines.push(format!("  {}", line));
    }
    lines
}

fn draw_centered_box(canvas: &mut [Vec<char>], width: usize, height: usize, ch: char) {
    let canvas_h = canvas.len();
    let canvas_w = canvas.first().map_or(0, Vec::len);
    if canvas_w == 0 || canvas_h == 0 {
        return;
    }

    let start_x = (canvas_w.saturating_sub(width)) / 2;
    let start_y = (canvas_h.saturating_sub(height)) / 2;
    let end_x = (start_x + width.saturating_sub(1)).min(canvas_w.saturating_sub(1));
    let end_y = (start_y + height.saturating_sub(1)).min(canvas_h.saturating_sub(1));

    for (y, row) in canvas.iter_mut().enumerate().take(end_y + 1).skip(start_y) {
        for (x, cell) in row.iter_mut().enumerate().take(end_x + 1).skip(start_x) {
            let on_border = x == start_x || x == end_x || y == start_y || y == end_y;
            if !on_border {
                continue;
            }

            *cell = match (*cell, ch) {
                (' ', next) => next,
                (existing, next) if existing == next => existing,
                _ => '*',
            };
        }
    }
}

fn aspect_ratio_label(width: u32, height: u32) -> String {
    let divisor = gcd(width, height).max(1);
    format!("{}:{}", width / divisor, height / divisor)
}

fn gcd(mut a: u32, mut b: u32) -> u32 {
    while b != 0 {
        let tmp = a % b;
        a = b;
        b = tmp;
    }
    a
}

fn megapixels(mode: &DisplayMode) -> f64 {
    (mode.width as f64 * mode.height as f64) / 1_000_000.0
}

fn compare_pixels(delta_pixels: i128) -> String {
    if delta_pixels == 0 {
        return "Same pixel count as current".to_string();
    }

    let delta_mp = (delta_pixels.unsigned_abs() as f64) / 1_000_000.0;
    if delta_pixels > 0 {
        format!("Larger canvas than current (+{delta_mp:.2} MP)")
    } else {
        format!("Smaller canvas than current (-{delta_mp:.2} MP)")
    }
}

fn compare_refresh(delta_refresh: i128) -> String {
    const NOISE_THRESHOLD_MILLIHZ: i128 = 10;

    if delta_refresh.abs() < NOISE_THRESHOLD_MILLIHZ {
        return "Same refresh rate as current".to_string();
    }

    let delta_hz = delta_refresh.unsigned_abs() as f64 / 1000.0;
    if delta_refresh > 0 {
        format!("Higher refresh than current (+{delta_hz:.2} Hz)")
    } else {
        format!("Lower refresh than current (-{delta_hz:.2} Hz)")
    }
}

impl Setting for ConfigureDisplay {
    fn metadata(&self) -> SettingMetadata {
        SettingMetadata::builder()
            .id("display.configure")
            .title("Display Configuration")
            .icon(NerdFont::Monitor)
            .summary("Configure display resolution and refresh rate.\n\nSelect a display and choose from available modes.\n\nSupported on X11 (all WMs), Sway, Hyprland, and InstantWM Wayland.")
            .build()
    }

    fn setting_type(&self) -> SettingType {
        SettingType::Action
    }

    fn apply(&self, ctx: &mut SettingsContext) -> Result<()> {
        let display_server = DisplayServer::detect();
        let compositor = CompositorType::detect();
        let is_sway = matches!(compositor, CompositorType::Sway);
        let is_hyprland = matches!(compositor, CompositorType::Hyprland);

        // Determine which provider to use:
        //   X11 (any WM)               → xrandr
        //   Wayland + Sway             → swaymsg
        //   Wayland + Hyprland         → hyprctl
        //   Wayland + instantWM        → instantwmctl
        //   Wayland + other/unknown    → unsupported
        let use_xrandr = display_server.is_x11();
        let use_sway = display_server.is_wayland() && is_sway;
        let use_hyprland = display_server.is_wayland() && is_hyprland;
        let use_instantwm =
            display_server.is_wayland() && matches!(compositor, CompositorType::InstantWM);

        if !use_xrandr && !use_sway && !use_hyprland && !use_instantwm {
            ctx.emit_unsupported(
                "settings.display.configure.unsupported",
                &format!(
                    "Display configuration requires X11, Sway, Hyprland, or InstantWM. Detected: {} on {}.",
                    compositor.name(),
                    display_server,
                ),
            );
            return Ok(());
        }

        // Query outputs from the appropriate provider
        let outputs = if use_sway {
            match SwayDisplayProvider::get_outputs_sync() {
                Ok(outputs) => outputs,
                Err(e) => {
                    ctx.emit_failure(
                        "settings.display.configure.query_failed",
                        &format!("Failed to query displays: {e}"),
                    );
                    return Ok(());
                }
            }
        } else if use_hyprland {
            match HyprlandDisplayProvider::get_outputs_sync() {
                Ok(outputs) => outputs,
                Err(e) => {
                    ctx.emit_failure(
                        "settings.display.configure.query_failed",
                        &format!("Failed to query displays: {e}"),
                    );
                    return Ok(());
                }
            }
        } else if use_instantwm {
            match InstantWMDisplayProvider::get_outputs_sync() {
                Ok(outputs) => outputs,
                Err(e) => {
                    ctx.emit_failure(
                        "settings.display.configure.query_failed",
                        &format!("Failed to query displays: {e}"),
                    );
                    return Ok(());
                }
            }
        } else {
            // X11 — xrandr
            match XrandrDisplayProvider::get_outputs_sync() {
                Ok(outputs) => outputs,
                Err(e) => {
                    ctx.emit_failure(
                        "settings.display.configure.query_failed",
                        &format!("Failed to query displays: {e}"),
                    );
                    return Ok(());
                }
            }
        };

        if outputs.is_empty() {
            ctx.emit_info(
                "settings.display.configure.no_displays",
                "No displays detected.",
            );
            return Ok(());
        }

        // If there's only one display, use it directly without prompting
        let output = if outputs.len() == 1 {
            outputs.first().unwrap()
        } else {
            // Build display selection menu
            let display_options: Vec<String> = outputs.iter().map(|o| o.display_label()).collect();

            let selected_display = FzfWrapper::builder()
                .prompt("Select Display")
                .header("Choose a display to configure")
                .select(display_options.clone())?;

            match selected_display {
                crate::menu_utils::FzfResult::Selected(selection) => {
                    outputs.iter().find(|o| o.display_label() == selection)
                }
                _ => return Ok(()),
            }
            .ok_or_else(|| anyhow::anyhow!("No display selected"))?
        };

        // Build resolution/refresh rate menu
        // Sort modes: highest resolution first, then highest refresh rate
        let mut sorted_modes = output.available_modes.clone();
        sorted_modes.sort_by(|a, b| {
            b.resolution()
                .cmp(&a.resolution())
                .then(b.refresh.cmp(&a.refresh))
        });

        // Build menu options with current mode marked
        let mode_items: Vec<ModeMenuItem> = sorted_modes
            .iter()
            .cloned()
            .map(|mode| ModeMenuItem {
                output_name: output.name.clone(),
                mode,
                current_mode: output.current_mode.clone(),
            })
            .collect();

        let selected_mode = FzfWrapper::builder()
            .prompt("Select Mode")
            .header(format!("Choose resolution/refresh for {}", output.name))
            .select(mode_items)?;

        let target_mode = match selected_mode {
            crate::menu_utils::FzfResult::Selected(selection) => Some(selection.mode),
            _ => return Ok(()),
        };

        let mode = match target_mode {
            Some(m) => m,
            None => return Ok(()),
        };

        // Apply the mode via the appropriate provider
        let result = if use_sway {
            SwayDisplayProvider::set_output_mode_sync(&output.name, &mode)
        } else if use_hyprland {
            HyprlandDisplayProvider::set_output_mode_sync(&output.name, &mode)
        } else if use_instantwm {
            InstantWMDisplayProvider::set_output_mode_sync(&output.name, &mode)
        } else {
            XrandrDisplayProvider::set_output_mode_sync(&output.name, &mode)
        };

        if let Err(e) = result {
            ctx.emit_failure(
                "settings.display.configure.apply_failed",
                &format!("Failed to apply mode: {e}"),
            );
            return Ok(());
        }

        ctx.notify(
            "Display",
            &format!("{} set to {}", output.name, mode.display_format()),
        );
        Ok(())
    }
}
