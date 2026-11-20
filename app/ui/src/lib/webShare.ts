// The share picture on a phone (在手机上看): the browser's own ways out instead of the app's. Copy and the system share
// sheet need a secure page (the public https link); saving works on any page, as a download, and a long press on the
// picture saves it to the photos as well.

const file = (png: Uint8Array, name: string) => new File([png as BlobPart], name, { type: 'image/png' });

/** Whether this browser can put a picture on the clipboard (Safari and Chrome do on an https page). */
export const canCopyPicture = () =>
  typeof window !== 'undefined' && window.isSecureContext && typeof ClipboardItem !== 'undefined' && !!navigator.clipboard?.write;

export async function copyPicture(png: Uint8Array): Promise<boolean> {
  try {
    await navigator.clipboard.write([new ClipboardItem({ 'image/png': new Blob([png as BlobPart], { type: 'image/png' }) })]);
    return true;
  } catch {
    return false;
  }
}

/** Whether the system share sheet takes a picture here. */
export function canSharePicture(): boolean {
  if (typeof navigator === 'undefined' || typeof navigator.share !== 'function' || typeof navigator.canShare !== 'function') return false;
  try {
    return navigator.canShare({ files: [file(new Uint8Array(), 'Starmeter.png')] });
  } catch {
    return false;
  }
}

/** The share sheet with the picture; false when the reader closed it or it failed. */
export async function sharePicture(png: Uint8Array, name: string): Promise<boolean> {
  try {
    await navigator.share({ files: [file(png, name)] });
    return true;
  } catch {
    return false;
  }
}

/** Saves the picture as a download: the Files app or the downloads folder. */
export function savePicture(png: Uint8Array, name: string): boolean {
  const url = URL.createObjectURL(new Blob([png as BlobPart], { type: 'image/png' }));
  const link = Object.assign(document.createElement('a'), { href: url, download: name });
  document.body.append(link);
  link.click();
  link.remove();
  setTimeout(() => URL.revokeObjectURL(url), 60_000);
  return true;
}
