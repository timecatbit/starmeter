// The language switch (the choice is kept for the next visit) and the copy button for the Homebrew command.
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
})();
