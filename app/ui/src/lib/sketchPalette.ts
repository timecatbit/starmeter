// 手绘's inks: the 手绘 share picture (shareSketch.ts) draws with them, and so does the panel drawn by hand (外观 → 风格
// → 手绘, sketch.css). By day coloured markers on dotted journal paper, by night coloured chalk on a blackboard.

export const SKETCH = {
  light: {
    paper: '#FFFDF7', dots: 'rgba(60, 60, 90, 0.13)', ink: '#2D2A33', soft: 'rgba(45, 42, 51, 0.62)', highlight: 'rgba(255, 214, 64, 0.55)', warn: '#E8590C',
    pens: { coral: '#F0565A', orange: '#F08C2E', yellow: '#E8B10C', green: '#37B24D', teal: '#1098AD', blue: '#3B5BDB', violet: '#7950F2', pink: '#E64980', grey: '#868E96' },
  },
  dark: {
    paper: '#243029', dots: 'rgba(255, 255, 255, 0.05)', ink: '#F1EFE7', soft: 'rgba(241, 239, 231, 0.62)', highlight: 'rgba(255, 224, 102, 0.28)', warn: '#FFA94D',
    pens: { coral: '#FF8787', orange: '#FFC078', yellow: '#FFE066', green: '#8CE99A', teal: '#66D9E8', blue: '#91A7FF', violet: '#B197FC', pink: '#F783AC', grey: '#CED4DA' },
  },
};
export type Pen = keyof typeof SKETCH.light.pens;

/** Each tool's pen; tools without one take blue. */
export const PENS: Record<string, Pen> = {
  claude: 'orange', codex: 'violet', magpie: 'teal', opencode: 'pink', qoder: 'green', gemini: 'blue', cursor: 'grey',
  workbuddy: 'coral', 'deepseek-harness': 'blue', opencodex: 'blue', other: 'grey',
};

/** A pen's colour, by day or by night. */
export const pen = (name: Pen, dark: boolean) => SKETCH[dark ? 'dark' : 'light'].pens[name];

/** A tool's pen colour. */
export const toolPen = (id: string, dark: boolean) => pen(PENS[id] ?? 'blue', dark);
