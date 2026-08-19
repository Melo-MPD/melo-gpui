//! A horizontal slider implemented as a custom GPUI element. Drag tracking
//! uses window-level mouse listeners so the thumb keeps following the pointer
//! after it leaves the track (something div-level `on_mouse_move` can't do).

use gpui::{
    App, Bounds, CursorStyle, DispatchPhase, Element, ElementId, GlobalElementId, Hitbox,
    HitboxBehavior, Hsla, IntoElement, LayoutId, MouseButton, MouseDownEvent, MouseMoveEvent,
    MouseUpEvent, Pixels, Style, Window, fill, point, px, relative, size,
};
use std::cell::RefCell;
use std::rc::Rc;

type Callback = Rc<dyn Fn(f32, &mut Window, &mut App)>;

#[derive(Default)]
struct DragState {
    dragging: bool,
    fraction: f32,
}

pub struct Slider {
    id: ElementId,
    value: f32,
    width: Option<Pixels>,
    track_color: Hsla,
    fill_color: Hsla,
    thumb_color: Hsla,
    thumb_size: Pixels,
    track_height: Pixels,
    on_change: Option<Callback>,
    on_release: Option<Callback>,
}

pub fn slider(id: impl Into<ElementId>, value: f32) -> Slider {
    Slider {
        id: id.into(),
        value: value.clamp(0., 1.),
        width: None,
        track_color: gpui::opaque_grey(0.5, 0.3),
        fill_color: gpui::blue(),
        thumb_color: gpui::white(),
        thumb_size: px(14.),
        track_height: px(4.),
        on_change: None,
        on_release: None,
    }
}

impl Slider {
    pub fn w(mut self, width: Pixels) -> Self {
        self.width = Some(width);
        self
    }
    pub fn colors(mut self, track: Hsla, fill: Hsla, thumb: Hsla) -> Self {
        self.track_color = track;
        self.fill_color = fill;
        self.thumb_color = thumb;
        self
    }
    pub fn thumb_size(mut self, size: Pixels) -> Self {
        self.thumb_size = size;
        self
    }
    /// Fired on click and on every drag tick with the new fraction (0..=1).
    pub fn on_change(mut self, f: impl Fn(f32, &mut Window, &mut App) + 'static) -> Self {
        self.on_change = Some(Rc::new(f));
        self
    }
    /// Fired once when the mouse button is released after a drag/click.
    pub fn on_release(mut self, f: impl Fn(f32, &mut Window, &mut App) + 'static) -> Self {
        self.on_release = Some(Rc::new(f));
        self
    }
}

impl IntoElement for Slider {
    type Element = Self;
    fn into_element(self) -> Self::Element {
        self
    }
}

fn fraction_for(bounds: &Bounds<Pixels>, x: Pixels, thumb: Pixels) -> f32 {
    let inset = thumb / 2.;
    let left = bounds.left() + inset;
    let width = bounds.size.width - thumb;
    if width <= px(0.) {
        return 0.;
    }
    ((x - left) / width).clamp(0., 1.)
}

impl Element for Slider {
    type RequestLayoutState = ();
    type PrepaintState = Hitbox;

    fn id(&self) -> Option<ElementId> {
        Some(self.id.clone())
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&gpui::InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        let mut style = Style::default();
        style.size.width = match self.width {
            Some(w) => w.into(),
            None => relative(1.).into(),
        };
        style.size.height = (self.thumb_size + px(6.)).into();
        style.flex_shrink = 0.;
        (window.request_layout(style, [], cx), ())
    }

    fn prepaint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&gpui::InspectorElementId>,
        bounds: Bounds<Pixels>,
        _request_layout: &mut Self::RequestLayoutState,
        window: &mut Window,
        _cx: &mut App,
    ) -> Self::PrepaintState {
        window.insert_hitbox(bounds, HitboxBehavior::Normal)
    }

    fn paint(
        &mut self,
        id: Option<&GlobalElementId>,
        _inspector_id: Option<&gpui::InspectorElementId>,
        bounds: Bounds<Pixels>,
        _request_layout: &mut Self::RequestLayoutState,
        hitbox: &mut Self::PrepaintState,
        window: &mut Window,
        _cx: &mut App,
    ) {
        let id = id.expect("slider always has an id");
        let state: Rc<RefCell<DragState>> = window.with_element_state(id, |prev, _| {
            let s: Rc<RefCell<DragState>> = prev.unwrap_or_default();
            (s.clone(), s)
        });

        let dragging = state.borrow().dragging;
        let fraction = if dragging {
            state.borrow().fraction
        } else {
            self.value
        };

        // Geometry
        let thumb = self.thumb_size;
        let track_h = self.track_height;
        let center_y = bounds.top() + bounds.size.height / 2.;
        let track_left = bounds.left() + thumb / 2.;
        let track_width = bounds.size.width - thumb;
        let track_bounds = Bounds::new(
            point(track_left, center_y - track_h / 2.),
            size(track_width, track_h),
        );
        let fill_bounds = Bounds::new(
            point(track_left, center_y - track_h / 2.),
            size(track_width * fraction, track_h),
        );
        let thumb_x = track_left + track_width * fraction - thumb / 2.;
        let thumb_bounds = Bounds::new(point(thumb_x, center_y - thumb / 2.), size(thumb, thumb));

        window.paint_quad(fill(track_bounds, self.track_color).corner_radii(track_h / 2.));
        window.paint_quad(fill(fill_bounds, self.fill_color).corner_radii(track_h / 2.));
        window.paint_shadows(
            thumb_bounds,
            (thumb / 2.).into(),
            &[gpui::BoxShadow {
                color: gpui::black().opacity(0.25),
                offset: point(px(0.), px(1.)),
                blur_radius: px(3.),
                spread_radius: px(0.),
            }],
        );
        window.paint_quad(
            fill(thumb_bounds, self.thumb_color)
                .corner_radii(thumb / 2.)
                .border_widths(px(0.5))
                .border_color(gpui::black().opacity(0.15)),
        );

        window.set_cursor_style(CursorStyle::PointingHand, hitbox);

        // Mouse handling (window-level so drags survive leaving the track).
        let on_change = self.on_change.clone();
        let on_release = self.on_release.clone();
        let hitbox_id = hitbox.id;

        {
            let state = state.clone();
            let on_change = on_change.clone();
            window.on_mouse_event(move |event: &MouseDownEvent, phase, window, cx| {
                if phase != DispatchPhase::Bubble
                    || event.button != MouseButton::Left
                    || !hitbox_id.is_hovered(window)
                {
                    return;
                }
                let f = fraction_for(&bounds, event.position.x, thumb);
                {
                    let mut s = state.borrow_mut();
                    s.dragging = true;
                    s.fraction = f;
                }
                if let Some(cb) = &on_change {
                    cb(f, window, cx);
                }
                cx.stop_propagation();
                window.refresh();
            });
        }
        {
            let state = state.clone();
            window.on_mouse_event(move |event: &MouseMoveEvent, phase, window, cx| {
                if phase != DispatchPhase::Bubble || !state.borrow().dragging {
                    return;
                }
                if event.pressed_button != Some(MouseButton::Left) {
                    state.borrow_mut().dragging = false;
                    window.refresh();
                    return;
                }
                let f = fraction_for(&bounds, event.position.x, thumb);
                state.borrow_mut().fraction = f;
                if let Some(cb) = &on_change {
                    cb(f, window, cx);
                }
                window.refresh();
            });
        }
        {
            window.on_mouse_event(move |event: &MouseUpEvent, phase, window, cx| {
                if phase != DispatchPhase::Bubble
                    || event.button != MouseButton::Left
                    || !state.borrow().dragging
                {
                    return;
                }
                let f = fraction_for(&bounds, event.position.x, thumb);
                {
                    let mut s = state.borrow_mut();
                    s.dragging = false;
                    s.fraction = f;
                }
                if let Some(cb) = &on_release {
                    cb(f, window, cx);
                }
                window.refresh();
            });
        }
    }
}
