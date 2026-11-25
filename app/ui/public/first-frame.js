// 在手机上看, opened again (a reload, the same link): the page's ground, the browser's bars and the computer's theme as
// they were a moment ago (src/lib/edges.ts keeps them), painted before anything is drawn. Safari tints its bars from
// the first frame, which would otherwise be the phone's own theme. Nothing on the computer's own panel.
(function () {
  try {
    if (!/(^#|&)k=[0-9a-f]{32}/.test(location.hash)) return;
    var kept = JSON.parse(localStorage.getItem('starmeter.edges') || 'null');
    if (!kept) return;
    var root = document.documentElement;
    if (kept.theme === 'light' || kept.theme === 'dark') root.dataset.theme = kept.theme;
    if (!kept.top || !kept.bottom) return;
    var meta = document.createElement('meta');
    meta.name = 'theme-color';
    meta.content = kept.top;
    document.head.appendChild(meta);
    root.style.backgroundColor = kept.bottom;
    root.style.backgroundImage = 'linear-gradient(' + kept.top + ' 0 50%, ' + kept.bottom + ' 50% 100%)';
  } catch (e) { /* the page paints them itself in a moment */ }
})();
