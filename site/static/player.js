// The site's player of Glut's recordings (`cargo run -p xtask -- record <scene>`): a drawn terminal whose rows are
// text in spans, filled frame by frame from the recording's JSON. Text, not video: sharp at any size, light, and
// selectable. It is fetched as it comes near the screen, plays while on screen and pauses off it; with reduced motion
// it shows the last screen and waits for a click on play. Scenes behind tabs ([data-scenes]) share one player.
// An image Glut had the terminal draw (the recording's "images" and a frame's "pics") is laid over its cells.
(() => {
  "use strict";

  const still = window.matchMedia("(prefers-reduced-motion: reduce)").matches;

  // xterm's 16 colours in Glut's own tones, then its 6×6×6 cube and the greys.
  const BASE = [
    "#1a1a1a", "#e06c6c", "#8cc88c", "#e8b04a", "#8aadf4", "#c6a0f6", "#8bd5ca", "#c8c3ba",
    "#5a5552", "#f08080", "#a6da95", "#ffd36b", "#9cc8e8", "#f5bde6", "#9ee0d6", "#ece6dc",
  ];
  const indexed = (n) => {
    if (n < 16) return BASE[n];
    if (n >= 232) {
      const grey = 8 + (n - 232) * 10;
      return `rgb(${grey},${grey},${grey})`;
    }
    const m = n - 16;
    const step = (v) => (v === 0 ? 0 : 55 + v * 40);
    return `rgb(${step(Math.floor(m / 36))},${step(Math.floor(m / 6) % 6)},${step(m % 6)})`;
  };
  // Only a colour of the recorder's two forms reaches a style attribute; anything else is the player's own.
  const colour = (value) => {
    if (/^#[0-9a-f]{6}$/i.test(value)) return value;
    if (/^i\d{1,3}$/.test(value)) return indexed(Number(value.slice(1)) & 255);
    return "";
  };

  const escape = (text) => text.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;");

  // A character beyond ASCII may come from another font, wider or narrower than a cell: each is set in a box of
  // exactly one cell, two where the recorder marked it wide (U+E000 behind it), so the rows keep the terminal's
  // columns and Glimm its shape.
  const WIDE = "\uE000";
  // The block characters Glimm is drawn with, as the quadrants they fill (top left 1, top right 2, bottom left 4,
  // bottom right 8): drawn here as exact areas of the cell in its ink, not as a font's glyph with gaps around it.
  const QUADRANTS = {
    "▘": 1, "▝": 2, "▀": 3, "▖": 4, "▌": 5, "▞": 6, "▛": 7,
    "▗": 8, "▚": 9, "▐": 10, "▜": 11, "▄": 12, "▙": 13, "▟": 14, "█": 15,
  };
  const AREAS = [[1, "0 0"], [2, "100% 0"], [4, "0 100%"], [8, "100% 100%"]];
  const quadrant = (mask) => {
    const lit = AREAS.filter(([bit]) => mask & bit);
    const images = lit.map(() => "linear-gradient(currentColor,currentColor)").join(",");
    const places = lit.map(([, place]) => place).join(",");
    return `<b class="q" style="background-image:${images};background-position:${places}"></b>`;
  };
  const cells = (text) => {
    const chars = [...text];
    let html = "";
    for (let at = 0; at < chars.length; at += 1) {
      const c = chars[at];
      if (c === WIDE) continue;
      if (c.charCodeAt(0) < 0x80) html += escape(c);
      else if (QUADRANTS[c]) html += quadrant(QUADRANTS[c]);
      else html += `<b class="${chars[at + 1] === WIDE ? "c2" : "c"}">${escape(c)}</b>`;
    }
    return html;
  };

  /** One style as inline CSS: colours, then bold, dim, italic, underline, inverse. */
  function css([fg, bg, flags]) {
    let ink = colour(fg);
    let ground = colour(bg);
    if (flags & 16) [ink, ground] = [ground || "var(--ground)", ink || "var(--ink)"];
    const parts = [];
    if (ink) parts.push(`color:${ink}`);
    if (ground) parts.push(`background:${ground}`);
    if (flags & 1) parts.push("font-weight:700");
    if (flags & 2) parts.push("opacity:.6");
    if (flags & 4) parts.push("font-style:italic");
    if (flags & 8) parts.push("text-decoration:underline");
    return parts.join(";");
  }

  async function player(figure) {
    const screen = figure.querySelector(".screen");
    const button = figure.querySelector("button.play");
    const bar = figure.querySelector(".progress i");
    let film;
    try {
      const answer = await fetch(figure.dataset.src);
      film = await answer.json();
    } catch {
      figure.classList.add("missing");
      return;
    }
    const styles = film.styles.map(css);
    const rows = [];
    screen.textContent = "";
    for (let at = 0; at < film.rows; at += 1) {
      const row = document.createElement("div");
      row.className = "row";
      screen.append(row);
      rows.push(row);
    }
    // Only base64 of the recorder's PNGs becomes an image's source.
    const images = (film.images || []).map((data) =>
      typeof data === "string" && /^[A-Za-z0-9+/]+=*$/.test(data) ? `data:image/png;base64,${data}` : "");
    const pictures = document.createElement("div");
    pictures.className = "pics";
    screen.append(pictures);
    // A picture at a cell, in cells: the font's own width (ch) and the rows' height, so it scales with the text.
    const picture = ([image, x, y, cols, height]) => {
      const source = images[image];
      if (!source) return null;
      const img = document.createElement("img");
      img.alt = "";
      img.src = source;
      img.style.cssText = `left:${Number(x)}ch;top:${Number(y) * 1.22}em;`
        + `width:${Number(cols)}ch;height:${Number(height) * 1.22}em`;
      return img;
    };
    const draw = (frame) => {
      if (Array.isArray(frame.pics)) pictures.replaceChildren(...frame.pics.map(picture).filter(Boolean));
      for (const [at, runs] of Object.entries(frame.rows)) {
        rows[Number(at)].innerHTML = runs
          .map(([text, style]) => {
            const look = styles[style];
            return look ? `<span style="${look}">${cells(text)}</span>` : cells(text);
          })
          .join("");
      }
    };
    const length = film.frames[film.frames.length - 1].t || 1;

    // The rows' size follows the figure's width, by the width of a cell of the font as drawn here.
    const probe = document.createElement("span");
    probe.textContent = "0".repeat(100);
    probe.style.cssText = "position:absolute;visibility:hidden;font-size:100px";
    screen.append(probe);
    const ratio = probe.getBoundingClientRect().width / 100 / 100 || 0.6;
    probe.remove();
    const fit = () => {
      const width = screen.clientWidth;
      screen.style.fontSize = `${Math.max(6, width / (film.cols * ratio))}px`;
    };
    new ResizeObserver(fit).observe(screen);
    fit();

    let next = 0;
    let clock = 0;
    let last = 0;
    let playing = false;
    const show = (until) => {
      while (next < film.frames.length && film.frames[next].t <= until) {
        draw(film.frames[next]);
        next += 1;
      }
      bar.style.width = `${Math.min(100, (until / length) * 100)}%`;
    };
    const restart = () => {
      next = 0;
      clock = 0;
      rows.forEach((row) => (row.textContent = ""));
      pictures.textContent = "";
      // The first screen at once: no empty frame between the end and the new start.
      show(0);
    };
    const tick = (now) => {
      // A figure taken off the page (another scene chosen) stops for good.
      if (!playing || !figure.isConnected) return;
      clock += Math.min(100, now - last);
      last = now;
      show(clock);
      if (clock >= length) {
        // The last screen stays a moment (it is in the recording's end), then the scene starts over.
        restart();
      }
      requestAnimationFrame(tick);
    };
    const play = () => {
      if (playing) return;
      playing = true;
      last = performance.now();
      figure.classList.add("playing");
      button.setAttribute("aria-label", button.dataset.pause);
      requestAnimationFrame(tick);
    };
    const pause = () => {
      playing = false;
      figure.classList.remove("playing");
      button.setAttribute("aria-label", button.dataset.play);
    };
    // A pause the reader chose holds, also when the scene comes back on screen; a play clears it.
    button.addEventListener("click", () => {
      if (playing) {
        pause();
        figure.dataset.chosen = "1";
      } else {
        delete figure.dataset.chosen;
        play();
      }
    });

    if (still) {
      // The whole scene's last screen, to read at rest.
      show(length);
      next = film.frames.length;
      clock = length;
      return;
    }
    show(0);
    new IntersectionObserver(
      (entries) => entries.forEach((entry) => {
        // A scene the reader paused stays paused; one that left the screen pauses.
        if (entry.isIntersecting && !figure.dataset.chosen) play();
        else if (!entry.isIntersecting) pause();
      }),
      { threshold: 0.35 },
    ).observe(figure);
  }

  // A recording is fetched only as it comes near the screen: a page of several loads what is read.
  const near = new IntersectionObserver(
    (entries) => entries.forEach((entry) => {
      if (!entry.isIntersecting) return;
      near.unobserve(entry.target);
      player(entry.target);
    }),
    { rootMargin: "100% 0px" },
  );
  const pristine = new WeakMap();
  document.querySelectorAll("figure.player").forEach((figure) => {
    pristine.set(figure, figure.cloneNode(true));
    near.observe(figure);
  });

  // Scenes behind tabs: one player, the chosen scene in it. Arrow keys move along the tabs, as a tab list's do.
  document.querySelectorAll("[data-scenes]").forEach((box) => {
    const tabs = [...box.querySelectorAll("[data-scene]")];
    const caption = box.querySelector("[data-caption]");
    const choose = (tab) => {
      const old = box.querySelector("figure.player");
      const fresh = (pristine.get(old) || old).cloneNode(true);
      fresh.dataset.src = tab.dataset.scene;
      const title = fresh.querySelector(".bar span");
      if (title) title.textContent = tab.dataset.bar || "";
      near.unobserve(old);
      old.replaceWith(fresh);
      pristine.set(fresh, fresh.cloneNode(true));
      player(fresh);
      tabs.forEach((other) => {
        other.setAttribute("aria-selected", String(other === tab));
        other.tabIndex = other === tab ? 0 : -1;
      });
      if (caption) caption.textContent = tab.dataset.about || "";
    };
    tabs.forEach((tab, at) => {
      tab.addEventListener("click", () => choose(tab));
      tab.addEventListener("keydown", (event) => {
        const step = { ArrowRight: 1, ArrowLeft: -1 }[event.key];
        if (!step) return;
        event.preventDefault();
        const next = tabs[(at + step + tabs.length) % tabs.length];
        next.focus();
        choose(next);
      });
    });
  });
})();
