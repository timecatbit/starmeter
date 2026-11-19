// The share picture: its own layout, never the panel's glass (what shows through depends on each desktop), in two
// styles picked when copying or sharing. It covers what Summary counts; subscriptions, balances and keys never appear.
import { inTauri, shell } from './backend';
import { drawCardPicture } from './shareCard';
import { drawTerminalPicture } from './shareTerminal';
import { panel } from './ui.svelte';
import type { Dashboard, Project } from './types';

export type ShareStyle = 'card' | 'terminal';
export interface SharePick { style: ShareStyle; op: 'copy' | 'share' }

const styles: [ShareStyle, string][] = [['card', '卡片风格'], ['terminal', '终端风格']];

/** Our own small menu, grouped by style, each with 复制图片 and 分享…; native inside the app. */
export async function openShareMenu(button: HTMLElement): Promise<SharePick | null> {
  const box = button.getBoundingClientRect();
  panel.menuOpen = true;
  await shell.holdOpen(true);
  try {
    if (inTauri) return await nativeMenu(box);
    return await pageMenu(button);
  } finally {
    panel.menuOpen = false;
    void shell.holdOpen(false);
  }
}

async function nativeMenu(box: DOMRect): Promise<SharePick | null> {
  const { Menu, MenuItem, PredefinedMenuItem } = await import('@tauri-apps/api/menu');
  const { LogicalPosition } = await import('@tauri-apps/api/dpi');
  let picked: SharePick | null = null;
  const share = navigator.userAgent.includes('Mac') ? '分享…' : '保存图片…';
  const items = [];
  for (const [index, [style, title]] of styles.entries()) {
    if (index) items.push(await PredefinedMenuItem.new({ item: 'Separator' }));
    items.push(await MenuItem.new({ text: title, enabled: false }));
    items.push(await MenuItem.new({ text: '复制图片', action: () => (picked = { style, op: 'copy' }) }));
    items.push(await MenuItem.new({ text: share, action: () => (picked = { style, op: 'share' }) }));
  }
  const menu = await Menu.new({ items });
  await menu.popup(new LogicalPosition(box.left, box.bottom + 4));
  // The chosen item's event arrives just after the menu closes.
  await new Promise((resolve) => setTimeout(resolve, 60));
  return picked;
}

/** The browser preview's stand-in for the native menu. */
function pageMenu(button: HTMLElement): Promise<SharePick | null> {
  return new Promise((resolve) => {
    const frame = button.closest('.window') as HTMLElement;
    const box = button.getBoundingClientRect(), origin = frame.getBoundingClientRect();
    const menu = document.createElement('div');
    menu.className = 'share-menu';
    menu.setAttribute('role', 'menu');
    Object.assign(menu.style, { position: 'absolute', left: `${box.left - origin.left}px`, top: `${box.bottom - origin.top + 4}px`, zIndex: '20', minWidth: '140px',
      padding: '5px', borderRadius: '8px', background: 'var(--solid)', boxShadow: '0 0 0 0.5px var(--separator), 0 6px 20px rgba(0,0,0,.18)', fontSize: '12px' });
    const done = (pick: SharePick | null) => { menu.remove(); document.removeEventListener('pointerdown', outside, true); resolve(pick); };
    const outside = (event: Event) => { if (!menu.contains(event.target as Node)) done(null); };
    for (const [index, [style, title]] of styles.entries()) {
      if (index) menu.append(Object.assign(document.createElement('div'), { style: 'height:1px;margin:5px 4px;background:var(--separator)' }));
      menu.append(Object.assign(document.createElement('div'), { textContent: title, style: 'padding:3px 8px;font-size:11px;font-weight:600;color:var(--secondary)' }));
      for (const [op, text] of [['copy', '复制图片'], ['share', '分享…']] as const) {
        const item = Object.assign(document.createElement('button'), { textContent: text, className: 'share-item' });
        item.setAttribute('role', 'menuitem');
        Object.assign(item.style, { display: 'block', width: '100%', padding: '4px 8px', borderRadius: '5px' });
        item.addEventListener('pointerenter', () => (item.style.background = 'var(--accent)', item.style.color = 'white'));
        item.addEventListener('pointerleave', () => (item.style.background = '', item.style.color = ''));
        item.addEventListener('click', () => done({ style, op }));
        menu.append(item);
      }
    }
    frame.append(menu);
    setTimeout(() => document.addEventListener('pointerdown', outside, true), 0);
  });
}

/** The picture at 2x, as PNG bytes. */
export async function renderPicture(style: ShareStyle, dashboard: Dashboard, project: Project, dark: boolean): Promise<Uint8Array | null> {
  const canvas = style === 'terminal' ? await drawTerminalPicture(dashboard, project, dark) : await drawCardPicture(dashboard, project, dark);
  const blob = await new Promise<Blob | null>((resolve) => canvas.toBlob(resolve, 'image/png'));
  return blob ? new Uint8Array(await blob.arrayBuffer()) : null;
}
