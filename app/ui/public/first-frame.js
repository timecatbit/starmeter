// 在手机上看, opened (again): the page's ground, the browser's bars and the computer's theme painted before anything is
// drawn. iOS Safari takes its bars' colours from the first frame and keeps them, so that frame must be in the
// computer's look as it is now: the phone server writes it into the page (window.STARMETER_LOOK, phone.rs), and these
// are 网页版's sky edges for it (PhoneStage.svelte, sketch.css). Without it (an older app, the dev server), the colours
// of the last load (src/lib/edges.ts keeps them). Nothing on the computer's own panel.
(function () {
  try {
    if (!/(^#|&)k=[0-9a-f]{32}/.test(location.hash)) return;
    var root = document.documentElement;
    // The bars' samplers (index.html) on, at the colours below.
    root.classList.add('phone-page');
    // The page's storage on its own (STAGE-15): refused (a private window) or spoilt, the colours are still painted.
    var kept = {};
    try { kept = JSON.parse(localStorage.getItem('starmeter.edges') || 'null') || {}; } catch (e) { kept = {}; }
    var look = window.STARMETER_LOOK;
    var top, mid, bottom, theme;
    if (look) {
      theme = look.theme;
      var dark = theme === 'dark' || (theme !== 'light' && window.matchMedia('(prefers-color-scheme: dark)').matches);
      var sketch = look.style === 'sketch';
      var paper = dark ? '#1f2924' : '#fdfaf4';
      if (sketch) { top = paper; mid = paper; bottom = paper; }
      else { top = dark ? '#1F3A7A' : '#DCE3FF'; mid = dark ? '#0E1D4C' : '#EEF0FF'; bottom = dark ? '#050A24' : '#F7F6FF'; }
      // The look itself too, so the page's first drawing is in it (style.svelte.ts reads this).
      try { localStorage.setItem('starmeter.style', sketch ? 'sketch' : 'standard'); } catch (e) { /* this load only */ }
    } else {
      theme = kept.theme; top = kept.top; mid = kept.mid; bottom = kept.bottom;
    }
    if (theme === 'light' || theme === 'dark') root.dataset.theme = theme;
    if (!top || !bottom) return;
    var meta = document.createElement('meta');
    meta.name = 'theme-color';
    meta.content = top;
    document.head.appendChild(meta);
    root.style.setProperty('--bar-top', top);
    root.style.setProperty('--bar-bottom', bottom);
    root.style.backgroundColor = bottom;
    // The two colours over the whole screen in one piece: before the stylesheet (height: 100%) the page has no height,
    // and its background would repeat in thin stripes.
    root.style.minHeight = '100%';
    // And beyond the page: the top's colour all the way up, the bottom's all the way down. Safari 26 lays the page out
    // between its bars (the status bar's band, the toolbar's) and shows the page's ground in them; repeated, the
    // gradient put the bottom's colour (near white by day) under the status bar. On the screen itself, 网页版's sky as
    // the page draws it (src/lib/edges.ts, skyGround), not two halves meeting in a line across the middle.
    root.style.backgroundImage = mid
      ? 'linear-gradient(' + top + ' 0, ' + top + ' calc(100% / 3 + 72px), ' + mid + ' calc(100% / 3 + 16%), ' + bottom + ' calc(200% / 3 - 90px), ' + bottom + ' 100%)'
      : 'linear-gradient(' + top + ' 0, ' + top + ' calc(100% / 3), ' + bottom + ' calc(200% / 3), ' + bottom + ' 100%)';
    root.style.backgroundSize = '100% 300%';
    root.style.backgroundPosition = 'center';
    root.style.backgroundRepeat = 'no-repeat';
  } catch (e) { /* the page paints them itself in a moment */ }
})();
