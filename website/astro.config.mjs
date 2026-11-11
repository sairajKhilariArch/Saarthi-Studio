import { defineConfig } from 'astro/config';
import mdx from '@astrojs/mdx';
import sitemap from '@astrojs/sitemap';

// Local development defaults to the root; the Pages workflow sets the
// production origin and project subpath explicitly.
const site = process.env.SITE_URL ?? 'http://localhost:4321';
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
