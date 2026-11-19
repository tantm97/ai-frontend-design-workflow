# Stack guide: React / Next.js

Applies only after G3 approved React (or Next.js, Remix, Vite + React).

- **Tokens:** generate CSS custom properties from `design/approved/vN/tokens/tokens.json` (Style Dictionary `css/variables`) into a global stylesheet imported once (Next: `app/layout.tsx` / `pages/_app.tsx`). With Tailwind (only if approved), map the theme to the CSS variables (`colors: { surface: 'var(--color-surface)' }`) instead of copying values. With CSS-in-JS, expose a theme object generated from the same tokens.
- **Components:** one component per design component; props mirror design variants/states (`variant`, `size`, `state`). Keep `data-dfw-id` as `data-dfw-id` props in development only if the user wants the live-app review overlay; otherwise drop them.
- **Semantics:** keep the design's landmarks and heading order; use `<button>` for actions, `<a>`/`Link` for navigation; forms with `<label htmlFor>`, `aria-describedby` for errors.
- **States:** loading/empty/error states from the handoff are real components, not afterthoughts (Suspense boundaries / query states).
- **Next.js:** respect the router in use (app vs pages); server vs client components: interactive parts are client components, keep the rest server-rendered.
- **Motion:** CSS transitions first; a motion library only if approved at G3.
- **Composition:** compound components and explicit variant components instead of boolean props; React 19: no `forwardRef` (ref is a prop).
- **Performance:** after G3 only, and only if the user wants it, Vercel's `react-best-practices` skill (vercel-labs/agent-skills) is a good rule list for waterfalls, bundle size and re-renders. Its repository has no LICENSE file (MIT is stated only in the skill's frontmatter), so dfw does not vendor it; the user installs it themselves.
- **Verification route list:** fill handoff section 1 with actual routes.
