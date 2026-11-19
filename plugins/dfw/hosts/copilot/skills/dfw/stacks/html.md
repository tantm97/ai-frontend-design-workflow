# Stack guide: plain HTML / CSS / JavaScript

- The approved screens are close to production markup: reuse their semantics and `data-dfw-id`-free copies of the markup.
- **Tokens:** ship `tokens.css` generated from the approved DTCG file; one stylesheet per component or a layered `@layer` structure (`tokens, base, components, pages`).
- **JavaScript:** progressive enhancement; no framework unless approved at G3. Use native elements (`<dialog>`, `<details>`, form validation) before custom widgets.
- **Build:** none unless the project already has one (Vite, Eleventy, Astro); ask before adding.
