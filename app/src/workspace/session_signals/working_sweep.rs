//! The thin line that glides along a tab's bottom edge while its agent is working.

use std::f32::consts::PI;
use std::sync::LazyLock;
use std::time::Duration;

use instant::Instant;
use pathfinder_color::ColorU;
use warpui::elements::{CornerRadius, Fill, Point, Radius};
use warpui::event::DispatchedEvent;
use warpui::geometry::rect::RectF;
use warpui::geometry::vector::{Vector2F, vec2f};
use warpui::{
    AfterLayoutContext, AppContext, Element, EventContext, LayoutContext, PaintContext,
    SizeConstraint,
};

const SWEEP_PERIOD: Duration = Duration::from_millis(2600);
/// ~25fps: smooth enough to read as motion without redrawing on every frame.
const SWEEP_FRAME_INTERVAL: Duration = Duration::from_millis(40);
/// Length of the line as a fraction of the tab's width.
const BAND_WIDTH_RATIO: f32 = 0.35;
const LINE_THICKNESS: f32 = 1.5;

/// Shared start time, so every working tab's line glides in step.
static SWEEP_ANCHOR: LazyLock<Instant> = LazyLock::new(Instant::now);

/// Paints a short line in `color`, fading at both ends, gliding left to right along the bottom
/// edge of `child`, and keeps the window repainting while it is shown.
pub struct WorkingSweep {
    child: Box<dyn Element>,
    color: ColorU,
    corner_radius: CornerRadius,
    origin: Option<Point>,
}

impl WorkingSweep {
    /// `corner_radius` should match the child's so the line never shows past its rounded corners.
    pub fn new(child: Box<dyn Element>, color: ColorU, corner_radius: CornerRadius) -> Self {
        Self {
            child,
            color,
            corner_radius,
            origin: None,
        }
    }

    /// Where the line's center is, as a fraction of the width, `elapsed` into the animation. It
    /// starts and ends fully outside the tab so each pass enters and leaves cleanly.
    fn band_center(elapsed: Duration) -> f32 {
        let period = SWEEP_PERIOD.as_secs_f32();
        let t = (elapsed.as_secs_f32() % period) / period;
        let eased = (1. - (PI * t).cos()) / 2.;
        let half_band = BAND_WIDTH_RATIO / 2.;
        -half_band + eased * (1. + 2. * half_band)
    }

    /// Draws the part of the gradient running from `from` to `to` (in pixels from the left edge)
    /// that falls between `min_x` and `max_x`.
    fn paint_ramp(
        origin: Vector2F,
        size: Vector2F,
        (min_x, max_x): (f32, f32),
        (from, to): (f32, f32),
        (from_color, to_color): (ColorU, ColorU),
        ctx: &mut PaintContext,
    ) {
        let left = from.max(min_x);
        let right = to.min(max_x);
        if right - left < 0.5 {
            return;
        }
        // The renderer extrapolates gradients linearly past their end points, so a ramp cut off by
        // the tab's edge keeps its colors by placing the end points outside the drawn rect.
        let visible = right - left;
        let start = vec2f((from - left) / visible, 0.);
        let end = vec2f((to - left) / visible, 0.);

        ctx.scene
            .draw_rect_without_hit_recording(RectF::new(
                origin + vec2f(left, size.y() - LINE_THICKNESS),
                vec2f(visible, LINE_THICKNESS),
            ))
            .with_background(Fill::Gradient {
                start,
                end,
                start_color: from_color,
                end_color: to_color,
            });
    }
}

impl Element for WorkingSweep {
    fn layout(
        &mut self,
        constraint: SizeConstraint,
        ctx: &mut LayoutContext,
        app: &AppContext,
    ) -> Vector2F {
        self.child.layout(constraint, ctx, app)
    }

    fn after_layout(&mut self, ctx: &mut AfterLayoutContext, app: &AppContext) {
        self.child.after_layout(ctx, app);
    }

    fn paint(&mut self, origin: Vector2F, ctx: &mut PaintContext, app: &AppContext) {
        self.child.paint(origin, ctx, app);
        self.origin = Some(Point::from_vec2f(origin, ctx.scene.z_index()));
        if let Some(size) = self.child.size() {
            let center = Self::band_center(SWEEP_ANCHOR.elapsed()) * size.x();
            let half_band = BAND_WIDTH_RATIO * size.x() / 2.;
            // The line hugs the bottom edge, so it stays out of the rounded corners entirely
            // rather than poking past their curve.
            let bounds = (
                radius_in_pixels(self.corner_radius.get_bottom_left(), size),
                size.x() - radius_in_pixels(self.corner_radius.get_bottom_right(), size),
            );
            let clear = ColorU { a: 0, ..self.color };
            Self::paint_ramp(
                origin,
                size,
                bounds,
                (center - half_band, center),
                (clear, self.color),
                ctx,
            );
            Self::paint_ramp(
                origin,
                size,
                bounds,
                (center, center + half_band),
                (self.color, clear),
                ctx,
            );
        }
        ctx.repaint_after(SWEEP_FRAME_INTERVAL);
    }

    fn size(&self) -> Option<Vector2F> {
        self.child.size()
    }

    fn origin(&self) -> Option<Point> {
        self.origin
    }

    fn dispatch_event(
        &mut self,
        event: &DispatchedEvent,
        ctx: &mut EventContext,
        app: &AppContext,
    ) -> bool {
        self.child.dispatch_event(event, ctx, app)
    }
}

fn radius_in_pixels(radius: Radius, size: Vector2F) -> f32 {
    match radius {
        Radius::Pixels(pixels) => pixels,
        Radius::Percentage(percentage) => size.x().min(size.y()) * percentage / 100.,
    }
}

#[cfg(test)]
#[path = "working_sweep_tests.rs"]
mod tests;
