//! Moving/swapping windows with a configured number of fingers on a touchscreen, see
//! [`fht_compositor_config::Config::touchbinds`].
//!
//! This mirrors what [`SwapTileGrab`](super::swap_tile_grab::SwapTileGrab) does for the pointer:
//! dragging a window moves it around if floating, or swaps it with whatever tiled window ends up
//! under it. There is no dedicated hit-target (no titlebar) to grab, so the trigger is instead
//! touching the *same* window with the configured number of fingers.
//!
//! This is **not** a [`smithay::input::touch::TouchGrab`]: a touch grab takes over every touch
//! point for the whole seat, which fights with the sticky "every touch point goes to the surface
//! the first one landed on" behaviour Smithay's own default touch grab already implements (see
//! `TouchDownGrab` in Smithay). Instead, we track touch points ourselves in [`State`] and only take
//! over once enough of them land on the same window, at which point we [`cancel`
//! ](smithay::input::touch::TouchHandle::cancel) the touch sequence (so the client forgets about
//! it) and drive [`Space`](crate::space::Space)'s interactive swap directly, ourselves.
//!
//! Known limitations:
//! - If a lower finger count and a higher one are both bound (say `2` and `4`), the lower one
//!   always triggers first, since it is reached first. The higher binding can then never trigger
//!   from the same continuous touch.
//! - If extra fingers are already down when the bound count is reached, their touch sequence gets
//!   cancelled too (since `cancel` applies to the whole seat), and we keep "tracking" them as far
//!   as not sending them a spurious `up` afterwards, but nothing more.

use std::collections::HashMap;
use std::num::NonZero;

use fht_compositor_config::TouchAction;
use smithay::backend::input::TouchSlot;
use smithay::utils::{Logical, Point};

use crate::state::State;
use crate::window::Window;

/// A touch point that isn't (yet) part of a bound multi-finger window drag. See
/// [`State::touch_swap_down`].
#[derive(Debug, Clone)]
pub struct TouchPoint {
    /// The window the touch point landed on, if any.
    pub window: Option<Window>,
    /// The touch point's last known location, in the global compositor space.
    pub location: Point<f64, Logical>,
}

/// An in-progress multi-finger drag moving/swapping a window. See [`State::touch_swap_down`].
#[derive(Debug)]
pub struct TouchSwap {
    pub window: Window,
    action: TouchAction,
    /// The touch points driving this drag, and their last known location.
    points: HashMap<TouchSlot, Point<f64, Logical>>,
}

/// The average of every point, used as the "cursor" position driving the drag.
fn average(points: impl ExactSizeIterator<Item = Point<f64, Logical>>) -> Point<f64, Logical> {
    let count = points.len() as f64;
    let sum = points.fold(Point::from((0.0, 0.0)), |acc, point| acc + point);
    (sum.x / count, sum.y / count).into()
}

impl State {
    /// Record a new touch point landing at `location`, on `window` if any.
    ///
    /// If it brings the number of touch points on `window` up to a number of fingers bound in
    /// [`fht_compositor_config::Config::touchbinds`], this starts that drag instead, and returns
    /// `true`: the caller should [`cancel`](smithay::input::touch::TouchHandle::cancel) the touch
    /// sequence instead of forwarding this touch point to the client.
    pub(super) fn touch_swap_down(
        &mut self,
        slot: TouchSlot,
        window: Option<Window>,
        location: Point<f64, Logical>,
    ) -> bool {
        if self.fht.touch_swap.is_none() {
            if let Some(window) = window.clone() {
                let matching: Vec<(TouchSlot, Point<f64, Logical>)> = self
                    .fht
                    .touch_points
                    .iter()
                    .filter(|(_, point)| point.window.as_ref() == Some(&window))
                    .map(|(&slot, point)| (slot, point.location))
                    .collect();

                // Safe: `matching.len()` is always >= 0, so this is always >= 1.
                let finger_count = NonZero::new(matching.len() as u32 + 1).unwrap();
                if let Some(&action) = self.fht.config.touchbinds.get(&finger_count) {
                    for (other_slot, _) in &matching {
                        self.fht.touch_points.remove(other_slot);
                    }

                    let mut points: HashMap<_, _> = matching.iter().copied().collect();
                    points.insert(slot, location);

                    let start_location = average(points.values().copied());
                    let started = match action {
                        TouchAction::SwapTile => self.fht.space.start_interactive_swap(
                            &window,
                            start_location.to_i32_round(),
                            true,
                        ),
                    };

                    if started {
                        self.fht.touch_swap = Some(TouchSwap {
                            window,
                            action,
                            points,
                        });
                        return true;
                    }

                    // Could not start (a keyboard/mouse swap might already be in progress), keep
                    // tracking the other touch points normally.
                    for (other_slot, other_location) in matching {
                        self.fht.touch_points.insert(
                            other_slot,
                            TouchPoint {
                                window: Some(window.clone()),
                                location: other_location,
                            },
                        );
                    }
                }
            }
        }

        self.fht
            .touch_points
            .insert(slot, TouchPoint { window, location });
        false
    }

    /// Update a touch point's location at `slot`, moving/swapping the window if this touch point
    /// is part of an in-progress drag.
    ///
    /// Returns `true` if it is: the caller should not forward this touch point to the client.
    pub(super) fn touch_swap_motion(
        &mut self,
        slot: TouchSlot,
        location: Point<f64, Logical>,
    ) -> bool {
        let Some(swap) = self.fht.touch_swap.as_mut() else {
            if let Some(point) = self.fht.touch_points.get_mut(&slot) {
                point.location = location;
            }
            return false;
        };

        let Some(point) = swap.points.get_mut(&slot) else {
            // An unrelated finger moved while we are dragging a window with the others.
            return false;
        };
        *point = location;

        let midpoint = average(swap.points.values().copied());
        let window = swap.window.clone();
        match swap.action {
            TouchAction::SwapTile => {
                // `center_window` was `true` when the drag started, so the "initial" location
                // argument here is unused, see `Space::handle_interactive_swap_motion`.
                self.fht.space.handle_interactive_swap_motion(
                    &window,
                    midpoint.to_i32_round(),
                    midpoint.to_i32_round(),
                );
            }
        }
        true
    }

    /// Forget about a touch point at `slot`, ending its drag if it was part of one.
    ///
    /// Returns `true` if it was: the caller should not forward this touch point to the client (its
    /// sequence was already [`cancel`](smithay::input::touch::TouchHandle::cancel)led when the drag
    /// started).
    pub(super) fn touch_swap_up(&mut self, slot: TouchSlot) -> bool {
        let is_swap_slot = self
            .fht
            .touch_swap
            .as_ref()
            .is_some_and(|swap| swap.points.contains_key(&slot));

        if !is_swap_slot {
            self.fht.touch_points.remove(&slot);
            return false;
        }

        // Lifting any one of the fingers driving the drag ends it.
        let swap = self.fht.touch_swap.take().unwrap();
        self.end_touch_swap(swap);
        true
    }

    /// End the whole touch session: any in-progress drag, and every tracked touch point. Called
    /// when the backend cancels the entire touch sequence (see [`State::on_touch_cancel`]).
    pub(super) fn touch_swap_cancel(&mut self) {
        self.fht.touch_points.clear();
        if let Some(swap) = self.fht.touch_swap.take() {
            self.end_touch_swap(swap);
        }
    }

    fn end_touch_swap(&mut self, swap: TouchSwap) {
        let midpoint = average(swap.points.values().copied());
        match swap.action {
            TouchAction::SwapTile => {
                self.fht
                    .space
                    .handle_interactive_swap_end(&swap.window, midpoint);
            }
        }
    }
}
