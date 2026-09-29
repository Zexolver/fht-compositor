# Touch bindings

Touch bindings let you bind actions to touching a window with a given number of fingers. Unlike
[mousebindings](/configuration/mousebindings), there is no modifier or button to combine this with, the number of fingers
is the only thing that triggers the action. And unlike [gesturebindings](/configuration/gesturebindings), touching a window
is a continuous drag, not a one-shot discrete swipe, and it only affects the window it is done on, not the whole compositor.

There is no titlebar or other hit-target to grab on a touchscreen, so the trigger is purely "this many fingers landed on the
same window", anywhere on it.

By default, `touchbinds` is empty: nothing happens beyond what [input configuration](/configuration/input#touchscreens)
already covers (tap-to-focus, and forwarding touch to applications) until you bind something yourself.

## Touch patterns

`fingers = action`: touching a window with `fingers` fingers triggers `action`. Lifting any one of the fingers ends it.

example:

```toml
[touchbinds]
2 = "swap-tile"
```

> [!WARNING]
> Binding an action to `1` finger means every single touch on a window immediately triggers it, and no touch input ever
> reaches applications. This is rarely what you want.

> [!WARNING]
> If you bind more than one finger count (say `2` and `4`), the lower one always triggers first, since it is reached first
> in a continuous touch. The higher one can then never trigger from that same touch.

## Available touch actions

- `swap-tile`: Moves the window around (or swaps it with whatever tiled window it ends up on top of), the same as
  [`swap-tile`](/configuration/mousebindings#available-mouse-actions) does for the mouse.
