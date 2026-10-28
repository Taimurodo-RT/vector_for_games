// Sidebar with the pages and the sections of the current one, plus the light/dark switch.
// Plain script, so the pages also work when opened straight from disk.

(function () {
  const pages = [
    ["index.html", "Начало"],
    ["tools.html", "Инструменты и холст"],
    ["properties.html", "Панель свойств"],
    ["animation.html", "Анимация"],
    ["machine.html", "Машина состояний"],
    ["bones.html", "Кости и IK"],
    ["physics.html", "Физика"],
    ["orbit.html", "2.5D и 3D"],
    ["script.html", "Скрипт сцены"],
    ["export.html", "Экспорт и программы"],
    ["web.html", "SVG и Web assets"],
    ["runtime.html", "Motif в игре"],
  ];

  try {
    const saved = localStorage.getItem("motif-docs-theme");
    if (saved) document.documentElement.dataset.theme = saved;
  } catch (e) {}

  const here = location.pathname.split("/").pop() || "index.html";
  const side = document.createElement("nav");
  side.className = "sidebar";
  let html = '<a class="brand" href="index.html"><span class="logo">M</span>Motif</a><ul class="nav">';
  for (const [file, title] of pages) {
    const current = file === here;
    html += `<li><a href="${file}"${current ? ' class="current"' : ""}>${title}</a>`;
    if (current) {
      const heads = [...document.querySelectorAll("main h2[id]")];
      if (heads.length) {
        html += '<ul class="nav sub">' + heads.map(h => `<li><a href="#${h.id}">${h.textContent}</a></li>`).join("") + "</ul>";
      }
    }
    html += "</li>";
  }
  html += '</ul><button class="theme-toggle" type="button">Светлая / тёмная тема</button>';
  side.innerHTML = html;
  document.querySelector(".layout").prepend(side);

  side.querySelector(".theme-toggle").addEventListener("click", () => {
    const root = document.documentElement;
    const light = root.dataset.theme !== "light";
    root.dataset.theme = light ? "light" : "dark";
    try { localStorage.setItem("motif-docs-theme", root.dataset.theme); } catch (e) {}
  });

  // Previous / next page links at the bottom.
  const i = pages.findIndex(p => p[0] === here);
  const content = document.querySelector(".content");
  if (i >= 0 && content) {
    const pager = document.createElement("div");
    pager.className = "pager";
    const prev = pages[i - 1], next = pages[i + 1];
    pager.innerHTML = (prev ? `<a href="${prev[0]}">← ${prev[1]}</a>` : "<span></span>") + (next ? `<a href="${next[0]}">${next[1]} →</a>` : "<span></span>");
    content.append(pager);
  }
})();
