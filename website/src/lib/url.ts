const configuredBase = import.meta.env.BASE_URL || '/';

/**
 * Prefix an internal path with Astro's configured base.
 *
 * The production site is served from /Saarthi-Studio/ on GitHub Pages, while
 * local development and a future custom domain use /. Nothing in the site
 * should concatenate a leading slash by hand.
 */
export function withBase(path = '/'): string {
  if (path.startsWith('#')) return path;
  if (/^(?:[a-z][a-z0-9+.-]*:|\/\/)/i.test(path)) return path;

  const base = configuredBase.endsWith('/')
    ? configuredBase.slice(0, -1)
    : configuredBase;
  const normalizedPath = path.startsWith('/') ? path : `/${path}`;

  return `${base}${normalizedPath}` || '/';
}

export function isExternal(href: string): boolean {
  return /^(?:https?:)?\/\//i.test(href) || /^(?:mailto|tel):/i.test(href);
}
