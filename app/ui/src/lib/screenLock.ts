// 网页版's 保持亮屏 (a monitor by the desk): the screen wake lock, held while the switch is on and the page shows. At
// most one request out at a time; a lock that arrives after the switch went off (or the page went away) is let go at
// once; the system's own letting go (the page hidden) is noticed, and the lock taken again when the page comes back.

interface Sentinel {
  release: () => Promise<void>;
  addEventListener?: (type: 'release', listener: () => void) => void;
}

type Request = () => Promise<Sentinel>;

export class ScreenLock {
  private held: Sentinel | null = null;
  private asking = false;
  private wanted = false;
  private gone = false;
  /** Why the browser last refused it (NotAllowedError: Low Power Mode, a page not in front), for the page to say. */
  refusal: string | null = null;

  constructor(private request: Request) {}

  /** Whether the browser offers it (an https page, iOS 16.4 on). */
  static offered(): boolean {
    return typeof navigator !== 'undefined' && 'wakeLock' in navigator;
  }

  static browser(): ScreenLock {
    return new ScreenLock(() => (navigator as unknown as { wakeLock: { request: (kind: string) => Promise<Sentinel> } }).wakeLock.request('screen'));
  }

  /** The switch turned on or off; on, the lock taken (resolves false if the browser refused it). */
  async hold(on: boolean): Promise<boolean> {
    this.wanted = on;
    if (!on) { this.letGo(); return true; }
    return this.take();
  }

  /** The page shows again: the lock the system let go of taken again, if still wanted. */
  resume(): Promise<boolean> {
    return this.wanted ? this.take() : Promise.resolve(true);
  }

  /** The page went away: nothing held, nothing taken later. */
  dispose() {
    this.gone = true;
    this.wanted = false;
    this.letGo();
  }

  private async take(): Promise<boolean> {
    if (this.held || this.asking || this.gone) return true;
    this.asking = true;
    let got: Sentinel;
    try { got = await this.request(); } catch (error) {
      this.asking = false; this.wanted = false;
      this.refusal = error instanceof Error ? error.name : 'Error';
      return false;
    }
    this.refusal = null;
    this.asking = false;
    if (!this.wanted || this.gone) {
      void got.release().catch(() => {});
      return true;
    }
    this.held = got;
    got.addEventListener?.('release', () => { if (this.held === got) this.held = null; });
    return true;
  }

  private letGo() {
    const held = this.held;
    this.held = null;
    void held?.release().catch(() => {});
  }
}
