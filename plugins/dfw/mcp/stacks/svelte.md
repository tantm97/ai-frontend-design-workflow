# Stack guide: Svelte / SvelteKit

- **Tokens:** generate CSS custom properties into `src/app.css` (imported in the root `+layout.svelte`).
- **Components:** one `.svelte` per design component; props for variants/states; snippets/slots for composition. Styles use `var(--token)`.
- **SvelteKit:** routes per handoff; load functions for data; form actions with progressive enhancement and accessible error messages.
- **Motion:** prefer CSS; Svelte transitions only where the design specifies motion, respecting reduced motion.
