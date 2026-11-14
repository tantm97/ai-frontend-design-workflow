// Detect the technology stack of an existing project (input to gate G3).
import fs from 'node:fs';
import path from 'node:path';
import { readJson } from './util.mjs';

const FRAMEWORKS = [
  ['next', 'Next.js (React)'],
  ['nuxt', 'Nuxt (Vue)'],
  ['@sveltejs/kit', 'SvelteKit'],
  ['@angular/core', 'Angular'],
  ['@remix-run/react', 'Remix (React)'],
  ['@builder.io/qwik', 'Qwik'],
  ['astro', 'Astro'],
  ['solid-js', 'Solid'],
  ['svelte', 'Svelte'],
  ['vue', 'Vue'],
  ['preact', 'Preact'],
  ['react', 'React'],
  ['lit', 'Lit (Web Components)'],
];
const STYLING = [
  ['tailwindcss', 'Tailwind CSS'],
  ['styled-components', 'styled-components'],
  ['@emotion/react', 'Emotion'],
  ['@vanilla-extract/css', 'vanilla-extract'],
  ['@pandacss/dev', 'Panda CSS'],
  ['unocss', 'UnoCSS'],
  ['sass', 'Sass'],
  ['less', 'Less'],
  ['bootstrap', 'Bootstrap'],
];
const UI_LIBS = [
  ['@mui/material', 'MUI'],
  ['antd', 'Ant Design'],
  ['@chakra-ui/react', 'Chakra UI'],
  ['@mantine/core', 'Mantine'],
  ['@radix-ui/react-slot', 'Radix (likely shadcn/ui)'],
  ['@headlessui/react', 'Headless UI'],
  ['react-aria-components', 'React Aria'],
  ['vuetify', 'Vuetify'],
  ['primevue', 'PrimeVue'],
  ['primereact', 'PrimeReact'],
  ['element-plus', 'Element Plus'],
  ['naive-ui', 'Naive UI'],
  ['@angular/material', 'Angular Material'],
  ['primeng', 'PrimeNG'],
  ['bits-ui', 'Bits UI'],
  ['@skeletonlabs/skeleton', 'Skeleton'],
];

export function detectStack(root) {
  const result = { detectedAt: new Date().toISOString(), kind: 'unknown', framework: null, styling: [], uiLibraries: [], language: null, packageManager: null, evidence: [] };
  const pkgPath = path.join(root, 'package.json');
  if (fs.existsSync(pkgPath)) {
    const pkg = readJson(pkgPath, {});
    const deps = { ...pkg.dependencies, ...pkg.devDependencies };
    result.kind = 'javascript';
    result.evidence.push('package.json');
    for (const [dep, name] of FRAMEWORKS) {
      if (deps[dep]) {
        result.framework = { name, package: dep, version: deps[dep] };
        break;
      }
    }
    for (const [dep, name] of STYLING) if (deps[dep]) result.styling.push({ name, package: dep, version: deps[dep] });
    for (const [dep, name] of UI_LIBS) if (deps[dep]) result.uiLibraries.push({ name, package: dep, version: deps[dep] });
    if (fs.existsSync(path.join(root, 'components.json'))) {
      result.uiLibraries.push({ name: 'shadcn/ui', package: 'components.json' });
      result.evidence.push('components.json');
    }
    result.language = deps.typescript || fs.existsSync(path.join(root, 'tsconfig.json')) ? 'TypeScript' : 'JavaScript';
    for (const [f, pm] of [['pnpm-lock.yaml', 'pnpm'], ['yarn.lock', 'yarn'], ['bun.lockb', 'bun'], ['bun.lock', 'bun'], ['package-lock.json', 'npm']]) {
      if (fs.existsSync(path.join(root, f))) {
        result.packageManager = pm;
        result.evidence.push(f);
        break;
      }
    }
  }
  const other = [
    ['composer.json', 'php', 'PHP (Composer; check for Laravel/Symfony)'],
    ['Gemfile', 'ruby', 'Ruby (check for Rails)'],
    ['go.mod', 'go', 'Go'],
    ['pubspec.yaml', 'dart', 'Flutter/Dart'],
    ['manage.py', 'python', 'Django'],
    ['pyproject.toml', 'python', 'Python'],
    ['Cargo.toml', 'rust', 'Rust'],
  ];
  for (const [f, kind, name] of other) {
    if (fs.existsSync(path.join(root, f))) {
      result.evidence.push(f);
      if (result.kind === 'unknown') {
        result.kind = kind;
        result.framework = { name };
      }
    }
  }
  if (result.kind === 'unknown') {
    const html = fs.existsSync(root) ? fs.readdirSync(root).filter((f) => f.endsWith('.html')) : [];
    if (html.length) {
      result.kind = 'static';
      result.framework = { name: 'Plain HTML/CSS/JS' };
      result.evidence.push(...html.slice(0, 3));
    }
  }
  result.isNewProject = result.evidence.length === 0;
  return result;
}
