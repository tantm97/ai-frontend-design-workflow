# Stack guide: Vue / Nuxt

- **Tokens:** generate CSS custom properties (Style Dictionary) into `assets/tokens.css`, import once (`main.ts` or `nuxt.config` `css: []`). With an approved UI kit (Vuetify, PrimeVue, Naive UI), map its theme variables to the token variables.
- **Components:** SFCs per design component; variants/states as props; slots for composition (matching design slots). Scoped styles use `var(--token)` only.
- **Semantics/forms:** native elements first; `v-model` with labeled inputs; error text linked via `aria-describedby`.
- **Nuxt:** pages map to handoff routes; keep server/client rendering choices consistent with the project.
- **Verification:** routes listed in handoff section 1.
