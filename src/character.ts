import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
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

async function start() {
  await listen<CharacterView>("character-view", (e) => draw(e.payload));
  draw(await invoke<CharacterView>("character_state"));
  blinkLater();
}

start();
