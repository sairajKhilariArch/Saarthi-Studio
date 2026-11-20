/** Documentation navigation, shared by the hub, every doc page and the footer. */
export interface DocEntry {
  label: string;
  href: string;
  summary: string;
}

export const DOCS_NAV: Array<{ title: string; items: DocEntry[] }> = [
  {
    title: 'Start here',
    items: [
      {
        label: 'Getting started',
        href: '/docs/getting-started/',
        summary: 'Install, connect a model, open a project, first chat and first agent run.',
      },
      {
        label: 'Troubleshooting',
        href: '/docs/troubleshooting/',
        summary: 'Gatekeeper, SmartScreen, router keys, ports, language servers, recovery.',
      },
    ],
  },
  {
    title: 'Using Studio',
    items: [
      {
        label: 'Providers & models',
        href: '/docs/providers/',
        summary: 'Routers, presets, local Ollama models, fallback, Thinking, deep search.',
      },
      {
        label: 'Agents & runs',
        href: '/docs/agents/',
        summary: 'Build and Plan modes, approvals, checkpoints, worktrees, the task queue.',
      },
      {
        label: 'Workspace & IDE',
        href: '/docs/workspace/',
        summary: 'Projects, files, the editor, terminal, Git, diagnostics, debugging.',
      },
      {
        label: 'Skills & MCP',
        href: '/docs/skills-and-mcp/',
        summary: 'SKILL.md packs, the composer picker, MCP servers and tool permissions.',
      },
    ],
  },
  {
    title: 'Trust',
    items: [
      {
        label: 'Privacy & security',
        href: '/docs/privacy/',
        summary: 'What stays local, what leaves, where data lives, the Host API model.',
      },
    ],
  },
];

/** Flat reading order for previous/next links. */
export const DOCS_ORDER: DocEntry[] = DOCS_NAV.flatMap((group) => group.items);
