import type { CharacterKind, Look } from "./api";

const INK = "#2b2a33";

export const CHARACTERS: { kind: CharacterKind; name: string }[] = [
  { kind: "pixel", name: "小訊" },
  { kind: "mic", name: "小麥" },
  { kind: "blob", name: "小圓" },
  { kind: "owl", name: "小鴞" },
  { kind: "cloud", name: "小雲" },
  { kind: "cat", name: "小喵" },
  { kind: "sprout", name: "小芽" },
  { kind: "penguin", name: "小企" },
];

function face(s: Look, x1: number, x2: number, y: number, mx: number, my: number, ink = INK): string {
  let eyes: string;
  if (s === "pause") {
    eyes = `<path d="M${x1 - 5} ${y}h10M${x2 - 5} ${y}h10" stroke="${ink}" stroke-width="3" stroke-linecap="round"/>`;
  } else if (s === "think") {
    eyes = `<circle cx="${x1 + 2}" cy="${y - 3}" r="3.6" fill="${ink}"/><circle cx="${x2 + 2}" cy="${y - 3}" r="3.6" fill="${ink}"/>`;
  } else if (s === "listen") {
    eyes =
      `<circle cx="${x1}" cy="${y}" r="5" fill="${ink}"/><circle cx="${x2}" cy="${y}" r="5" fill="${ink}"/>` +
      `<circle cx="${x1 + 1.6}" cy="${y - 1.6}" r="1.5" fill="#fff"/><circle cx="${x2 + 1.6}" cy="${y - 1.6}" r="1.5" fill="#fff"/>`;
  } else {
    eyes = `<circle class="blink" cx="${x1}" cy="${y}" r="3.8" fill="${ink}"/><circle class="blink" cx="${x2}" cy="${y}" r="3.8" fill="${ink}"/>`;
  }
  let mouth: string;
  if (s === "speak") {
    mouth = `<ellipse class="talk" cx="${mx}" cy="${my + 1}" rx="4.5" ry="4" fill="${ink}"/>`;
  } else if (s === "think") {
    mouth = `<path d="M${mx - 3} ${my}h6" stroke="${ink}" stroke-width="2.6" stroke-linecap="round"/>`;
  } else if (s === "listen") {
    mouth = `<circle cx="${mx}" cy="${my}" r="2.2" fill="${ink}"/>`;
  } else if (s === "pause") {
    mouth = "";
  } else {
    mouth = `<path d="M${mx - 4} ${my - 1}q4 4 8 0" fill="none" stroke="${ink}" stroke-width="2.6" stroke-linecap="round"/>`;
  }
  return eyes + mouth;
}

/** 狀態的附加效果：聲波、思考的點、音量條、z。都放在身體外圍，四個造型共用。 */
function fx(s: Look, c = "#6f9bff"): string {
  if (s === "listen") {
    return (
      `<g class="pulse" fill="none" stroke="${c}" stroke-width="3" stroke-linecap="round">` +
      `<path d="M12 50q-6 10 0 20"/><path d="M108 50q6 10 0 20"/></g>`
    );
  }
  if (s === "think") {
    return `<g fill="${c}"><circle class="d1" cx="86" cy="10" r="3.4"/><circle class="d2" cx="97" cy="10" r="3.4"/><circle class="d3" cx="108" cy="10" r="3.4"/></g>`;
  }
  if (s === "speak") {
    return (
      `<g class="pulse" fill="${c}"><rect x="6" y="52" width="4" height="16" rx="2"/><rect x="13" y="46" width="4" height="28" rx="2"/>` +
      `<rect x="103" y="46" width="4" height="28" rx="2"/><rect x="110" y="52" width="4" height="16" rx="2"/></g>`
    );
  }
  if (s === "pause") {
    return `<text x="92" y="24" font-size="15" font-weight="700" fill="#9a9aa8">z</text><text x="102" y="14" font-size="11" font-weight="700" fill="#9a9aa8">z</text>`;
  }
  return "";
}

function mic(s: Look): string {
  const o = s === "pause" ? "#c9b58d" : "#e0c690";
  return (
    `<rect x="50" y="88" width="20" height="12" rx="4" fill="#8d7a5c"/>` +
    `<rect x="38" y="99" width="44" height="9" rx="4.5" fill="#8d7a5c"/>` +
    `<rect x="30" y="16" width="60" height="76" rx="30" fill="#f6ead0" stroke="${o}" stroke-width="3"/>` +
    `<path d="M42 26h36M37 34h46" stroke="${o}" stroke-width="3" stroke-linecap="round"/>` +
    face(s, 48, 72, 56, 60, 68) +
    `<circle cx="39" cy="66" r="4" fill="#f4b8a8" opacity=".8"/><circle cx="81" cy="66" r="4" fill="#f4b8a8" opacity=".8"/>` +
    fx(s)
  );
}

function blob(s: Look): string {
  const body = s === "pause" ? "#a9b8b5" : "#7fd3c4";
  return (
    `<path d="M60 22c28 0 40 20 40 44 0 26-16 38-40 38S20 92 20 66c0-24 12-44 40-44z" fill="${body}"/>` +
    `<ellipse cx="46" cy="36" rx="11" ry="6" fill="#fff" opacity=".35"/>` +
    face(s, 47, 73, 62, 60, 75) +
    fx(s, "#3f8f81")
  );
}

function owl(s: Look): string {
  const body = s === "pause" ? "#c7b8a6" : "#e7b36f";
  const ring = "#fdf3df";
  return (
    `<path d="M32 30l4-14 14 10h20l14-10 4 14c6 8 8 18 8 30 0 30-16 46-38 46S22 90 22 60c0-12 4-22 10-30z" fill="${body}"/>` +
    `<ellipse cx="60" cy="86" rx="20" ry="17" fill="#f6dcb1"/>` +
    `<circle cx="45" cy="52" r="14" fill="${ring}"/><circle cx="75" cy="52" r="14" fill="${ring}"/>` +
    face(s, 45, 75, 52, 60, 66, "#3a2f28") +
    fx(s, "#c17f2e")
  );
}

function pixel(s: Look): string {
  const u = 5;
  const r = (x: number, y: number, w: number, h: number, c: string) =>
    `<rect x="${x * u}" y="${y * u}" width="${w * u}" height="${h * u}" fill="${c}"/>`;
  const paused = s === "pause";
  const [shell, dark, cheek] = paused ? ["#b9c3cf", "#5c6675", "#d7b7ae"] : ["#a9c7ff", "#2f4f8f", "#ffb7a8"];
  let out =
    r(11, 2, 2, 3, dark) + r(10, 1, 4, 2, paused ? "#c9c9c9" : "#ffd166") +
    r(6, 5, 12, 1, dark) + r(5, 6, 14, 1, dark) + r(4, 7, 16, 9, dark) + r(5, 16, 14, 1, dark) +
    r(6, 6, 12, 1, shell) + r(5, 7, 14, 9, shell) +
    r(7, 8, 10, 7, "#f4f7ff") +
    r(7, 17, 10, 3, dark) + r(8, 17, 8, 2, shell) + r(5, 18, 2, 2, dark) + r(17, 18, 2, 2, dark) +
    r(8, 20, 3, 2, dark) + r(13, 20, 3, 2, dark) +
    r(8, 13, 1, 1, cheek) + r(15, 13, 1, 1, cheek);
  if (s === "pause") {
    out += r(8, 11, 3, 1, INK) + r(13, 11, 3, 1, INK);
  } else if (s === "think") {
    out += r(10, 9, 1, 2, INK) + r(15, 9, 1, 2, INK) + r(11, 13, 2, 1, INK);
  } else if (s === "listen") {
    out += r(9, 10, 2, 2, INK) + r(13, 10, 2, 2, INK) + r(11, 13, 2, 1, INK);
  } else if (s === "speak") {
    out += r(9, 10, 1, 2, INK) + r(14, 10, 1, 2, INK) + `<g class="talk">${r(11, 13, 2, 2, INK)}</g>`;
  } else {
    const eye = (x: number) => `<g class="blink">${r(x, 10, 1, 2, INK)}</g>`;
    out += eye(9) + eye(14) + r(10, 13, 1, 1, INK) + r(13, 13, 1, 1, INK) + r(11, 14, 2, 1, INK);
  }
  return `<g shape-rendering="crispEdges">${out}</g>` + fx(s);
}

function cloud(s: Look): string {
  const c = s === "pause" ? "#c5ced6" : "#bfe0fb";
  return (
    `<g fill="${c}"><circle cx="40" cy="64" r="20"/><circle cx="60" cy="48" r="27"/><circle cx="82" cy="64" r="18"/>` +
    `<rect x="28" y="60" width="66" height="26" rx="13"/></g>` +
    `<ellipse cx="50" cy="32" rx="10" ry="5" fill="#fff" opacity=".6"/>` +
    face(s, 48, 72, 58, 60, 71) +
    `<circle cx="39" cy="68" r="4" fill="#f4b8a8" opacity=".8"/><circle cx="81" cy="68" r="4" fill="#f4b8a8" opacity=".8"/>` +
    fx(s, "#4f8fc4")
  );
}

function cat(s: Look): string {
  const body = s === "pause" ? "#c4bfce" : "#c9b8e8";
  const ear = "#f3c6d3";
  return (
    `<path d="M90 92q22 2 16-20" fill="none" stroke="${body}" stroke-width="8" stroke-linecap="round"/>` +
    `<path d="M28 50l2-32 24 16zM92 50l-2-32-24 16z" fill="${body}"/>` +
    `<path d="M34 40l1-13 10 7zM86 40l-1-13-10 7z" fill="${ear}"/>` +
    `<ellipse cx="60" cy="68" rx="38" ry="34" fill="${body}"/>` +
    `<path d="M26 66l12 2M26 76l12-2M94 66l-12 2M94 76l-12-2" stroke="${INK}" stroke-width="2" stroke-linecap="round" opacity=".55"/>` +
    face(s, 47, 73, 62, 60, 76) +
    fx(s, "#7d63b8")
  );
}

function sprout(s: Look): string {
  const [pot, rim, leaf] = s === "pause" ? ["#c4a795", "#b39684", "#a9b8a2"] : ["#e39a6c", "#cf8253", "#7cc47a"];
  return (
    `<path d="M60 44V26" stroke="${leaf}" stroke-width="4" stroke-linecap="round"/>` +
    `<path d="M60 30C50 14 38 14 32 18c2 12 14 18 28 12zM60 30c10-16 22-16 28-12-2 12-14 18-28 12z" fill="${leaf}"/>` +
    `<path d="M32 50h56l-7 50a6 6 0 0 1-6 5H45a6 6 0 0 1-6-5z" fill="${pot}"/>` +
    `<rect x="26" y="40" width="68" height="15" rx="5" fill="${rim}"/>` +
    face(s, 49, 71, 72, 60, 84) +
    `<circle cx="42" cy="82" r="4" fill="#f6c4b0" opacity=".9"/><circle cx="78" cy="82" r="4" fill="#f6c4b0" opacity=".9"/>` +
    fx(s, "#4f9c4d")
  );
}

function penguin(s: Look): string {
  const [body, foot] = s === "pause" ? ["#6d7684", "#c9b79c"] : ["#39465c", "#f2a33c"];
  return (
    `<ellipse cx="45" cy="104" rx="11" ry="5" fill="${foot}"/><ellipse cx="75" cy="104" rx="11" ry="5" fill="${foot}"/>` +
    `<path d="M26 56q-10 16-4 28 8-4 10-14zM94 56q10 16 4 28-8-4-10-14z" fill="${body}"/>` +
    `<rect x="24" y="18" width="72" height="86" rx="36" fill="${body}"/>` +
    `<ellipse cx="60" cy="68" rx="28" ry="32" fill="#fbfaf6"/>` +
    face(s, 48, 72, 56, 60, 68) +
    `<circle cx="40" cy="66" r="4" fill="#f4b8a8" opacity=".85"/><circle cx="80" cy="66" r="4" fill="#f4b8a8" opacity=".85"/>` +
    fx(s, "#4f79b8")
  );
}

const WARNING =
  `<circle cx="100" cy="100" r="15" fill="#e5484d" stroke="#fff" stroke-width="3"/>` +
  `<rect x="98" y="91" width="4" height="11" rx="2" fill="#fff"/><circle cx="100" cy="107" r="2.4" fill="#fff"/>`;

const DRAW: Record<CharacterKind, (s: Look) => string> = { mic, blob, owl, pixel, cloud, cat, sprout, penguin };

/** 回傳角色的 SVG 字串；內容全是程式內建，沒有外部輸入。 */
export function render(kind: CharacterKind, look: Look, warning = false): string {
  return `<svg viewBox="0 0 120 120" role="img">${DRAW[kind](look)}${warning ? WARNING : ""}</svg>`;
}
