# Design format: component masters and tokens

Part of the design workspace format (`references/design-format.md` has the layout and `design.json`; `references/design-format-screens.md` the screens). Masters and tokens are the design system: screens only compose them.

## Component masters
`design/system/components/button.html`:
```html
<dfw-component name="button" group="Actions" variants="primary secondary ghost danger" sizes="md sm lg"
  props="icon kbd" replaces="button" description="One primary per view; ...">
<style>
.btn { min-height: var(--size-control); padding: 0 var(--space-4); border-radius: var(--radius-md); ... }
.btn--primary { background: var(--color-accent); color: var(--color-on-accent); }
</style>
<template>
<button class="btn btn--{{variant}} btn--{{size}}" type="button"><dfw-icon data-dfw-if="icon" name="{{icon}}"></dfw-icon><span><dfw-slot>Button</dfw-slot></span><kbd data-dfw-if="kbd">{{kbd}}</kbd></button>
</template>
<dfw-examples>
<dfw-use c="button" variant="primary">Save changes</dfw-use>
<dfw-use c="button" variant="primary" disabled>Disabled</dfw-use>
</dfw-examples>
</dfw-component>
```
- `props="a b=default"` (or comma-separated when a default has spaces: `props="current, brand=Asset Tracking"`); `{{prop}}` (escaped), `{{variant}}`, `{{size}}`, `{{prop==value?text}}` (e.g. `aria-current` in the shared `app-shell`); `data-dfw-if="prop"` / `"!prop"` keeps an element only when the prop is set / unset.
- `<dfw-slot>` = instance content; `<dfw-slot name="actions">` is filled by `<dfw-fill slot="actions">` in the instance.
- `replaces="button input"`: the lint warns when a screen uses those raw elements instead of the component.
- Styles: tokens only (hard-coded colors are MUST errors in masters; raw lengths are SHOULD warnings). Cover every state in CSS: hover, focus-visible, active, disabled, loading (`aria-busy`), invalid/selected where relevant.
- `<dfw-examples>` must show every variant, size and state; the Design System page renders them.

## Tokens (DTCG 2025.10)
- Colors: `{"$type":"color","$value":{"colorSpace":"srgb","components":[r,g,b],"hex":"#rrggbb"}}`.
- Dimensions: `{"$type":"dimension","$value":{"value":16,"unit":"px"}}`; durations `{value, unit:"ms"}`.
- Aliases: `"$value": "{color.blue.600}"`; group `$type` is inherited.
- The token contract (names component masters and presets rely on): `color.{canvas,surface,surface-sunken,surface-hover,ink,muted,subtle,border,border-strong,rule,accent,accent-hover,accent-soft,accent-text,on-accent,on-fill,on-warn,focus,focus-ring}`, `color.status.{ok,info,warn,err}{,-tint,-text}`, `font.{family,size,weight,leading,tracking}.*`, `space.{half,1,1-5,2,3,4,5,6,8,10,12,16}`, `radius.{sm,md,lg,xl,pill}`, `border.{width,width-strong}`, `focus.{width,offset}`, `size.{control,control-sm,control-lg,target-wcag,target-touch,icon*,sidebar,rail,rail-item,dialog*,band,avatar-xl}`, `shadow.{sm,md,lg}`, `duration.{fast,base}`, `easing.out`. Keep these names when adapting a preset; add new ones freely.
