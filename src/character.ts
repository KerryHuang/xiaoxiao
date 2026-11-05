import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import type { CharacterKind, Look } from "./api";
import { render } from "./characters";

interface CharacterView {
  kind: CharacterKind;
  look: Look;
  warning: boolean;
  hint: string;
}

const sprite = document.getElementById("sprite")!;
let last = "";
let idle = false;

function draw(v: CharacterView) {
  idle = v.look === "idle";
  sprite.title = v.hint;
  const svg = render(v.kind, v.look, v.warning);
  // 狀態沒變就不重畫，動畫才不會一直從頭開始
  if (svg !== last) {
    sprite.innerHTML = svg;
    last = svg;
  }
}

/** 待命時每隔幾秒眨一次眼；用計時器而不是無限循環的 CSS 動畫，平常才不耗電。 */
function blinkLater() {
  setTimeout(() => {
    const quiet = matchMedia("(prefers-reduced-motion: reduce)").matches;
    if (idle && !quiet) {
      sprite.classList.add("blinking");
      setTimeout(() => sprite.classList.remove("blinking"), 140);
    }
    blinkLater();
  }, 3000 + Math.random() * 4000);
}

/** 按下後移動超過這個距離才算拖曳，否則放開時算點一下。 */
const DRAG_PX = 4;
let pressed: { x: number; y: number } | null = null;

document.addEventListener("pointerdown", (e) => {
  // Mac 的 Control＋點一下是右鍵
  if (e.button !== 0 || e.ctrlKey) return;
  pressed = { x: e.screenX, y: e.screenY };
  document.documentElement.setPointerCapture(e.pointerId);
});
document.addEventListener("pointermove", (e) => {
  if (pressed && Math.hypot(e.screenX - pressed.x, e.screenY - pressed.y) > DRAG_PX) {
    pressed = null;
    getCurrentWindow().startDragging();
  }
});
document.addEventListener("pointerup", () => {
  if (pressed) invoke("character_click");
  pressed = null;
});
document.addEventListener("pointercancel", () => (pressed = null));
document.addEventListener("contextmenu", (e) => {
  e.preventDefault();
  pressed = null;
  invoke("character_menu");
});

async function start() {
  await listen<CharacterView>("character-view", (e) => draw(e.payload));
  draw(await invoke<CharacterView>("character_state"));
  blinkLater();
}

start();
