// The language switch (the choice is kept for the next visit), the copy button for the Homebrew command, and the
// page's motion: twinkling halftone skies, pieces rising into view and the effects tied to scrolling. Motion runs only
// when the page set html.motion (the system doesn't ask to reduce motion); otherwise the skies hold still.
(function () {
  var root = document.documentElement;
  var titles = {
    zh: 'Starmeter · 在菜单栏一眼看清 AI 编程工具用了多少',
    en: 'Starmeter · See how much your AI coding tools use, from the menu bar',
  };
  var descriptions = {
    zh: document.querySelector('meta[name="description"]').content,
    en: 'Starmeter shows the tokens and API-equivalent cost of Claude Code, Codex, Cursor, Gemini CLI and more, your subscription limits, API balances and active sessions, in the macOS menu bar and the Windows tray. Free, open source, and your data stays on your computer.',
  };

  function show(lang) {
    root.classList.toggle('en', lang === 'en');
    root.lang = lang === 'en' ? 'en' : 'zh-CN';
    document.title = titles[lang];
    document.querySelector('meta[name="description"]').content = descriptions[lang];
  }
  show(root.classList.contains('en') ? 'en' : 'zh');

  document.querySelector('[data-switch-lang]').addEventListener('click', function () {
    var lang = root.classList.contains('en') ? 'zh' : 'en';
    show(lang);
    try { localStorage.setItem('starmeter-lang', lang); } catch (e) {}
    var url = new URL(location.href);
    url.searchParams.delete('lang');
    history.replaceState(null, '', url);
  });

  document.querySelectorAll('[data-copy]').forEach(function (button) {
    var label = button.innerHTML;
    button.addEventListener('click', function () {
      navigator.clipboard.writeText(button.getAttribute('data-copy')).then(function () {
        button.innerHTML = '<span class="zh">已复制</span><span class="en">Copied</span>';
        setTimeout(function () { button.innerHTML = label; }, 1800);
      });
    });
  });

  // MARK: Motion

  root.classList.add('ready');
  var moving = root.classList.contains('motion');
  document.querySelectorAll('canvas.starfield').forEach(function (canvas) { starfield(canvas, moving); });
  if (moving) {
    reveal();
    marquees();
  }
  scrolling(moving);

  function clamp(value) { return Math.min(1, Math.max(0, value)); }

  // A halftone night sky like the logo's: a square grid of dots whose size and brightness follow the light under them,
  // brightest at one edge of the section (data-sky) and fading away from it. Every dot twinkles at its own slow pace,
  // and every few seconds a few neighbouring dots swell into a sparkle, the logo's own four-pointed star, then fade.
  // Sparkles keep clear of the text named in data-avoid. With reduced motion, one still frame with a few sparkles.
  function starfield(canvas, moving) {
    var context = canvas.getContext('2d');
    if (!context) return;
    var PITCH = 18, LEVELS = 12, TAU = Math.PI * 2, FRAME = 1000 / 30;
    var fromBottom = canvas.getAttribute('data-sky') === 'bottom';
    var width = 0, height = 0, ratio = 1, columns = 0, rows = 0;
    var xs, ys, base, swing, pace, phase, glow, bright, level, order;
    var counts = new Int32Array(LEVELS), colours = [], stencils = {}, sparkles = [];
    var begun = performance.now(), nextSparkle = 0, previous = 0, frame = 0, inView = false;

    // How much light the sky gives at (x, y): full along the bright edge's middle, half at its corners, none 85% away.
    function sky(x, y) {
      var away = (fromBottom ? height - y : y) / height, across = (x - width / 2) / (width / 2);
      return Math.pow(Math.max(0, 1 - away / 0.85), 1.4) * (0.45 + 0.55 * Math.exp(-across * across / 0.3));
    }

    // The logo's sparkle (scripts/make_logo.py): 1 at the centre, 0 past the tips, x and y from -1 to 1.
    function sparkle(x, y) {
      var edge = Math.pow(Math.pow(Math.abs(x), 0.66) + Math.pow(Math.abs(y), 0.66), 1 / 0.66);
      return Math.min(1, Math.pow(Math.max(0, 1 - edge), 0.8) * 1.25 + 0.3 * Math.exp(-(x * x + y * y) / 0.18));
    }

    function stencil(reach) {
      if (!stencils[reach]) {
        var side = reach * 2 + 1, values = new Float32Array(side * side);
        for (var r = 0; r < side; r++) {
          for (var c = 0; c < side; c++) values[r * side + c] = sparkle((c - reach) / reach, (r - reach) / reach);
        }
        stencils[reach] = values;
      }
      return stencils[reach];
    }

    function recolour() {
      var style = getComputedStyle(canvas);
      var from = style.getPropertyValue('--star-from').split(',').map(Number);
      var to = style.getPropertyValue('--star-to').split(',').map(Number);
      colours = [];
      for (var k = 0; k < LEVELS; k++) {
        var mix = (k + 0.5) / LEVELS, part = function (n) { return from[n] + (to[n] - from[n]) * mix; };
        colours.push('rgba(' + Math.round(part(0)) + ',' + Math.round(part(1)) + ',' + Math.round(part(2)) + ',' + part(3).toFixed(3) + ')');
      }
    }

    function layout() {
      width = canvas.clientWidth;
      height = canvas.clientHeight;
      if (!width || !height) return false;
      ratio = Math.min(window.devicePixelRatio || 1, 2);
      canvas.width = Math.round(width * ratio);
      canvas.height = Math.round(height * ratio);
      columns = Math.ceil(width / PITCH) + 1;
      rows = Math.ceil(height / PITCH) + 1;
      var count = columns * rows, left = (width / 2) % PITCH;
      xs = new Float32Array(count); ys = new Float32Array(count); base = new Float32Array(count);
      swing = new Float32Array(count); pace = new Float32Array(count); phase = new Float32Array(count);
      glow = new Float32Array(count); bright = new Float32Array(count);
      level = new Uint8Array(count); order = new Int32Array(count);
      for (var r = 0; r < rows; r++) {
        for (var c = 0; c < columns; c++) {
          var i = r * columns + c;
          xs[i] = left + c * PITCH;
          ys[i] = PITCH / 2 + r * PITCH;
          // Mostly faint, a few brighter, like distant stars; about a third twinkle deeply, the rest barely.
          base[i] = sky(xs[i], ys[i]) * (0.1 + 0.55 * Math.pow(Math.random(), 2));
          swing[i] = Math.random() < 0.32 ? 0.9 : 0.3;
          pace[i] = TAU / (3 + Math.random() * 6);
          phase[i] = Math.random() * TAU;
        }
      }
      // A few sparkles already on their way, so the sky isn't empty when it appears; held at their brightest if still.
      var now = performance.now();
      sparkles = [];
      for (var k = 0; k < Math.ceil(wanted() * 0.6); k++) {
        var star = place(now);
        if (star) {
          star.born = now - star.life * 1000 * (moving ? Math.random() * 0.8 : 0.5);
          sparkles.push(star);
        }
      }
      return true;
    }

    function wanted() { return Math.max(2, Math.min(6, Math.round(width * height / 220000))); }

    // Somewhere in the brighter part of the sky, clear of the text and of the other sparkles.
    function place(now) {
      var box = canvas.parentElement.getBoundingClientRect(), selector = canvas.getAttribute('data-avoid');
      var holes = selector ? Array.prototype.map.call(document.querySelectorAll(selector), function (element) {
        var r = element.getBoundingClientRect();
        return { left: r.left - box.left, right: r.right - box.left, top: r.top - box.top, bottom: r.bottom - box.top, empty: !r.width };
      }) : [];
      for (var tries = 0; tries < 60; tries++) {
        var c = Math.floor(Math.random() * columns), r = Math.floor(Math.random() * rows), i = r * columns + c;
        var light = sky(xs[i], ys[i]);
        if (light < 0.3 || Math.random() > light * light) continue;
        var reach = 3 + Math.floor(Math.random() * Math.random() * 4), margin = (reach + 1) * PITCH;
        var x = xs[i], y = ys[i];
        var blocked = holes.some(function (h) {
          return !h.empty && x > h.left - margin && x < h.right + margin && y > h.top - margin && y < h.bottom + margin;
        }) || sparkles.some(function (o) {
          return Math.abs(o.c - c) < o.reach + reach + 3 && Math.abs(o.r - r) < o.reach + reach + 3;
        });
        if (!blocked) return { c: c, r: r, reach: reach, born: now, life: 5 + Math.random() * 4, peak: 0.75 + Math.random() * 0.25 };
      }
      return null;
    }

    function draw(now) {
      var t = now / 1000, count = columns * rows, k, i;
      glow.fill(0);
      sparkles = sparkles.filter(function (star) {
        var age = (now - star.born) / 1000 / star.life;
        if (age >= 1) return false;
        var strength = Math.pow(Math.sin(Math.PI * Math.max(0, age)), 2) * star.peak;
        var shape = stencil(star.reach), side = star.reach * 2 + 1;
        for (var dr = -star.reach; dr <= star.reach; dr++) {
          var row = star.r + dr;
          if (row < 0 || row >= rows) continue;
          for (var dc = -star.reach; dc <= star.reach; dc++) {
            var column = star.c + dc;
            if (column < 0 || column >= columns) continue;
            var value = shape[(dr + star.reach) * side + dc + star.reach] * strength, index = row * columns + column;
            if (value > glow[index]) glow[index] = value;
          }
        }
        return true;
      });
      if (moving && sparkles.length < wanted() && now >= nextSparkle) {
        var star = place(now);
        if (star) sparkles.push(star);
        nextSparkle = now + 600 + Math.random() * 2400;
      }

      // Each dot's brightness, then the dots sorted by brightness so each shade is one path.
      counts.fill(0);
      for (i = 0; i < count; i++) {
        var b = base[i] * (moving ? 1 + swing[i] * Math.sin(pace[i] * t + phase[i]) : 1);
        if (glow[i]) b = 1 - (1 - b) * (1 - glow[i]);
        if (b < 0.015) { level[i] = 255; continue; }
        b = Math.min(1, b);
        bright[i] = b;
        level[i] = Math.min(LEVELS - 1, Math.floor(b * LEVELS));
        counts[level[i]]++;
      }
      var starts = [], total = 0;
      for (k = 0; k < LEVELS; k++) { starts.push(total); total += counts[k]; }
      var cursor = starts.slice();
      for (i = 0; i < count; i++) if (level[i] !== 255) order[cursor[level[i]]++] = i;

      context.setTransform(ratio, 0, 0, ratio, 0, 0);
      context.clearRect(0, 0, width, height);
      context.globalAlpha = moving ? clamp((now - begun) / 1800) : 1;
      for (k = 0; k < LEVELS; k++) {
        if (!counts[k]) continue;
        context.fillStyle = colours[k];
        context.beginPath();
        for (var n = starts[k]; n < starts[k] + counts[k]; n++) {
          i = order[n];
          // Halftone: the dot's area follows its brightness.
          var radius = PITCH * (0.04 + 0.24 * Math.sqrt(bright[i]));
          context.moveTo(xs[i] + radius, ys[i]);
          context.arc(xs[i], ys[i], radius, 0, TAU);
        }
        context.fill();
      }
    }

    function tick(now) {
      frame = 0;
      if (!inView || document.hidden) return;
      frame = requestAnimationFrame(tick);
      if (now - previous < FRAME) return;
      previous = now;
      draw(now);
    }
    function run() { if (moving && !frame && inView && !document.hidden) frame = requestAnimationFrame(tick); }
    function redraw() { if (layout()) draw(performance.now()); }

    recolour();
    if (!layout()) return;
    canvas.classList.add('live');
    draw(performance.now());

    // Only animate while the sky is on screen and the page is in front.
    new IntersectionObserver(function (entries) {
      inView = entries[entries.length - 1].isIntersecting;
      run();
    }).observe(canvas);
    document.addEventListener('visibilitychange', run);
    var size = width + 'x' + height;
    new ResizeObserver(function () {
      if (canvas.clientWidth + 'x' + canvas.clientHeight === size) return;
      size = canvas.clientWidth + 'x' + canvas.clientHeight;
      redraw();
    }).observe(canvas);
    window.matchMedia('(prefers-color-scheme: dark)').addEventListener('change', function () {
      recolour();
      draw(performance.now());
    });
  }

  // Each piece rises into place as it comes into view; pieces that arrive together follow one another. The list matches
  // the one in style.css that hides them until then.
  function reveal() {
    var pieces = document.querySelectorAll('.tools > *, .feature .copy > :not(ul), .feature li, .feature .shot, ' +
      '.copy.centered > *, .more > h2, .gallery > a, .more .grid > div, .privacy .card, .platform, main .note');
    var observer = new IntersectionObserver(function (entries) {
      var arriving = entries.filter(function (entry) { return entry.isIntersecting; }).map(function (entry) { return entry.target; });
      arriving.sort(function (a, b) { return a.compareDocumentPosition(b) & Node.DOCUMENT_POSITION_FOLLOWING ? -1 : 1; });
      arriving.forEach(function (piece, k) {
        observer.unobserve(piece);
        var delay = Math.min(k, 8) * 90;
        piece.style.setProperty('--delay', delay + 'ms');
        piece.classList.add('in');
        setTimeout(function () {
          piece.classList.add('done');
          piece.style.removeProperty('--delay');
        }, delay + 1400);
      });
    }, { rootMargin: '0px 0px -8% 0px' });
    pieces.forEach(function (piece) { observer.observe(piece); });
  }

  // The tool names as two endless rows: the names repeat until half the row is wider than the screen, then the whole
  // set once more, so moving by half (style.css) brings the row back to where it began. The copies are hidden from
  // screen readers.
  function marquees() {
    document.querySelectorAll('.marquee').forEach(function (box) {
      var list = box.querySelector('.chips'), names = Array.prototype.slice.call(list.children), span = 0;
      function fill() {
        if (box.clientWidth === span) return;
        while (list.children.length > names.length) list.removeChild(list.lastChild);
        box.classList.add('run');
        span = box.clientWidth;
        var once = list.getBoundingClientRect().width, copies = Math.max(1, Math.ceil(span / once));
        for (var k = 1; k < copies * 2; k++) {
          names.forEach(function (name) {
            var copy = name.cloneNode(true);
            copy.setAttribute('aria-hidden', 'true');
            list.appendChild(copy);
          });
        }
        list.style.setProperty('--duration', (once * copies / 30).toFixed(1) + 's');
      }
      fill();
      var pending = 0;
      window.addEventListener('resize', function () {
        clearTimeout(pending);
        pending = setTimeout(fill, 200);
      });
    });
  }

  // Tied to the scroll position: the bar frosts once the page moves under it; the headline drifts and fades, the sky
  // behind it moves slower than the page, the two panels behind the homepage fan out, each screenshot lags a little,
  // and the privacy promise lights up word by word.
  function scrolling(moving) {
    var nav = document.querySelector('.nav'), hero = document.querySelector('.hero');
    var copy = document.querySelector('.hero-copy'), sky = document.querySelector('.hero .starfield');
    var fan = document.querySelector('.fan'), shots = Array.prototype.slice.call(document.querySelectorAll('.feature .shot'));
    var statements = Array.prototype.slice.call(document.querySelectorAll('[data-scrub]'));
    if (moving) statements.forEach(split);
    var waiting = false;

    function update() {
      waiting = false;
      var y = window.scrollY, view = window.innerHeight;
      nav.classList.toggle('top', y < 4);
      if (!moving) return;

      // Measure everything first, then write, so the page is laid out once a frame.
      var opened = clamp(1 - (hero.getBoundingClientRect().top + fan.offsetTop - view * 0.12) / (view * 0.55));
      var lags = shots.map(function (shot) {
        var box = shot.parentElement.getBoundingClientRect();
        if (box.bottom < -view || box.top > view * 2) return null;
        return Math.max(-1, Math.min(1, (box.top + box.height / 2 - view / 2) / view)) * -40;
      });
      var lit = statements.map(function (statement) {
        var box = statement.getBoundingClientRect();
        return clamp((view * 0.88 - box.top) / (box.height + view * 0.36));
      });

      copy.style.setProperty('--lift', (Math.min(y, view) * 0.14).toFixed(1));
      copy.style.setProperty('--fade', clamp(1 - y / (view * 0.8)).toFixed(3));
      if (sky) sky.style.translate = '0 ' + (Math.min(y, view * 1.5) * 0.35).toFixed(1) + 'px';
      fan.style.setProperty('--fan', (0.3 + 0.7 * opened * opened * (3 - 2 * opened)).toFixed(3));
      shots.forEach(function (shot, k) { if (lags[k] !== null) shot.style.setProperty('--shift', lags[k].toFixed(1)); });
      statements.forEach(function (statement, k) { statement.style.setProperty('--p', lit[k].toFixed(3)); });
    }

    function request() {
      if (waiting) return;
      waiting = true;
      requestAnimationFrame(update);
    }
    window.addEventListener('scroll', request, { passive: true });
    window.addEventListener('resize', request);
    update();
  }

  // Each language's text in its own words (a Chinese character, with any punctuation after it, counts as one).
  function split(statement) {
    Array.prototype.forEach.call(statement.children, function (part) {
      var words = part.classList.contains('en')
        ? part.textContent.match(/\S+\s*/g)
        : part.textContent.match(/[A-Za-z0-9]+\s*|[^A-Za-z0-9\s][，。：；、！？）”]*\s*/g);
      if (!words) return;
      part.textContent = '';
      words.forEach(function (word, i) {
        var piece = document.createElement('span');
        piece.className = 'w';
        piece.style.setProperty('--i', i);
        piece.textContent = word;
        part.appendChild(piece);
      });
      part.style.setProperty('--n', words.length);
    });
  }
})();
