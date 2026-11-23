export const SITE = {
  name: 'SAARTHI-STUDIO',
  title: 'SAARTHI-STUDIO — a local-first AI development workspace',
  description:
    'SAARTHI-STUDIO is a local-first AI development workspace with chat, agents, local models, routers, skills, MCP, memory, and a real developer environment.',
  repo: 'https://github.com/sairajKhilariArch/Saarthi-Studio',
  releases: 'https://github.com/sairajKhilariArch/Saarthi-Studio/releases',
  issues: 'https://github.com/sairajKhilariArch/Saarthi-Studio/issues',
  license: 'https://github.com/sairajKhilariArch/Saarthi-Studio/blob/main/LICENSE',
  readme: 'https://github.com/sairajKhilariArch/Saarthi-Studio/blob/main/README.md',
  architecture:
    'https://github.com/sairajKhilariArch/Saarthi-Studio/blob/main/docs/ARCHITECTURE.md',
  featureStatus:
    'https://github.com/sairajKhilariArch/Saarthi-Studio/blob/main/docs/FEATURE-STATUS.md',
  testing:
    'https://github.com/sairajKhilariArch/Saarthi-Studio/blob/main/docs/TESTING-STRATEGY.md',
} as const;

/**
 * Primary navigation. Downloads and Docs are dedicated multi-page sections;
 * Features stays an anchor on the home page.
 */
export const NAV = [
  { label: 'Features', href: '/#capabilities' },
  { label: 'Downloads', href: '/download/' },
  { label: 'Docs', href: '/docs/' },
] as const;
