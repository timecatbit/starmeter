// The share pictures: their own layouts, never the panel's glass (what shows through depends on each desktop), in six
// styles picked on the share page, which previews the picture before it is copied, saved or shared. They cover what
// Summary counts; subscriptions, balances and keys never appear. Every style says the same things in the same order
// (sharePoster.ts works them out); a new style is a drawing function registered below.
import { drawCardPicture } from './shareCard';
import { drawKnitPicture } from './shareKnit';
import { drawMetroPicture } from './shareMetro';
import { drawNightPicture } from './shareNight';
import { drawPaperPicture } from './sharePaper';
import { drawTerminalPicture } from './shareTerminal';
import type { Dashboard, Project } from './types';

export type ShareStyle = 'card' | 'paper' | 'night' | 'metro' | 'knit' | 'terminal';

/** In the order the page offers them. 卡片 and 终端 follow light and dark; the posters keep their own colours. */
export const shareStyles: { key: ShareStyle; name: string }[] = [
  { key: 'card', name: '卡片' }, { key: 'paper', name: '报刊' }, { key: 'night', name: '夜空' },
  { key: 'metro', name: '地铁' }, { key: 'knit', name: '织物' }, { key: 'terminal', name: '终端' },
];

const draw: Record<ShareStyle, (d: Dashboard, project: Project, dark: boolean) => Promise<HTMLCanvasElement>> = {
  card: drawCardPicture, terminal: drawTerminalPicture,
  paper: (d, project) => drawPaperPicture(d, project), night: (d, project) => drawNightPicture(d, project),
  metro: (d, project) => drawMetroPicture(d, project), knit: (d, project) => drawKnitPicture(d, project),
};

export interface Picture { png: Uint8Array; url: string; width: number; height: number }

/** The picture at 2x: its PNG bytes, a URL to preview it by, and its size in points. */
export async function renderPicture(style: ShareStyle, dashboard: Dashboard, project: Project, dark: boolean): Promise<Picture | null> {
  const canvas = await draw[style](dashboard, project, dark);
  const blob = await new Promise<Blob | null>((resolve) => canvas.toBlob(resolve, 'image/png'));
  if (!blob) return null;
  return { png: new Uint8Array(await blob.arrayBuffer()), url: URL.createObjectURL(blob), width: canvas.width / 2, height: canvas.height / 2 };
}
