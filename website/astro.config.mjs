import { defineConfig } from 'astro/config';
import mdx from '@astrojs/mdx';
import sitemap from '@astrojs/sitemap';

// Local development defaults to the root; the Pages workflow sets the
// production origin and project subpath explicitly. The site origin defaults
// to the live Pages host so a plain `npm run build` (no env vars) produces a
// production-correct, link-check-clean build. Override SITE_URL for local dev.
const site = process.env.SITE_URL ?? 'https://sairajkhilariarch.github.io/Saarthi-Studio';
const base = process.env.SITE_BASE ?? '/';

export default defineConfig({
  site,
  base,
  output: 'static',
  trailingSlash: 'always',
  build: {
    format: 'directory',
  },
  integrations: [mdx(), sitemap()],
});
