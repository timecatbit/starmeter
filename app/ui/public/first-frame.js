// 在手机上看, opened (again): the page's ground, the browser's bars and the computer's theme painted before anything is
// drawn. iOS Safari takes its bars' colours from the first frame and keeps them, so that frame must be in the
// computer's look as it is now: the phone server writes it into the page (window.STARMETER_LOOK, phone.rs), and these
// are 网页版's sky edges and 电脑版's ground for it (PhoneStage.svelte, app.css, sketch.css). Without it (an older
// app, the dev server), the colours of the last load (src/lib/edges.ts keeps them). Nothing on the computer's own panel.
(function () {
  try {
    if (!/(^#|&)k=[0-9a-f]{32}/.test(location.hash)) return;
    var root = document.documentElement;
    var kept = JSON.parse(localStorage.getItem('starmeter.edges') || 'null') || {};
    var look = window.STARMETER_LOOK;
    var top, bottom, theme;
    if (look) {
      theme = look.theme;
      var dark = theme === 'dark' || (theme !== 'light' && window.matchMedia('(prefers-color-scheme: dark)').matches);
      var sketch = look.style === 'sketch';
      // The phone's own choice of look: 网页版 (the sky) or 电脑版 (the panel's ground).
      var asked = (location.hash.match(/(^#|&)v=(stage|panel)/) || [])[2];
      var cookie = (document.cookie.match(/(?:^|; )starmeter-phone-look=(stage|panel)/) || [])[1];
      var chosen = asked || cookie || localStorage.getItem('starmeter-phone-look') || 'stage';
      var paper = dark ? '#1f2924' : '#fdfaf4';
      if (sketch) { top = paper; bottom = paper; }
      else if (chosen === 'panel') { top = bottom = dark ? 'rgb(29, 29, 29)' : 'rgb(253, 253, 253)'; }
      else { top = dark ? '#1F3A7A' : '#DCE3FF'; bottom = dark ? '#050A24' : '#F7F6FF'; }
      // The look itself too, so the page's first drawing is in it (style.svelte.ts reads this).
      localStorage.setItem('starmeter.style', sketch ? 'sketch' : 'standard');
    } else {
      theme = kept.theme; top = kept.top; bottom = kept.bottom;
    }
    if (theme === 'light' || theme === 'dark') root.dataset.theme = theme;
    if (!top || !bottom) return;
    var meta = document.createElement('meta');
    meta.name = 'theme-color';
    meta.content = top;
    document.head.appendChild(meta);
    root.style.backgroundColor = bottom;
    // The two colours over the whole screen in one piece: before the stylesheet (height: 100%) the page has no height,
    // and its background would repeat in thin stripes.
    root.style.minHeight = '100%';
    root.style.backgroundImage = 'linear-gradient(' + top + ' 0 50%, ' + bottom + ' 50% 100%)';
  } catch (e) { /* the page paints them itself in a moment */ }
})();
