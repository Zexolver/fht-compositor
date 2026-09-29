//! Moving/swapping windows with two fingers on a touchscreen.
//!
//! This mirrors what [`SwapTileGrab`](super::swap_tile_grab::SwapTileGrab) does for the pointer:
//! dragging a window moves it around if floating, or swaps it with whatever tiled window ends up
//! under it. There is no dedicated hit-target (no titlebar) to grab, so the trigger is instead
//! touching the *same* window with a second finger.
//!
//! This is **not** a [`smithay::input::touch::TouchGrab`]: a touch grab takes over every touch
//! point for the whole seat, which fights with the sticky "every touch point goes to the surface
//! the first one landed on" behaviour Smithay's own default touch grab already implements (see
//! `TouchDownGrab` in Smithay). Instead, we track touch points ourselves in [`State`] and only take
//! over once exactly two of them land on the same window, at which point we [`cancel`
//! ](smithay::input::touch::TouchHandle::cancel) the touch sequence (so the client forgets about
//! it) and drive [`Space`](crate::space::Space)'s interactive swap directly, ourselves.
//!
//! Known limitation: if a third finger is already down when the two-finger drag starts, its touch
//! sequence gets cancelled too (since `cancel` applies to the whole seat), and we keep "tracking"
//! it as far as not sending it a spurious `up` afterwards, but nothing more.

use smithay::backend::input::TouchSlot;
use smithay::utils::{Logical, Point};

use crate::state::State;
use crate::window::Window;

/// A touch point that isn't (yet) part of a two-finger window drag. See [`State::touch_swap_down`].
#[derive(Debug, Clone)]
pub struct TouchPoint {
    /// The window the touch point landed on, if any.
    pub window: Option<Window>,
    /// The touch point's last known location, in the global compositor space.
    pub location: Point<f64, Logical>,
}

/// An in-progress two-finger drag moving/swapping a window. See [`State::touch_swap_down`].
#[derive(Debug)]
pub struct TouchSwap {
    pub window: Window,
    slots: (TouchSlot, TouchSlot),
    locations: (Point<f64, Logical>, Point<f64, Logical>),
}

fn midpoint(a: Point<f64, Logical>, b: Point<f64, Logical>) -> Point<f64, Logical> {
    Point::from(((a.x + b.x) / 2.0, (a.y + b.y) / 2.0))
}

impl State {
    /// Record a new touch point landing at `location`, on `window` if any.
    ///
    /// If it lands on the same window as the only other touch point currently down, this starts a
    /// two-finger drag instead, and returns `true`: the caller should [`cancel`
    /// ](smithay::input::touch::TouchHandle::cancel) the touch sequence instead of forwarding this
    /// touch point to the client.
    pub(super) fn touch_swap_down(
        &mut self,
        slot: TouchSlot,
        window: Option<Window>,
        location: Point<f64, Logical>,
    ) -> bool {
        let existing = (self.fht.touch_swap.is_none() && self.fht.touch_points.len() == 1)
            .then(|| {
                self.fht
                    .touch_points
                    .iter()
                    .next()
                    .map(|(&s, p)| (s, p.clone()))
            })
            .flatten();

        if let (Some(window), Some((other_slot, other))) = (window.clone(), existing) {
            if other.window.as_ref() == Some(&window) {
                self.fht.touch_points.remove(&other_slot);

                let midpoint = midpoint(location, other.location);
                if self
                    .fht
                    .space
                    .start_interactive_swap(&window, midpoint.to_i32_round(), true)
                {
                    self.fht.touch_swap = Some(TouchSwap {
                        window,
                        slots: (other_slot, slot),
                        locations: (other.location, location),
                    });
                    return true;
                }

                // Could not start the swap (a keyboard/mouse swap might already be in progress),
                // keep tracking the other touch point normally.
                self.fht.touch_points.insert(other_slot, other);
            }
        }

        self.fht
            .touch_points
            .insert(slot, TouchPoint { window, location });
        false
    }

    /// Update a touch point's location at `slot`, moving/swapping the window if this touch point
    /// is part of an in-progress two-finger drag.
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

        if slot == swap.slots.0 {
            swap.locations.0 = location;
        } else if slot == swap.slots.1 {
            swap.locations.1 = location;
        } else {
            // An unrelated finger moved while we are dragging a window with two others.
            return false;
        }

        let midpoint = midpoint(swap.locations.0, swap.locations.1);
        let window = swap.window.clone();
        // `center_window` was `true` when the swap started, so the "initial" location argument
        // here is unused, see `Space::handle_interactive_swap_motion`.
        self.fht.space.handle_interactive_swap_motion(
            &window,
            midpoint.to_i32_round(),
            midpoint.to_i32_round(),
        );
        true
    }

    /// Forget about a touch point at `slot`, ending the two-finger drag if it was part of one.
    ///
    /// Returns `true` if it was: the caller should not forward this touch point to the client (its
    /// sequence was already [`cancel`](smithay::input::touch::TouchHandle::cancel)led when the drag
    /// started).
    pub(super) fn touch_swap_up(&mut self, slot: TouchSlot) -> bool {
        let is_swap_slot = self
            .fht
            .touch_swap
            .as_ref()
            .is_some_and(|swap| slot == swap.slots.0 || slot == swap.slots.1);

        if !is_swap_slot {
            self.fht.touch_points.remove(&slot);
            return false;
        }

        let swap = self.fht.touch_swap.take().unwrap();
        let midpoint = midpoint(swap.locations.0, swap.locations.1);
        self.fht
            .space
            .handle_interactive_swap_end(&swap.window, midpoint);
        true
    }

    /// End the whole touch session: any in-progress two-finger drag, and every tracked touch
    /// point. Called when the backend cancels the entire touch sequence (see
    /// [`State::on_touch_cancel`]).
    pub(super) fn touch_swap_cancel(&mut self) {
        self.fht.touch_points.clear();
        let Some(swap) = self.fht.touch_swap.take() else {
            return;
        };

        let midpoint = midpoint(swap.locations.0, swap.locations.1);
        self.fht
            .space
            .handle_interactive_swap_end(&swap.window, midpoint);
    }
}
