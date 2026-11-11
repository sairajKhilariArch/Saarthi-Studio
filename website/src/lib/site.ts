export const SITE = {
  name: 'Sarthi Studio',
  title: 'Sarthi Studio — a local-first AI development workspace',
  description:
    'Sarthi Studio is a local-first AI development workspace with chat, agents, local models, routers, skills, MCP, memory, and a real developer environment.',
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
 * Foundation navigation. These are same-page sections until the dedicated
 * feature and documentation routes land in later milestones.
 */
export const NAV = [
  { label: 'Capabilities', href: '/#capabilities' },
  { label: 'How it works', href: '/#how-it-works' },
  { label: 'Platforms', href: '/#platforms' },
  { label: 'Source', href: '/#source' },
] as const;
