import type { APIRoute } from 'astro';
import { withBase } from '../lib/url';

export const GET: APIRoute = ({ site }) => {
  const origin = site ?? new URL('https://sairajkhilariarch.github.io/Saarthi-Studio');
  const sitemap = new URL(withBase('/sitemap-index.xml'), origin);

  return new Response(
    ['User-agent: *', 'Allow: /', '', `Sitemap: ${sitemap.href}`, ''].join('\n'),
    {
      headers: {
        'Content-Type': 'text/plain; charset=utf-8',
      },
    },
  );
};
