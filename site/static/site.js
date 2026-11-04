// Glut's site: Glimm drawn from the terminal's own frames (glimm.js, made at build time), the start sequence, the
// reveals on scroll, the sticky steps, the scroll readout and the copy buttons. No library. Everything here only adds
// motion: without it, or with reduced motion asked for, every word of the page is there and readable.
(() => {
  "use strict";

  const still = window.matchMedia("(prefers-reduced-motion: reduce)").matches;
  // Storage may be shut (cookies blocked, some private modes): then the start simply runs again.
  const booted = () => {
    try {
      return sessionStorage.getItem("glut-booted") === "1";
    } catch {
      return false;
    }
  };
  const markBooted = () => {
    try {
      sessionStorage.setItem("glut-booted", "1");
    } catch {
      // Nothing kept: the start shows again on the next page.
    }
  };
  // How long Glimm moves on its own before it holds still (WCAG 2.2.2); a click on it lets it move again.
  const MOVES_FOR = 8000;

  // Glimm on a canvas: one pixel of the terminal's raster is one unit wide and two tall, as on the screen.
  function glimm(canvas) {
    const data = window.GLIMM;
    if (!data || !canvas.getContext) return;
    const unit = Number(canvas.dataset.unit || 6);
    const ratio = window.devicePixelRatio || 1;
    canvas.width = data.w * unit * ratio;
    canvas.height = data.h * unit * 2 * ratio;
    canvas.style.width = `${data.w * unit}px`;
    canvas.style.height = `${data.h * unit * 2}px`;
    const ctx = canvas.getContext("2d");
    ctx.scale(ratio, ratio);
    const draw = (name) => {
      const rows = data.frames[name];
      if (!rows) return;
      ctx.clearRect(0, 0, data.w * unit, data.h * unit * 2);
      rows.forEach((row, y) => {
        [...row].forEach((letter, x) => {
          const colour = data.colours[letter];
          if (!colour) return;
          ctx.fillStyle = colour;
          ctx.fillRect(x * unit, y * unit * 2, unit, unit * 2);
        });
      });
    };
    const work = ["work0", "work1", "work2", "work3", "work4", "work5", "work4", "work3", "work2", "work1"];
    const rest = ["rest0", "rest1", "rest2", "rest3"];
    let mood = canvas.dataset.mood || "rest";
    let step = 0;
    let last = 0;
    let until = performance.now() + MOVES_FOR;
    const tick = (now) => {
      // A canvas gone from the page (the start's) stops drawing; one that has moved long enough rests still.
      if (!canvas.isConnected) return;
      if (now > until) {
        draw("rest0");
        running = false;
        return;
      }
      const every = mood === "work" ? 110 : 650;
      if (now - last >= every) {
        last = now;
        step += 1;
        if (mood === "work") draw(work[step % work.length]);
        else if (mood === "done") draw(`done${step % 2}`);
        else draw(step % 9 === 0 ? "restblink" : rest[step % rest.length]);
      }
      requestAnimationFrame(tick);
    };
    let running = false;
    const start = () => {
      until = performance.now() + MOVES_FOR;
      if (still || running) return;
      running = true;
      requestAnimationFrame(tick);
    };
    draw(mood === "work" ? "work0" : "rest0");
    canvas.addEventListener("click", start);
    start();
    return { set(next) { mood = next; } };
  }

  const flames = [...document.querySelectorAll("canvas.glimm")].map(glimm);

  // The start: a real Glut's first lines, counted up to 100 %, once per visit. A key or a click ends it at once.
  const boot = document.querySelector(".boot");
  if (boot && !still && !booted()) {
    markBooted();
    boot.hidden = false;
    document.body.classList.add("booting");
    const lines = [...boot.querySelectorAll("[data-line]")];
    const count = boot.querySelector(".boot-count");
    const started = performance.now();
    const length = 2200;
    let done = false;
    const end = () => {
      if (done) return;
      done = true;
      boot.classList.add("gone");
      document.body.classList.remove("booting");
      setTimeout(() => boot.remove(), 600);
      flames.forEach((flame) => flame && flame.set("rest"));
    };
    const run = (now) => {
      const part = Math.min(1, (now - started) / length);
      if (count) count.textContent = `${String(Math.round(part * 100)).padStart(3, "0")}%`;
      lines.forEach((line, at) => line.classList.toggle("on", part >= (at + 1) / (lines.length + 1)));
      if (part < 1 && !done) requestAnimationFrame(run);
      else setTimeout(end, 350);
    };
    requestAnimationFrame(run);
    addEventListener("keydown", end, { once: true });
    boot.addEventListener("click", end);
  } else if (boot) {
    boot.remove();
  }

  // Sections and items come in as they reach the screen.
  const reveal = new IntersectionObserver(
    (entries) => entries.forEach((entry) => {
      if (!entry.isIntersecting) return;
      entry.target.classList.add("in");
      reveal.unobserve(entry.target);
    }),
    { rootMargin: "0px 0px -12% 0px" },
  );
  document
    .querySelectorAll("[data-reveal]")
    .forEach((node) => (still ? node.classList.add("in") : reveal.observe(node)));

  // The manifesto lights word by word with the scroll where the browser ties animations to it (CSS); elsewhere it is
  // lit as a whole when it comes in.
  document.querySelectorAll("[data-words]").forEach((node) => {
    const words = node.textContent.trim().split(/\s+/);
    node.textContent = "";
    words.forEach((word, at) => {
      const span = document.createElement("span");
      span.textContent = `${word} `;
      span.style.setProperty("--at", at / words.length);
      node.append(span);
    });
  });

  // The steps: the panel beside them shows the step being read.
  document.querySelectorAll("[data-steps]").forEach((steps) => {
    const panels = [...steps.querySelectorAll("[data-panel]")];
    const watch = new IntersectionObserver(
      (entries) => entries.forEach((entry) => {
        if (!entry.isIntersecting) return;
        const name = entry.target.dataset.step;
        panels.forEach((panel) => panel.classList.toggle("on", panel.dataset.panel === name));
        steps.querySelectorAll("[data-step]").forEach((step) => step.classList.toggle("on", step === entry.target));
      }),
      { rootMargin: "-45% 0px -45% 0px" },
    );
    steps.querySelectorAll("[data-step]").forEach((step) => watch.observe(step));
  });

  // The readout: which numbered section is read, and how far down the page.
  const readout = document.querySelector(".readout");
  const sections = [...document.querySelectorAll("[data-section]")];
  if (readout && sections.length) {
    const update = () => {
      const middle = innerHeight / 2;
      let current = sections[0];
      sections.forEach((section) => {
        if (section.getBoundingClientRect().top < middle) current = section;
      });
      const total = document.documentElement.scrollHeight - innerHeight;
      const part = total > 0 ? Math.round((scrollY / total) * 100) : 0;
      const of = String(sections.length).padStart(2, "0");
      readout.textContent = `[${current.dataset.section}/${of}] ${String(part).padStart(3, "0")}%`;
      document.documentElement.style.setProperty("--read", part / 100);
    };
    addEventListener("scroll", update, { passive: true });
    update();
  }

  // Copy buttons: the command beside them onto the clipboard.
  document.querySelectorAll("[data-copy]").forEach((button) => {
    const label = button.textContent;
    let back;
    button.addEventListener("click", async () => {
      const target = document.getElementById(button.dataset.copy);
      if (!target) return;
      try {
        await navigator.clipboard.writeText(target.textContent.trim());
        button.textContent = button.dataset.done || "copied";
      } catch {
        button.textContent = button.dataset.failed || "select it";
      }
      clearTimeout(back);
      back = setTimeout(() => (button.textContent = label), 1600);
    });
  });

  // Only now that every part is wired may the reveals hide what has not come in yet.
  document.documentElement.classList.add("js");
})();
