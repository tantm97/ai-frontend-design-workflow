# Stack guide: Angular

- **Tokens:** generate CSS custom properties into a global stylesheet listed in `angular.json` `styles`. With Angular Material (only if approved), map its theme (M3 system variables) to the token variables instead of duplicating values.
- **Components:** standalone components per design component; `@Input()` / signals for variants and states; content projection for slots. Component styles use `var(--token)` only; keep `ViewEncapsulation` default unless the project differs.
- **Semantics/forms:** reactive forms with labels and `aria-describedby` errors; CDK a11y utilities for focus management in dialogs/menus.
- **Routing:** lazy routes per feature as the project does.
