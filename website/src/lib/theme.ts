export const THEMES = [
  { value: 'hermes', label: 'Hermes', color: '#38bdf8' },
  { value: 'deepseek', label: 'DeepSeek', color: '#7c8aff' },
  { value: 'solar', label: 'Solar', color: '#c084fc' },
  { value: 'forest', label: 'Forest', color: '#34d399' },
  { value: 'ocean', label: 'Ocean', color: '#38bdf8' },
  { value: 'midnight', label: 'Midnight', color: '#818cf8' },
  { value: 'indigo', label: 'Indigo', color: '#818cf8' },
  { value: 'sunset', label: 'Sunset', color: '#fbbf24' },
] as const;

export type ThemeValue = (typeof THEMES)[number]['value'];

export const storedAndValid = (value: string | null): value is ThemeValue =>
  typeof value === 'string' && THEMES.some((t) => t.value === value);
