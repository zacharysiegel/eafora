# Visual design

<!--
Status: draft, 2026-06-15. Cross-cutting visual identity for Eafora across all
platforms (web, iOS, Android). Per-platform implementation details (CSS
architecture, SwiftUI styling, Compose theming) inherit from this doc.

Companion artifacts in this directory:
  ./stub-desktop.html — desktop reference frame (map, ranking, scrubber)
  ./stub-mobile.html  — mobile reference frames (empty, peek, list, detail)
-->

## One-line summary

Sharp. White paper, red ink. Square corners, hairline borders, generous
whitespace, no shadows. Flat sheets of UI laid over a vector map. Editorial
and cartographic, not consumer-app.

## Reference stubs

Two static HTML stubs sit alongside this doc and render the language end-to-end. Open them in a browser to evaluate any proposed change against the established baseline; treat them as the visual ground truth that this prose describes.

- `./stub-desktop.html` — single-frame desktop layout: map with selected region (Japan), top-left detail panel, top-right ranking table, bottom time scrubber, full continent choropleth.
- `./stub-mobile.html` — four mobile frames at iPhone 14 Pro logical size: (00) empty map with choropleth legend and tap prompt; (01) selected region with full bottom sheet (region name, primary stat, metadata grid, source citation); (02) ranked list; (03) region detail with history chart and sources.

The stubs are static fixtures: no animations, no interactivity, no real data wiring. They demonstrate the visual language, not the app's behavior. When the language evolves, update both the prose below and the stubs in lockstep.

## The metaphor

A working desk: a printed map underneath, sheets of reference notes laid over
it. Every visual decision should reinforce that the user is consulting a
reference work, not browsing a feed.

## Color

- **Black** is the primary text color; nothing competes with it for legibility weight.
- **Red** is the single bright accent; it earns attention by being the only saturated color on screen. Use for: active selection, the cursor/playhead on a time scrubber, hover/focus on interactive controls, the highlighted region on the map, possibly the brand mark.
- **Blue** is secondary; reserved for signals that must read as distinct from "active". Likely links to source citations, secondary series in a multi-line chart, or non-destructive secondary actions.
- **White** is the substrate. No off-whites, no warm paper tints; clean white reads "high resolution."
- Greys exist only as hairline borders and disabled states. No mid-grey fills.
- No gradients. No alpha-tinted overlays. No translucent frosted glass.

## Geometry

- Square corners by default. The 0.5–1px radius option is for very small elements (inputs, chips) where pure 90° looks accidental at low resolution; the larger the element, the closer to zero the radius should be.
- Hairline borders: 1px black or near-black on white panels; 1px red on selected/focused elements. On retina displays, sub-pixel hairlines (`0.5px` via `transform: scale`) are acceptable for the "thin and high resolution" feel.
- No drop shadows on overlay panels. Separation comes from the border + the white fill, not from elevation. If a panel must read as "above" another, offset it spatially or stack with a subtle 1px outline rather than blur.

## Typography

- Sans-serif body: humanist or neo-grotesque with strong character distinction at small sizes. Candidates: Inter, IBM Plex Sans, Söhne, Untitled Sans, Helvetica Now.
- Sans-serif monospace for data tables and inline numerics: IBM Plex Mono, Berkeley Mono, JetBrains Mono. Most importantly: enable **tabular figures** (`font-feature-settings: "tnum"`) on every numeric display, even in the proportional body font, so columns align.
- Type scale skews tight; weights skew regular and medium. Bold is rare and earns its emphasis (data callouts, the active region's name).
- Generous line-height (approx. 1.5) for body prose; tight line-height (approx. 1.15) for table rows and chart labels.

## Layout

- Empty space is the most-used element. Panels should not extend to fill available width when their content doesn't need it. The map showing through at the edges is part of the composition.
- Panels feel like sheets: rectangular, opaque white, hairline-bordered, sitting on the map without shadow. Multiple panels can overlap; their stacking order is part of how the user reads them.
- A panel's edge IS its boundary. No internal padding hierarchy that creates "frames within frames."
- Information density is allowed. Sharp type at high contrast on white tolerates more density than typical consumer UI.

## Map

- Vector political boundaries, thin black strokes (matches the pen-on-paper metaphor exactly).
- Region fills use a single-hue scale (red intensity for the active statistic) over a white base. No multi-hue choropleths. The data itself is the only saturated color on the map.
- Selection: hairline red outline, not a fill change.
- Hover: a discrete state change to a hairline red outline. No scale-up in v1; if a scale effect is added in v2 it is a hard step (see "Animation").
- Choropleth legend: a small inline (no-border) caption + swatches + mono numerals at the bottom-left of the map. Visible on desktop permanently. On mobile, visible when no region is selected; hidden when a region is selected and the bottom sheet is up. The selected-state mobile sheet is the canonical place for the values themselves; the legend is the orientation cue for the empty state.

## Interaction

- Focus indicators: red 1px outline outside the element. No glow, no halo.
- Tooltips and popovers are themselves miniature panels: same hairline border, same flat fill.
- Loading states: a thin red progress bar or a static cursor-style indicator. Spinners are continuously animated and inherently soft; they violate the principle.

## Animation

Through v1, there are no animations at all. State changes are instant. This is consistent with the paper-and-ink metaphor: turning a page does not have an easing curve.

From v2 onward, animations may be introduced selectively, governed by a single principle: **crispness over smoothness**. A motion that snaps cleanly into place reads as sharp; a motion that glides reads as soft, which contradicts the visual identity. This biases the choice of *whether* to animate (the answer is usually no) and *how* to animate when the answer is yes:

- Prefer step transitions, very short fades (under 100ms), or hard cuts over interpolated movement.
- When position or size must animate, use short durations (under 150ms) and linear or near-linear easing. No spring physics, no overshoot, no elastic curves.
- An animation that would benefit from being slower to feel polished is the wrong animation. If it can't be crisp, don't animate it.

## Iconography

- Single-stroke geometric icons, 1–1.5px stroke, no fills. Black by default; red for active state. Match the pen-line vocabulary.

## Where Eafora can do better than enhancedradar.com

[enhancedradar.com](https://www.enhancedradar.com) is the closest existing reference for the language Eafora is reaching for: white-and-red, panels over a map. It is not the bar; it is the floor. That site uses (a) too much micro-chrome (rounded buttons, thick header bars), (b) inconsistent border weights, and (c) too many UI states at once. Eafora should:

- Use even less chrome. Most controls should be a label and a value; no button frames around inline toggles.
- Treat one element at a time as "active". The eye should always know where the focus is.
- Lean harder on type as the structural element. Section dividers can be a single line of bold-cap type with a hairline rule, not a header bar.
- Be willing to leave large regions of the screen empty when no data is selected. Restraint is the differentiator.

## How to apply

When implementing any UI element (web, iOS, Android), check the proposal against this vision before writing code:

- Does it use any color besides white / black / red / blue / hairline grey? Justify or remove.
- Are corners square (or ≤1px radius)? Are borders 1px (or sub-pixel hairline)?
- Does it use a drop shadow, blur, or translucent fill? Almost always: remove.
- Are numerics tabular-figured?
- Could the surrounding whitespace be larger? Default to yes.
- Is anything animated? In v1 the answer is no. In v2+, does the animation snap rather than glide?

Web styling is plain CSS; no Tailwind or utility-class libraries.

## Origin

This document is the extrapolation of the following author prompt (2026-06-15), preserved for provenance:

> i envision a generally white color scheme with relatively bright red accent, almost like white paper with red ink. maybe blue as a secondary accent color, and black. (pen colors)
>
> square corners (maaaaayybee .5-1px corner radius but even thats pushing it)
>
> one word for the style: "sharp"
>
> empty space should be used liberally
>
> design is flat and crisp, should feel thin and high resolution
>
> thin borders
>
> ui elements overlaying over the map background should have zero or very minimal shadow behind. should feel almost like arranging papers over each other
>
> This is not a great example but it's close to how i'm feeling. I think we can do much better: https://www.enhancedradar.com
>
> sans serif fonts
> generally non-monospace fonts, but i could imagine certain areas (e.g. data tables) using sans serif monospace
>
> all animations should prioritize crispness over smoothness. this will guide our choice of when or when not to animate as well as what types of animations to use. (no animations in <v2)
