// The language switch (简体中文, 繁體中文 or English, kept for the next visit), the copy buttons for the Homebrew
// commands, and the page's motion: twinkling halftone skies, pieces rising into view and the effects tied to scrolling.
// Motion runs only when the page set html.motion (the system doesn't ask to reduce motion); otherwise the skies hold
// still.
(function () {
  var root = document.documentElement;
  var titles = {
    zh: 'Starmeter · 在菜单栏一眼看清 AI 编程工具用了多少',
    en: 'Starmeter · See how much your AI coding tools use, from the menu bar',
  };
  var descriptions = {
    zh: document.querySelector('meta[name="description"]').content,
    en: 'Starmeter shows the tokens and API-equivalent cost of Claude Code, Codex, Cursor, Gemini CLI and more, your subscription limits, API balances and active sessions, in the macOS menu bar and the Windows and Linux tray. Free, open source, and your data stays on your computer.',
  };
  var HAN = /[\u3400-\u9fff]/;

  // 繁體中文: the page is written in Simplified Chinese and English; Traditional is the Simplified text converted in
  // place (hant.js, the panel's own tables), with the original kept to go back to. The pictures come from
  // images/zh-Hant/ and the README links go to README.zh-Hant.md.
  var parts = null;
  // Collected the first time the page turns Traditional, when hant.js is there.
  function chineseParts() {
    if (parts) return parts;
    parts = [];
    var texts = new Set(), elements = new Set();
    document.querySelectorAll('.zh').forEach(function (element) {
      var walker = document.createTreeWalker(element, NodeFilter.SHOW_TEXT);
      for (var node = walker.nextNode(); node; node = walker.nextNode()) {
        if (texts.has(node) || !HAN.test(node.nodeValue)) continue;
        texts.add(node);
        parts.push({ node: node, text: node.nodeValue, hant: window.toHant(node.nodeValue) });
      }
      [element].concat([].slice.call(element.querySelectorAll('*'))).forEach(function (item) {
        if (elements.has(item)) return;
        elements.add(item);
        ['alt', 'src', 'srcset', 'href'].forEach(function (name) {
          var value = item.getAttribute(name), hant = value === null ? null : traditional(name, value);
          if (hant !== null && hant !== value) parts.push({ element: item, name: name, text: value, hant: hant });
        });
      });
    });
    return parts;
  }
  function traditional(name, value) {
    if (name === 'alt') return window.toHant(value);
    if (/^images\/(?!en\/|zh-Hant\/)/.test(value)) return value.replace(/^images\//, 'images/zh-Hant/');
    var readme = 'https://github.com/timecatbit/starmeter#';
    if (name === 'href' && value.indexOf(readme) === 0) return 'https://github.com/timecatbit/starmeter/blob/main/README.zh-Hant.md#' + window.toHant(value.slice(readme.length));
    return null;
  }
  function convert(on) {
    if (!on && !parts) return;
    chineseParts().forEach(function (part) {
      var value = on ? part.hant : part.text;
      if (part.node) {
        if (part.node.nodeValue !== value) part.node.nodeValue = value;
      } else if (part.element.getAttribute(part.name) !== value) {
        part.element.setAttribute(part.name, value);
      }
    });
  }
  // hant.js, fetched once it loads: the head already asked for it when the page opens in 繁體中文. A request that
  // failed is marked (here or by the head) and asked again the next time 繁體中文 is chosen (SITE-2).
  function withHant(done, failed) {
    if (window.toHant) return done();
    var script = document.getElementById('hant');
    if (script && script.hasAttribute('data-failed')) {
      script.remove();
      script = null;
    }
    if (!script) {
      script = document.createElement('script');
      script.src = 'hant.js';
      script.id = 'hant';
      script.addEventListener('error', function () { script.setAttribute('data-failed', ''); });
      document.head.appendChild(script);
    }
    script.addEventListener('load', done);
    script.addEventListener('error', failed);
  }

  function pressed(lang) {
    document.querySelectorAll('[data-lang]').forEach(function (button) {
      button.setAttribute('aria-pressed', String(button.getAttribute('data-lang') === lang));
    });
  }
  // The language last chosen: hant.js can arrive after another was picked, and then changes nothing (SITE-1).
  var chosen = null;
  function show(lang) {
    chosen = lang;
    pressed(lang);
    if (lang === 'zh-Hant' && !window.toHant) {
      return withHant(function () { if (chosen === lang) show(lang); }, function () { if (chosen === lang) show('zh'); });
    }
    root.classList.toggle('en', lang === 'en');
    root.classList.toggle('hant', lang === 'zh-Hant');
    root.lang = { zh: 'zh-CN', 'zh-Hant': 'zh-Hant', en: 'en' }[lang];
    convert(lang === 'zh-Hant');
    if (lang === 'zh-Hant') root.classList.add('hant-ready');
    document.title = lang === 'zh-Hant' ? window.toHant(titles.zh) : titles[lang];
    document.querySelector('meta[name="description"]').content = lang === 'zh-Hant' ? window.toHant(descriptions.zh) : descriptions[lang];
    // The text has its new shape now; the skies shade around it.
    document.dispatchEvent(new Event('starmeter-language'));
  }
  show(root.classList.contains('en') ? 'en' : root.classList.contains('hant') ? 'zh-Hant' : 'zh');

  document.querySelectorAll('[data-lang]').forEach(function (button) {
    button.addEventListener('click', function () {
      var lang = button.getAttribute('data-lang');
      show(lang);
      try { localStorage.setItem('starmeter-lang', lang); } catch (e) {}
      var url = new URL(location.href);
      url.searchParams.delete('lang');
      history.replaceState(null, '', url);
    });
  });

  document.querySelectorAll('[data-copy]').forEach(function (button) {
    var timer = 0;
    button.addEventListener('click', function () {
      navigator.clipboard.writeText(button.getAttribute('data-copy')).then(function () {
        button.classList.add('copied');
        clearTimeout(timer);
        timer = setTimeout(function () { button.classList.remove('copied'); }, 1800);
      });
    });
  });

  // MARK: Motion

  // What waits hidden to rise comes first, and the page is marked ready only once it will rise: if anything before
  // that fails, the head shows everything after a moment. The skies and the rest only decorate, so one that fails
  // leaves the others and the content as they are (SITE-3).
  var moving = root.classList.contains('motion');
  if (moving) reveal();
  root.classList.add('ready');
  function decorate(what) {
    try { what(); } catch (error) { console.error(error); }
  }
  document.querySelectorAll('canvas.starfield').forEach(function (canvas) { decorate(function () { starfield(canvas, moving); }); });
  if (moving) decorate(marquees);
  decorate(function () { scrolling(moving); });

  function clamp(value) { return Math.min(1, Math.max(0, value)); }

  // A halftone night sky like the logo's, on a square grid: about a third of the points hold a star, mostly faint and a
  // few brighter, each twinkling at its own slow pace; the light fades away from one edge of the section (data-sky)
  // and stays low behind the text named in data-avoid. Every few seconds the dots around one point swell into a
  // sparkle, the logo's own four-pointed star with its soft glow, and fade again. With reduced motion, one still frame.
  function starfield(canvas, moving) {
    var context = canvas.getContext('2d');
    if (!context) return;
    var PITCH = 18, LEVELS = 12, TAU = Math.PI * 2, FRAME = 1000 / 30;
    var fromBottom = canvas.getAttribute('data-sky') === 'bottom';
    var width = 0, height = 0, ratio = 1, columns = 0, rows = 0, quiet = [];
    var xs, ys, star, base, swing, pace, phase, glow, bright, level, order;
    var counts = new Int32Array(LEVELS), colours = [], halo = '', stencils = {}, sparkles = [];
    var begun = performance.now(), nextSparkle = 0, previous = 0, frame = 0, inView = false;

    // How much light the sky gives at (x, y): full along the bright edge's middle, less at its corners, none 85% away.
    function sky(x, y) {
      var away = (fromBottom ? height - y : y) / height, across = (x - width / 2) / (width / 2);
      return Math.pow(Math.max(0, 1 - away / 0.85), 1.2) * (0.55 + 0.45 * Math.exp(-across * across / 0.3));
    }

    // The text to keep clear of, as boxes relative to the canvas (which sits at the top of its section).
    function textBoxes() {
      var selector = canvas.getAttribute('data-avoid'), origin = canvas.parentElement.getBoundingClientRect();
      return selector ? Array.prototype.map.call(document.querySelectorAll(selector), function (element) {
        var r = element.getBoundingClientRect();
        return { left: r.left - origin.left, right: r.right - origin.left, top: r.top - origin.top, bottom: r.bottom - origin.top, empty: !r.width };
      }).filter(function (box) { return !box.empty; }) : [];
    }

    // How far (x, y) is from the nearest of the text.
    function distance(x, y) {
      return quiet.reduce(function (nearest, box) {
        var dx = Math.max(box.left - x, 0, x - box.right), dy = Math.max(box.top - y, 0, y - box.bottom);
        return Math.min(nearest, Math.sqrt(dx * dx + dy * dy));
      }, Infinity);
    }

    // Behind the text the stars dim to a fifth, easing back over 70 points around it.
    function calm(x, y) { return 0.2 + 0.8 * clamp(distance(x, y) / 70); }

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
      halo = to.slice(0, 3).join(',');
    }

    // Where the stars are and how each twinkles; drawn once per size.
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
      xs = new Float32Array(count); ys = new Float32Array(count); star = new Float32Array(count); base = new Float32Array(count);
      swing = new Float32Array(count); pace = new Float32Array(count); phase = new Float32Array(count);
      glow = new Float32Array(count); bright = new Float32Array(count);
      level = new Uint8Array(count); order = new Int32Array(count);
      for (var r = 0; r < rows; r++) {
        for (var c = 0; c < columns; c++) {
          var i = r * columns + c;
          xs[i] = left + c * PITCH;
          ys[i] = PITCH / 2 + r * PITCH;
          // A third of the points hold a star: mostly faint, now and then a bright one, like distant stars.
          star[i] = Math.random() < 0.34 ? 0.12 + 0.5 * Math.pow(Math.random(), 2.2) : 0;
          // Half of them twinkle deeply, fading almost out; the rest only flicker.
          swing[i] = Math.random() < 0.5 ? 0.85 : 0.3;
          pace[i] = TAU / (2.5 + Math.random() * 5.5);
          phase[i] = Math.random() * TAU;
        }
      }
      shade();
      // A few sparkles already on their way, so the sky isn't empty when it appears; held at their brightest if still.
      var now = performance.now();
      sparkles = [];
      for (var k = 0; k < Math.ceil(wanted() * 0.6); k++) {
        var spark = place(now);
        if (spark) {
          spark.born = now - spark.life * 1000 * (moving ? Math.random() * 0.8 : 0.5);
          sparkles.push(spark);
        }
      }
      return true;
    }

    // Each star's light where it stands: the sky's, dimmed behind the text.
    function shade() {
      quiet = textBoxes();
      for (var i = 0; i < xs.length; i++) base[i] = star[i] ? star[i] * sky(xs[i], ys[i]) * calm(xs[i], ys[i]) : 0;
    }

    function wanted() { return Math.max(2, Math.min(5, Math.round(width * height / 300000))); }

    // Somewhere in the brighter part of the sky, clear of the text and of the other sparkles.
    function place(now) {
      for (var tries = 0; tries < 60; tries++) {
        var c = Math.floor(Math.random() * columns), r = Math.floor(Math.random() * rows), i = r * columns + c;
        var light = sky(xs[i], ys[i]);
        if (light < 0.3 || Math.random() > light * light) continue;
        // The tips are faint, so they may reach a little way towards the text.
        var reach = 3 + Math.floor(Math.random() * Math.random() * 4);
        if (distance(xs[i], ys[i]) < reach * PITCH * 0.8) continue;
        // Whole, not cut off by the section's edge.
        if (r < reach || c < reach || r >= rows - reach - 1 || c >= columns - reach - 1) continue;
        if (sparkles.some(function (o) {
          return Math.abs(o.c - c) < o.reach + reach + 3 && Math.abs(o.r - r) < o.reach + reach + 3;
        })) continue;
        return { c: c, r: r, reach: reach, born: now, life: 5 + Math.random() * 4, peak: 0.85 + Math.random() * 0.15 };
      }
      return null;
    }

    function draw(now) {
      var t = now / 1000, count = columns * rows, k, i;
      glow.fill(0);
      context.setTransform(ratio, 0, 0, ratio, 0, 0);
      context.clearRect(0, 0, width, height);
      context.globalAlpha = moving ? clamp((now - begun) / 1800) : 1;

      // The sparkles: the dots under each swell into the logo's star, over a soft glow like the logo's.
      sparkles = sparkles.filter(function (spark) {
        var age = (now - spark.born) / 1000 / spark.life;
        if (age >= 1) return false;
        var strength = Math.pow(Math.sin(Math.PI * Math.max(0, age)), 2) * spark.peak;
        var shape = stencil(spark.reach), side = spark.reach * 2 + 1;
        for (var dr = -spark.reach; dr <= spark.reach; dr++) {
          var row = spark.r + dr;
          if (row < 0 || row >= rows) continue;
          for (var dc = -spark.reach; dc <= spark.reach; dc++) {
            var column = spark.c + dc;
            if (column < 0 || column >= columns) continue;
            var value = shape[(dr + spark.reach) * side + dc + spark.reach] * strength, index = row * columns + column;
            if (value > glow[index]) glow[index] = value;
          }
        }
        var x = xs[spark.r * columns + spark.c], y = ys[spark.r * columns + spark.c], radius = spark.reach * PITCH * 1.1;
        var light = context.createRadialGradient(x, y, 0, x, y, radius);
        light.addColorStop(0, 'rgba(' + halo + ',' + (0.22 * strength).toFixed(3) + ')');
        light.addColorStop(1, 'rgba(' + halo + ',0)');
        context.fillStyle = light;
        context.fillRect(x - radius, y - radius, radius * 2, radius * 2);
        return true;
      });
      if (moving && sparkles.length < wanted() && now >= nextSparkle) {
        var spark = place(now);
        if (spark) sparkles.push(spark);
        nextSparkle = now + 800 + Math.random() * 2600;
      }

      // Each dot's brightness, then the dots sorted by brightness so each shade is one path.
      counts.fill(0);
      for (i = 0; i < count; i++) {
        var b = base[i] * (moving ? 1 + swing[i] * Math.sin(pace[i] * t + phase[i]) : 1);
        if (glow[i]) b = 1 - (1 - b) * (1 - glow[i]);
        if (b < 0.02) { level[i] = 255; continue; }
        b = Math.min(1, b);
        bright[i] = b;
        level[i] = Math.min(LEVELS - 1, Math.floor(b * LEVELS));
        counts[level[i]]++;
      }
      var starts = [], total = 0;
      for (k = 0; k < LEVELS; k++) { starts.push(total); total += counts[k]; }
      var cursor = starts.slice();
      for (i = 0; i < count; i++) if (level[i] !== 255) order[cursor[level[i]]++] = i;

      for (k = 0; k < LEVELS; k++) {
        if (!counts[k]) continue;
        context.fillStyle = colours[k];
        context.beginPath();
        for (var n = starts[k]; n < starts[k] + counts[k]; n++) {
          i = order[n];
          // Halftone: the brighter the dot, the larger; a faint star is a point, a sparkle's heart nearly fills its cell.
          var radius = 0.55 + 3.7 * bright[i];
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
      if (layout()) draw(performance.now());
    }).observe(canvas);
    // The other language's text has another shape.
    document.addEventListener('starmeter-language', function () {
      shade();
      if (!moving) draw(performance.now());
    });
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
